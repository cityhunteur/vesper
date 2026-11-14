---
status: proposed
owner: cityhunteur
last_reviewed: 2026-11-14
---

# 0010. No container is root on the System VM: dockerd remaps every container to an unprivileged user namespace

Date: 2026-11-14. Supersedes: none. Superseded by: none.

## Context and problem

On 2026-11-14 the owner put one more actor inside ADR 0009's threat model (D-034): a container that is given the
System VM's root, for example `docker run --privileged --pid=host -v /:/host …`. Until then `docs/SECURITY.md`
called that container "outside the boundary".

Today dockerd in the System VM grants what the client asks for. Such a container is root on the System VM. It can
enter any namespace, so a share server's private network namespace (ADR 0009) does not keep it out. It can also
trace a share server, read a Firecracker process's memory and load kernel modules. No part of the share design can
stop a process that is root on the same kernel. The only defence is that no container becomes that root.

The requests come from a process of the same user on macOS, which the threat table trusts. The actor here is the
workload inside the container, which the user may not trust, and which must not gain the System VM by being started
with broad flags.

A spike on the live System VM (dockerd 28.3.3, kernel 6.12.115-virt; `run/GH-160/spike-userns-remap/out.txt`)
measured what `userns-remap` does there:

| Request | Without remapping | With `userns-remap` |
|---|---|---|
| `-v /:/host`, default capabilities | reads `/etc/shadow` and a root-only file under `/run` | both denied; `setns` into a namespace bound under `/run` denied |
| the same with `--cap-add ALL --security-opt seccomp=unconfined` | root on the System VM in all but name | the same denials; the capabilities hold only inside the container's user namespace |
| `--privileged` | root on the System VM | dockerd refuses the request |
| `--pid=host`, `--network=host` | granted | dockerd refuses the request |
| `--device /dev/vda` | reads the disk | the node belongs to an unmapped uid; open denied |
| a `local` volume bound to `/run` | reads root-only files | denied |
| `docker exec --privileged` | root on the System VM | full capabilities inside the user namespace only |
| `-v <macOS share>:/s` | reads and writes | reads a `0600` file, writes, and `:ro` stays read-only |
| `--userns=host` with any of the above | — | **everything above is granted again** |

Two more facts from the spike:

- The System VM's virtio-fs mounts do not check the caller's uid. Uid 100000 read a `0600` file that root had
  created. Shares therefore keep working for a remapped container, and uid is no protection for share contents.
- A remapped container with `CAP_NET_ADMIN` in its own namespace made the kernel load a link-type module
  (`ip link add … type dummy`). It cannot load a module of its choice.

## Decision

We will run dockerd in the System VM with `userns-remap`, so that every container's root is an unprivileged uid on
the System VM, and we will refuse every request that would start a container outside that mapping.

1. **Remapping is the default and the only shipped mode.** The overlay creates the `dockremap` user and its
   subordinate uid and gid ranges and sets `userns-remap` in dockerd's configuration.
2. **`--userns=host` is refused before dockerd acts on it**, by an authorisation plugin that Vesper owns (dockerd's
   `--authorization-plugin` interface; a small Go program in this repository, standard library only). It refuses a
   container create whose `HostConfig.UsernsMode` is `host`, and the plugin endpoints (`/plugins/…`), because a
   managed plugin runs outside the mapping. If the plugin is not running, dockerd refuses every request; that is the
   interface's own behaviour and the one we want. The first milestone of GH #194 lists every Docker API route to an
   unmapped container and proves each one closed; the two named here are the known ones, not the proven set.
3. **What `docker run` no longer accepts** is stated to the user, not hidden: `--privileged`, `--pid=host`,
   `--network=host` and `--userns=host` fail with dockerd's or the plugin's message. `--cap-add`, `--device` and
   `--security-opt` are accepted and confer nothing on the System VM.
4. **Vesper's own tools stop using a privileged container as a root shell.** `scripts/systemvm/data-disk-suite.sh`
   and the operator's shell move to the agent channel (vsock port 1024), which only a macOS process of the same user
   can reach.
5. **One switch for the upstream Firecracker suite.** That suite runs Firecracker in containers that need
   `/dev/kvm`, TAP devices and real root (`scripts/systemvm/upstream-suite/run.sh`). The launch config gains a field
   that starts dockerd without remapping for that boot. Only the macOS side can set it, the System VM reports it in
   `HEALTH`, and `docs/SECURITY.md` states that ADR 0009's row does not hold in that mode. No shipped default or
   documented user workflow sets it.
6. **Directories under `/run/vesper` that hold per-microVM state become owner-only.** The spike listed
   `/run/vesper/microvm` from a remapped container.

## Alternatives considered

- **Refuse the dangerous flags and keep containers in the host user namespace.** Smaller, and containers are
  otherwise unchanged. It is a deny-list over a large API: a missed field (volume driver options, a new mount type)
  gives root back. With remapping a missed field yields uid 100000.
- **A microVM per container (GH #53).** The strongest boundary, and still the mechanism `design-v0.md` requires
  against a container that attacks the shared kernel. It is the largest piece of work, and it depends on the shares
  ADR 0009 builds, so GH #160 could not close first.
- **Keep the actor out of scope** and document it, as Docker does for `--privileged`. Rejected by the owner (D-034).
- **A second System VM for runc containers.** It isolates them from microVMs completely and doubles the memory,
  image store and share plumbing. Not pursued.
- **A hardened runtime such as Sysbox,** which runs `--privileged` workloads inside a user namespace. It would keep
  more flags working at the cost of a new runtime dependency with a kernel-version contract of its own.

## Rationale and consequences

Remapping moves the guarantee from "we listed every dangerous request" to "a container's root is not the System
VM's root", which the kernel enforces for every request we did not think of. The plugin then has one job with a
small surface.

- **Compatibility cost, stated plainly.** Under remapping dockerd refuses `--privileged` outright; it does not
  confine it. Workloads that need it (Docker-in-Docker, tools that mount filesystems or load modules) do not run on
  the Docker path. `--network=host` is refused as well. These are deviations from Docker and get entries in the
  conformance notes and, where the parity matrix covers them, `compat-exceptions.yaml`.
- **Not a kernel boundary.** A remapped container still shares the System VM's kernel and can reach more of it with
  capabilities in its own namespace (module autoload is one example). A kernel exploit stays the risk that GH #53
  answers. This ADR closes the configured route to root, not the exploited one.
- **Shares are readable by whoever is given their path.** A container started with `-v /:/host` sees every macOS
  directory shared with the System VM, because that is the bind the user asked for and virtio-fs does not check
  uids. What it cannot do is reach a microVM's share server or another task's private state.
- **Image store.** dockerd keeps a separate store per mapping (`/var/lib/docker/100000.100000`). Images and volumes
  in the old store are not visible after the change and are pulled or built again. The old store stays on the data
  disk until the owner approves deleting it.
- **ADR 0009's condition 3** ("a test proves that a Docker container cannot reach it") is then tested with the
  broadest request Docker accepts, not only with a default container.

Unknown, each resolved by a milestone of GH #194 before this ADR is accepted as built:

- Whether the Vesper runtime (`io.containerd.vesper.v2`) works when dockerd passes it a remapped root filesystem and
  uid mappings. The shim builds a guest image from that tree; ownership shifted by 100000 would be wrong inside the
  guest.
- Whether `docker build`, Compose, published ports, the cross-architecture path and the DNS suite pass unchanged.
- Whether the plugin sees every route (BuildKit, Swarm services, `docker cp`, container update).

## Implementation and migration impact

Tracked in GH #194; an ExecPlan follows acceptance.

1. Enumerate the Docker API routes to an unmapped or host-root container, with a failing test for each.
2. Overlay: `dockremap`, subordinate ranges, dockerd configuration; the authorisation plugin and its service.
3. Agent-channel replacements for the privileged root shell; the launch-config switch for the upstream suite.
4. A runtime suite (`make systemvm-confine-suite`) with the rows of the table above, and the ADR 0009 step: a
   container started with every accepted flag cannot connect to a share server or enter its namespace.
5. Rerun the Docker suites; record each new refusal as a deviation; update `design-v0.md`, `docs/SECURITY.md` and
   the conformance notes.

GH #160 milestones 3 to 7 do not depend on this work and continue. GH #160 closes only after GH #194.

## References

- D-034, D-021 and D-032 in `docs/design-docs/decision-log.md`; ADR 0009; GH #194, GH #160, GH #53.
- `run/GH-160/spike-userns-remap/` (the spike script and its output).
- `docs/specs/design-v0.md` (threat table), `docs/SECURITY.md`.
- `scripts/systemvm/data-disk-suite.sh`, `scripts/systemvm/upstream-suite/run.sh`,
  `systemvm/overlay/etc/init.d/vesper-services`.

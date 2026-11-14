---
status: accepted
owner: cityhunteur
last_reviewed: 2026-11-14
---

# 0010. No container is root on the System VM: dockerd remaps every container to an unprivileged user namespace

Date: 2026-11-14. Supersedes: none. Superseded by: none. Revised the same day after an llm-council design review
(`run/GH-194/council-adr0010.md`: 2 High, 9 Medium, 4 Low) and accepted by the owner with those fixes (D-035).

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

Two spikes on the live System VM (dockerd 28.3.3, kernel 6.12.115-virt) measured what `userns-remap` does there.
The second (`run/GH-194/spike2/out.txt`) ran each probe once and kept its exit status; the table cites it. The first
(`run/GH-160/spike-userns-remap/out.txt`) is kept as history: three of its probes did not test what they claimed.

| Request, always with `-v /:/host` | With `userns-remap` |
|---|---|
| default capabilities | a root-only file under `/run`, the disk, `/dev/kvm`, `/dev/vsock`, the Docker and containerd sockets: all denied; `setns` into a namespace bound under `/run`: denied |
| `--cap-add ALL`, seccomp and AppArmor unconfined | the same denials; `modprobe nbd` denied (`lsmod` before and after agrees); `sethostname` changes the container's own name only |
| `--uts=host`, `--cgroupns=host`, `--security-opt systempaths=unconfined`, each with all capabilities | accepted; cannot set the System VM's hostname, cannot write the cgroup tree, cannot `mknod` |
| `--device /dev/kvm`, `--device /dev/vsock` | accepted; the node belongs to an unmapped uid and cannot be opened |
| `--privileged`, `--pid=host`, `--network=host` | dockerd refuses the request |
| `--ipc=host` | the container fails to start (runc cannot mount `mqueue`) |
| `docker build` with the legacy builder | build steps run remapped |
| `-v <macOS share>:/s` | reads a `0600` file, writes, and `:ro` stays read-only |
| `--userns=host` | **root on the System VM again**: it read the root-only file |

What a remapped container can still do, from the same run:

- **Read another container's named volume** through `/host/var/lib/docker/100000.100000/volumes/…`, because every
  container shares one mapping.
- **Read world-readable state:** `/run/vesper/microvm`, `/var/lib/vesper/workloads` and `/var/log/docker.log`.
- **Read and write every macOS share** it is given a path to. The System VM's virtio-fs mounts do not check the
  caller's uid.
- **Make the kernel load a link-type module** (`ip link add … type ifb` loaded `ifb`), and open `/dev/net/tun`,
  which gives it devices in its own network namespace only.

Not measured: whether an unconfined container can connect a vsock socket. The spike's connects to CIDs 1, 2 and 3
failed, but its output does not show whether the socket or the connect failed. BuildKit (`/session`, `/grpc`) was
not measured either: the System VM has no `buildx`.

## Decision

We will run dockerd in the System VM with `userns-remap`, so that every container's root is an unprivileged uid on
the System VM, and we will refuse every Docker API request that is not known to stay inside that mapping.

1. **Remapping is the default and the only mode a normal boot has.** The overlay creates the `dockremap` user and
   its subordinate ranges and sets `userns-remap` in dockerd's configuration.

2. **A default-deny authorisation plugin.** Vesper owns it: a Go program in this repository, standard library only,
   on dockerd's `--authorization-plugin` interface.
   - It parses the request URI, cleans the path and strips the API version, then looks the method and path up in
     an allow-list. A request that is not listed is refused. Swarm, services, nodes, tasks, secrets, configs and
     managed plugins are not listed. A route joins the list only with a test that shows it cannot start an
     unmapped container; BuildKit's `/session` and `/grpc` start outside the list for that reason.
   - **Container create is refused unless the plugin can read it.** dockerd passes a body to a plugin only when
     it is JSON, has a declared length and is under 1 MiB, yet dockerd itself decodes any body. So the plugin
     refuses a create with no body, decodes the body with Go's `encoding/json` into a struct with dockerd's field
     names (the same case-insensitive, last-key-wins rules dockerd applies), and refuses unless
     `HostConfig.UsernsMode` is empty.
   - **Two more create rules,** each a way to act as root outside the mapping that no test bounds yet. A runtime
     other than the default is refused, so dockerd cannot be asked to start another runtime by name. Volume driver
     options, and drivers other than `local`, are refused on volume create and on a container's mounts: with
     options, the `local` driver makes dockerd, as root, perform a mount that the client describes.
   - **Fail closed.** dockerd refuses every request when the plugin does not answer. The plugin is an OpenRC
     service under `supervise-daemon`, started before dockerd, which removes a stale socket before it binds;
     `vesper-services` waits for its activation before it starts dockerd.
   - It logs each refusal (method, path, reason; never a body) to a bounded log.

3. **What `docker run` no longer accepts** is stated to the user: `--privileged`, `--pid=host`, `--network=host`,
   `--ipc=host` and `--userns=host` fail. `--cap-add`, `--device` and `--security-opt` are accepted and give
   nothing on the System VM beyond what the list above measures.

4. **Vesper's own callers of a privileged container change.** There are four:
   - `vesper-microvm test-share-reach` (the share-network suite) uses `--cap-add ALL --security-opt
     seccomp=unconfined` on the bridge; its host-network row becomes "refused by dockerd".
   - `scripts/systemvm/data-disk-suite.sh` uses new read-only `vesper-data` verbs over the agent.
   - `scripts/systemvm/upstream-suite/run.sh` (two calls) runs in the unmapped boot below.
   - No general root shell is added to the agent. Diagnosis uses the agent's named verbs and the serial console.

5. **One unmapped boot, for the upstream Firecracker suite only.** That suite needs `/dev/kvm` and real root in
   containers.
   - The switch is a kernel command-line token, `vesper.containers=unmapped`. The runner already writes a
     temporary launch config for its boot; the token goes in that config's command line, so it lasts one boot and
     nothing inside the System VM can set it.
   - **Its own data disk.** Each data disk records the mode of the first dockerd that used it (`mapped` or
     `unmapped`). A boot in the other mode does not start dockerd and says why in `HEALTH`. The suite gets a
     separate disk image. A normal disk therefore never runs unmapped, and old containers with a restart policy
     never start as root.
   - The runner keeps its saved launch config outside every shared directory. Today it sits in a directory shared
     read-write into the suite's boot.
   - The launcher prints the mode it booted with; `HEALTH` repeats it from inside as a cross-check. A test shows
     that `make systemvm-up` after a suite run is remapped.
   - `docs/SECURITY.md` states that ADR 0009's host-root row does not hold in that boot.

6. **Private state is not readable by other uids.** The work inventories what Vesper writes under `/run/vesper`,
   `/var/lib/vesper` and `/var/log` (console logs, workload records, leases, launch records) and creates each as
   owner-only, including on log rotation. Tests bind both the directory and a single file into a container.

7. **vsock.** The agent and the other vsock listeners refuse a peer that is not the macOS host (CID 2), and the
   `vsock_loopback` module is blocked from loading. A test with an unconfined container connects to CIDs 1, 2 and
   the System VM's own.

8. **Existing Docker data is not adopted silently.** dockerd keeps a separate store per mapping, so images, named
   volumes and stopped containers from before the change are invisible afterwards. Images are rebuilt or pulled.
   Volumes are data: `vesper-data legacy status` counts them, `HEALTH` and `make docker-up` report a populated old
   store, and `vesper-data legacy import-volume NAME` copies one into the new store with its ownership shifted.
   The old store is never deleted without the owner's approval. Going back to an older build shows the old store
   and none of the writes made since; the release note says so.

## What this guarantees, and what it does not

- **Guaranteed:** no container, whatever flags started it, is root on the System VM. It cannot enter a microVM's
  share namespace, signal or trace a share server, read root-owned microVM state, or reach the Docker, containerd
  or agent endpoints.
- **Not guaranteed: isolation between containers.** They share one mapping and one kernel. A container given `/`
  reads other containers' volumes and every macOS share. That is Docker's model, and a microVM per container
  (GH #53) is the answer to it.
- **Not a kernel boundary.** A remapped container reaches more of the kernel with capabilities in its own
  namespace. A kernel exploit stays the risk that GH #53 answers.

## Alternatives considered

- **Refuse the dangerous flags and keep containers in the host user namespace.** Smaller, and containers are
  otherwise unchanged. A missed field gives root back; with remapping a missed field yields uid 100000.
- **Rootless dockerd.** The daemon itself runs unprivileged, so `--userns=host` and `--privileged` mean the
  daemon's own namespace and no plugin is needed. It brings new runtime dependencies (RootlessKit and a user-mode
  network stack), routes all container networking through them, which the port relay, DNS and Compose suites would
  have to be proven on again, and loses cgroup limits without delegation. It is the fallback if the plugin's
  allow-list proves unmaintainable.
- **A microVM per container (GH #53).** The strongest boundary. It depends on the shares ADR 0009 builds, so
  GH #160 could not close first.
- **Keep the actor out of scope,** as Docker does for `--privileged`. Rejected by the owner (D-034).
- **A second System VM for runc containers.** Doubles the memory, image store and share plumbing.
- **Sysbox,** which confines `--privileged` in a user namespace: a new runtime with a kernel contract of its own.
- **Patch dockerd to drop the `host` user-namespace mode.** No plugin, at the cost of carrying a fork.

## Rationale and consequences

Remapping moves the guarantee from "we listed every dangerous request" to "a container's root is not the System
VM's root", which the kernel enforces for requests we did not think of. The plugin closes the one switch that
turns remapping off, and it is default-deny so that a new Docker route is refused until someone tests it.

- **Compatibility cost.** dockerd refuses `--privileged` under remapping; it does not confine it. Docker-in-Docker
  and tools that mount filesystems do not run on the Docker path, and neither does `--network=host`. Routes outside
  the allow-list (Swarm, managed plugins, at first BuildKit) fail. Each is a recorded deviation.
- **Every Docker call passes through the plugin** twice (request and response). Its latency and its behaviour
  while dockerd or the plugin restarts are measured in the work, not assumed.
- **The Vesper runtime is not affected.** `io.containerd.vesper.v2` is reached through `ctr` on containerd's
  root-only socket. The plugin refuses `--runtime` with any other value than the default, so dockerd never hands
  the shim a remapped tree; GH #53 revisits that rule. The containerd socket and its other namespaces are outside
  remapping and stay root-only.
- **ADR 0009's condition 3** is then tested with the broadest request Docker accepts.

Unknown, each resolved by a milestone of GH #194:

- Whether Compose, published ports, the cross-architecture path and the DNS suite pass unchanged.
- Whether BuildKit builds stay mapped, and so whether `/session` and `/grpc` can join the allow-list.
- Whether an unconfined remapped container can connect a vsock socket at all.

## Implementation and migration impact

The ExecPlan is `docs/exec-plans/active/2026-11-14-GH194-no-container-is-systemvm-root.md`. In order:

1. The plugin, with unit tests for the body rules and path handling, and the route inventory.
2. Overlay: remapping, the plugin service and its ordering, the disk mode record, `HEALTH` fields.
3. Private state modes and the vsock peer check.
4. The four callers, the unmapped boot and its disk, and the saved-config move.
5. `make systemvm-confine-suite`: the rows of the table above, the body-size and path cases against the live
   plugin, plugin loss, and the ADR 0009 step.
6. The legacy-store verbs; rerun every Docker suite; record deviations; update `design-v0.md`, `docs/SECURITY.md`
   and the conformance notes.

GH #160 milestones 3 to 7 do not depend on this work. GH #160 closes only after GH #194.

## References

- D-034, D-035, D-021 and D-032 in `docs/design-docs/decision-log.md`; ADR 0009; GH #194, GH #160, GH #53.
- `run/GH-194/spike2/` (script and output), `run/GH-160/spike-userns-remap/` (first spike),
  `run/GH-194/council-adr0010.md` (the review).
- moby v28.3.3 `pkg/authorization/authz.go` (when a plugin is given the body).
- `docs/specs/design-v0.md` (threat table), `docs/SECURITY.md`.
- `systemvm/overlay/usr/local/bin/vesper-microvm` (`test_share_reach`), `scripts/systemvm/data-disk-suite.sh`,
  `scripts/systemvm/upstream-suite/run.sh`, `systemvm/overlay/etc/init.d/vesper-services`.

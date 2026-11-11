---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-09
---

# Operating the Vesper containerd runtime

How to deploy, verify and troubleshoot `containerd-shim-vesper-v2` (runtime `io.containerd.vesper.v2`) inside the
System VM. Companion to [systemvm-bringup.md](systemvm-bringup.md), which describes the pieces; this page is the
runbook (GH #26).

## Deploy

1. `make systemvm-prepare` builds the launcher, fetches the pinned packages and builds the overlay, which carries
   the shim at `/usr/bin/containerd-shim-vesper-v2` (`make shim-build` alone rebuilds the binary).
2. `make systemvm-up` boots the System VM; containerd and dockerd come up supervised, `vesper-binfmt` registers
   qemu, `vesper-port-relay` and `vesper-docker-relay` listen on vsock.
3. `make docker-up` exposes the Docker API on `run/vesper/docker.sock` and publishes `-p` ports on localhost.

A task runs on the Vesper runtime when a client asks for it: `ctr -n <ns> run --runtime io.containerd.vesper.v2
IMAGE ID CMD` (from the host: `systemvmctl control ctr run NAME WORDS…`). Docker's own containers keep using runc.

## Verify

| Check | Command | Expect |
|---|---|---|
| Whole runtime contract | `make ctr-conformance` | `ctr conformance passed (74 steps, …)` and `run/systemvm/ctr-conformance-report.json` |
| Task lifecycle only | `make systemvm-shim-suite` | 22 steps incl. `task_metrics` |
| Exec, streams and terminals | `make systemvm-exec-suite` | 19 steps, including signals and resize |
| Root drive cache | `make systemvm-bundle-suite` | 15 steps |
| Restart behaviour | `make systemvm-restart-suite` | 18 steps |
| Committed capture still valid | `make ctr-conformance-check` | `ctr conformance report valid` (also run by the `ctr Conformance` workflow) |

## Troubleshoot

| Symptom | Where to look | Likely cause and recovery |
|---|---|---|
| `ctr run` fails with `guest session pN: … ERROR {…}` | `systemvmctl control microvm log <id> 100` | runc rejected the config or could not execute argv. The structured error names the failure; the console contains its diagnostics. `/bin/sh` is needed only when the requested command uses it. |
| `ctr run` fails with `InvalidArgument: bundle …` | the error text | Bundle validation: the reason and hint name the field (`process.args`, `cwd`, mounts). Fix the container spec. |
| `ctr run` fails with `FailedPrecondition: rootfs: …` | the error text, `systemvmctl control ctr cache` | Root drive build or copy failed: free space in `/var/lib/vesper/rootfs-cache` or the bundle, or `mke2fs` missing. `rm -rf /var/lib/vesper/rootfs-cache` in the guest resets the cache. |
| Task shows `RUNNING` but the microVM is gone | `systemvmctl control microvm status <id>`, `ctr state-file <id>` | Kill the shim (`systemvmctl control ctr kill-shim <id>`): containerd runs the shim's `delete`, which reconciles from `vesper-task.json` and cleans the jail. |
| Leftover jails, cgroups or processes | `systemvmctl control ctr leftovers` | `DIRTY firecracker=N shims=N jails=N tasks=N containers=N`; `ctr task delete -f`, `ctr container delete`, then `systemvmctl control microvm cleanup <id>`. |
| `ctr task metrics` returns nothing | `systemvmctl control ctr metrics <id>` | The VMM must be running in its cgroup (`/sys/fs/cgroup/firecracker/<id>`); `status` must say RUNNING. |
| containerd restarted | `systemvmctl control health` | Tasks survive; containerd reconnects to live shims through the bundle's bootstrap record. In-flight `ctr` clients get `EOF` and must retry. |
| `--platform linux/amd64` says `exec format error` | `systemvmctl control arch status` | `vesper-binfmt` did not register qemu; `rc-service vesper-binfmt restart` in the guest. Other architectures are unsupported (TD-040). |
| Published port not reachable on localhost | `run/vesper/ports.json`, `run/vesper/vesper-proxy.log` | `conflict`: the macOS port is taken; `unsupported`: UDP or a non-loopback host ip. The container stays reachable inside the guest. |
| Bind mount is empty | `systemvmctl control share list` | The host path is not in `VESPER_SHARES`; only configured shares map host paths. |
| Whole System VM gone | `run/systemvm/vmlauncher.log`, `console.log` | `make systemvm-down && make systemvm-up`; everything in the guest is tmpfs and is rebuilt from the pinned image. |

## Recovery

- Restart only the proxy: `make docker-down && make docker-up` (published ports are republished; the launcher
  survives since the SIGPIPE fix).
- Restart the runtime services: `systemvmctl control kill-service containerd` (supervise-daemon respawns it).
- Reset everything: `make docker-down && make systemvm-down && make systemvm-up`.

## Known limits

Rootfs cache growth (TD-035), at most 16,125 exec sessions per task (TD-041) and unresolved runc
pause/snapshot compatibility (TD-042). Hardware suites run on developer machines; CI validates their capture (TD-005).

# shim/

`containerd-shim-vesper-v2` runs each containerd task in a jailed Firecracker microVM inside the System VM.
Runtime name: `io.containerd.vesper.v2`. See [ADR 0004](../docs/design-docs/adr/0004-containerd-shim-in-go.md)
and [ADR 0006](../docs/design-docs/adr/0006-guest-exec-via-runc-delegation.md).
The Go module pins containerd 2.1.5 and builds static Linux arm64 binaries with `make shim-build`.

## Interface

- `cmd/containerd-shim-vesper-v2`: registers the task plugin and starts the shim manager.
- `internal/manager`: starts one shim per task. After a shim dies, `Stop` reads `vesper-task.json`,
  reports the recorded exit or 137, removes the microVM and unmounts the bundle.
- `internal/bundle`: validates OCI bundles before mounting. `Inspect` derives a cache key from the ordered
  overlay lower-layer chain. `GuestConfig` copies the process into an OCI spec for `/newroot` inside the microVM.
  `GuestProcess` preserves argv, environment, user and cwd, removes host security labels and grants the guest's
  capabilities. Both use the microVM as the isolation boundary. The config joins `/proc/1/ns/net`, which is
  the existing guest network namespace. It creates no namespace and installs no second seccomp filter.
- `internal/backend`: builds one digest-verified ext4 cache entry per layer chain and copies a private drive
  into each bundle. Directory bundles use the direct fallback. `vesper-rootfs.json` records the strategy,
  digest, size, timing and cache identity. Configs travel separately, so they never change the cached drive.
  `Launch` boots `vesper-microvm --guest-exec`. `OpenSession` connects to its private `v.sock`, sends the
  `CONNECT <port>` preface and pumps the containerd FIFOs over three independent vsock ports.
- `internal/task`: implements init and exec `Start`, `State`, `Kill`, `Wait`, `Delete`, `ResizePty` and `CloseIO`.
  A task follows created → running → stopped → deleted. Init exit waits for running exec exit events.
  Every init transition updates `vesper-task.json`. Exec records remain in memory; a dead shim destroys
  the entire microVM. `Stats` reports the VMM's cgroup v2 metrics. `Pids` reports its System VM pid.
  `Pause`, `Resume`, `Checkpoint` and `Update` remain unsupported.
- `cmd/vesper-authz` and `internal/authz`: dockerd's authorisation plugin in the System VM (ADR 0010, GH #194),
  standard library only. `internal/authz/policy.go` holds the allow-list of Docker API routes and the body rules
  for container and volume create; a request that is not listed, or a create whose body dockerd did not pass, is
  refused. `go test ./internal/authz` covers path handling, key case, duplicate keys and missing bodies. Built by
  `build-shim.sh` and installed as `/usr/bin/vesper-authz`.
- `internal/bundle/size.go` (TD-048, D-051): `SizeOf(spec, systemCPUs)` reads the microVM's machine from the task's OCI resources: a
  memory limit of L MiB gives a guest of L + 152 + L/32 MiB, 8 MiB more for each vCPU after the first and 112 MiB more
  above 2,048 MiB, never under 256 (the terms are fitted to `run/TD-048/measurements.txt`); a CPU set gives its count of
  vCPUs, else a quota gives quota/period rounded up, at most the System VM's CPUs. `Size.LaunchArgs()` gives `--mem-mib`,
  `--vcpus` and `--cpu-quota QUOTA:PERIOD` for `vesper-microvm launch`, nothing for the default machine.
  `GuestResources(spec)` carries the memory limit and the CPU quota into the guest's config, where runc sets them on the
  workload's cgroup; nothing else of `linux.resources` is carried. `Stats` still reads the microVM's cgroup, so a limited
  task's memory limit reads as its ceiling.
- `internal/shares`: maps a task's bind mounts from macOS shares onto its microVM's NFS exports (GH #160 M4, M5).
  `MakePlan` turns each bind whose resolved source lies under a virtio-fs mount into an export and a guest share,
  directories and single files (GH #208), refusing sources that hold another share; binds from several shares are one server's sources
  (GH #207), grouped by share in the `unfs.Args` record (`internal/unfs/args.go`); the task service passes the plan's `--share-net --unfs-*`
  arguments to `vesper-microvm launch` and the list to the guest as the annotation `io.vesper.shares`.
  `vesper-console shares mount|unmount` (`guest_linux.go`) mounts them under `/newroot` in the guest.
  `fanout.go` is the event hop (M6): `vesper-unfs fanout` maps replayed host paths to each microVM's exports and
  sends them, from a bounded coalescing queue, to `vesper-console shares replay` on guest vsock port 1026.
- `cmd/vesper-unfs`, `internal/unfs` and `internal/nfsprobe`: the per-microVM share server's sandbox (ADR 0009,
  GH #160 M3) and a test NFSv3 client. `vesper-unfs run` opens each source with `openat2` beneath the share mount,
  clones it into a detached mount tree, and starts `init` in new mount, pid, ipc and uts namespaces, which attaches
  the trees in a tmpfs root, binds read-only sources again inside read-write ones, pivots, drops privileges, loads a
  seccomp filter against user namespaces and execs `unfsd`. `vesper-unfs probe` runs the escape battery against a
  server, and `run --selfcheck` runs the escape checks inside the sandbox in place of `unfsd`. `protect.go` (TD-073, D-053): the list of objects the patched server keeps
  read-only by device and inode number (`ProtectList`: the root of each read-only source as a place, and the numbers
  recorded across the server's restarts by `RecordProtectIDs`); at a restart `GuardSources` (`sandbox_linux.go`) makes
  read-only a read-write source whose path now lies under a recorded object, and the re-attach does the same for a
  read-write file source; a replaced read-only file is held open (`fileWatch.pins`) so its number stays its own. The server's list is the supervisor's own file (`WriteList`), attached in the sandbox; `AppendSeen` adds a
  file's number to it before the re-attach puts the file at a protected place (TD-078, D-056). `anchor_linux.go` (TD-080, D-058): the directories on the way to a read-only source inside a read-write one,
  and the directory at a read-only directory source's path (`chainOf`, `anchorsOf`, by identity), which the server
  is told of at a start and, all of them again, by the watcher's `checkChains` when one changed, so that a task
  cannot rename them. A chain is one walk, each directory opened in the one before it; a server has at most 1,024
  anchors, and a start raises the supervisor's limit of open files to what it can need (`filesNeeded`,
  `ensureFiles`) or fails; a server that needs a watcher and cannot have one does not run (`watchServer`; TD-082,
  D-061). `spell_linux.go` (TD-082, D-061): sources that spell one directory in two ways get one spelling at a
  start, that of the outermost directory source that holds them (`respell` walks and checks by identity, `spellAll`
  is the rule), so that the nests, the holders, a directory bound both ways and a read-write source inside a
  read-only one are found by the text as before. The directory now at a read-only directory source's path is told as a read-only object too
  (`leafList`, an `s` line; `tellLeaves` tells it again after every file), so that a task cannot write in a
  directory that macOS put there (TD-081, D-059). `hold_linux.go` (TD-076, D-055):
  each recorded object gets a read-only mount of its own that outlives the supervisor, in a mount namespace that only
  the holds are in (its root is a small tmpfs), kept by a bind of its namespace file beside the record
  (`ID.prot.d/ns`); `ReleaseHolds` (`vesper-unfs release`) takes off that bind and removes that file and the
  directory, and `CountHolds`
  (`vesper-unfs holds`) counts them. `reattach_linux.go`
  (D-052): the supervisor looks at each file source's path once a second, attaches a file that replaced it on macOS
  in the sandbox's mount namespace (beneath the old mount, then the old one is removed), places a read-only nest
  that is gone again and tells the guest through `--guest-sock` (`shares.NotifyBind`) until it answers that its
  binds are the export's file. When a read-write directory source holds the file's path (`Holders`), the new file
  is taken through that source's export in the sandbox, so a file under a read-only nest stays read-only.
  `shares.rebindFile` in the guest's replay binds the export's file again at its destinations and answers `BIND N
  same|again|failed|none`. `go test
  ./internal/unfs ./internal/nfsprobe` covers parsing, nesting, exports, RPC encoding and the filter program.
  Installed as `/usr/bin/vesper-unfs`. `vesper-unfs probe --hold-remove PATH --until FILE` or `--hold-create` (`nfsprobe.Battery.Held`) sends a REMOVE or
  an UNCHECKED CREATE and holds it before its last bytes until FILE exists, as a task can do to its own server
  (TD-079).
- `cmd/vesper-runc` and `internal/runcwrap`: dockerd's default runtime in a normal boot (TD-062, D-036). Every
  runc command but `create` replaces itself with `/usr/bin/runc`. For `create` it runs runc, and then asks each
  bind source on a macOS share for its attributes with `statx(AT_STATX_FORCE_SYNC)`, as the container process's
  uid and gid (mapped through the spec's id mappings) on a locked thread with changed filesystem ids, so the
  System VM caches the owner the container sees rather than uid 0. It skips a process that is root on the System
  VM, never fails a create, gives up after 2 s and writes one line to `/run/vesper/runc-refresh`. `go test
  ./internal/runcwrap` covers runc's argument forms and the id mapping. Installed as `/usr/bin/vesper-runc`.
- `cmd/vesper-console`: uses the already pinned containerd `go-runc` and `console` libraries to receive
  runc's terminal master. It runs detached `runc create/start` or `exec`, reaps the guest process and
  relays its terminal through stdin/stdout. A private Unix socket applies terminal resize.
  The same binary supplies `proof-pty` for the hardware suite's `vesper-ptyrun` wrapper.

## Guest session protocol

`vesper-microvm-init` mounts proc, sysfs, cgroup2 and devpts, populates the workload's `/dev` and starts
`tini-static` with `socat` on microVM vsock port 1025. Each connection invokes the static shell script
`vesper-guest-session`. These listeners stay inside the microVM; no host socket or entitlement is added.

The shim sends `RUN|EXEC <base-port> <byte-count> <tty>` followed by the exact config bytes.
The guest writes `config.json` verbatim and replies `READY <base-port>`. The shim connects stdin,
stdout and stderr on `base-port`, `base-port+1` and `base-port+2`. A terminal merges output on stdout.
The guest returns `STARTED <guest-pid>` and later `EXIT <status>`, or `ERROR <JSON>` on runtime failure.
The shim bounds control replies. Requests validate numeric fields; configs may contain at most 1 MiB.

`SIGNAL <base-port> <signal> <all>`, `RESIZE <base-port> <rows> <cols>`, `ABORT <base-port> 0 0` and
`RELEASE <base-port> 0 0` use separate control connections. Signal numbers are 0..64.
Terminal dimensions are 1..65535. Init uses port 1028; execs reserve consecutive groups of four ports. When an exec is
deleted its group goes back to the shim's free list: at once if the guest never heard of it, else after `RELEASE`
removed the ended session's directory (`OK`; refused for 1028 or a session that has not ended). A group the guest does
not release is never reused (TD-041).
The session key `p<base-port>` also names its guest directory and console breadcrumbs.
The task id names its VMM, jail, shim record and console log.

## Docker tasks (ADR 0011)

dockerd reaches the shim as the runtime `vesper` (`docker run --runtime vesper`). A task is a Docker task when its
spec's `root.path` is dockerd's mounted rootfs and it is in a `moby` containerd namespace. For such a task:

- `Create` checks the spec (`bundle.CheckDocker`), builds the container's drive once from `root.path` on the data disk
  with `mke2fs` in a user namespace with the spec's mapping (`backend.PrepareDocker`; reused across starts, swept when
  dockerd's store no longer has the container), and starts the namespace holder (`internal/ctrns`): its pid is the
  task's for its life, and the shim stops the task if it exits.
- `Start` reads `resolv.conf`, `hosts` and `hostname` (`backend.ReadDockerFiles`, `openat2` with no symbolic links,
  64 KiB each), builds the guest spec (`bundle.GuestDockerConfig`) and launches with `vesper-microvm launch --ctrns`.
- `Delete` keeps the drive and releases the namespace (`vesper-microvm ctrns-down`); `manager.Stop` does too.

How each field of dockerd's spec is handled (ADR 0011 §4; tests in `internal/bundle/docker_test.go`):

| Field | Handling |
| --- | --- |
| `process` (args, env, cwd, user, terminal, rlimits), `hostname` | honoured in the guest |
| capabilities, seccomp, AppArmor, SELinux, `noNewPrivileges`, `oomScoreAdj` | ignored (CE-0004) |
| mounts proc, cgroup, devpts, mqueue, tmpfs (`/dev`, `/dev/shm`, `--tmpfs`) | made by the guest's runc |
| mount sysfs | the guest's `/sys` bound read-only (a second sysfs in the guest's network namespace is refused) |
| binds of `resolv.conf`, `hosts`, `hostname` | written by the guest from the config, then bound |
| binds of anonymous volumes | the image's own directory in the drive |
| `--init` (docker-init) | the guest's `/sbin/tini-static` |
| any other bind, devices, a namespace `path`, hooks, a read-only root | refused |
| namespaces | as given, without the user namespace; network is the guest's own |
| `linux.sysctl` | `net.*` set by the guest (`vesper-console files`), the others by the guest's runc |
| memory and CPU limits | honoured (TD-048) |

## Invariants

- A running process cannot start twice or be deleted. Concurrent lifecycle requests use a per-process lock.
- Cached ext4 bytes contain only rootfs content; process configuration never enters the cache key.
- runc launches argv directly. The image does not need `/bin/sh`.
- Non-terminal stdout and stderr remain separate, and stdin EOF reaches the workload.
- A signal exit maps to `128 + signal`; a lost transport returns 137 and logs the failure.
- The terminal starts at the OCI console size, or 24×80 when absent. `ResizePty` changes the existing terminal.
- Init exit destroys remaining guest processes. A failed exec start is aborted before its record can be deleted.
- The PID 1 supervisor reaps orphans. A dead shim never intentionally leaves a VMM behind.
- One task allows at most 16,125 exec sessions. Ports are not reused within that task (WOR-32 follow-up).

## Verification

    make shim-test                                  # vet and Go tests
    make shim-build                                 # static Linux arm64 shim and terminal helper
    make systemvm-up && make systemvm-exec-suite     # streams, exec, terminal resize and signals
    make systemvm-shim-suite                         # existing task lifecycle checks
    make systemvm-bundle-suite                       # layer cache and private drive checks
    make systemvm-restart-suite                      # containerd and shim crash checks

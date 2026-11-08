# shim/

`containerd-shim-vesper-v2`: the containerd runtime v2 shim that runs each container task as a jailed Firecracker
microVM inside the System VM ([ADR 0004](../docs/design-docs/adr/0004-containerd-shim-in-go.md), GH #22). Go module
`github.com/cityhunteur/vesper/shim`, pinned to containerd 2.1.5 (`containerd/api` 1.9.0, `ttrpc` 1.2.7), built
static for linux/arm64 by `scripts/systemvm/build-shim.sh` and shipped to `/usr/bin/containerd-shim-vesper-v2` in
the guest overlay. Runtime name `io.containerd.vesper.v2`.

## Interface

- `cmd/containerd-shim-vesper-v2/main.go`: `shim.Run(ctx, manager.New(manager.RuntimeName))` with the task plugin
  registered by import.
- `internal/manager`: `RuntimeName = "io.containerd.vesper.v2"`, `Version` (ldflags), `New(name) shim.Manager`.
  `Start` spawns the daemon (`-namespace`, `-id`, `-address`, optional `-debug`; socket inherited as fd 3; OOM
  score adjusted) and returns `BootstrapParams{Version: 3, Protocol: "ttrpc", Address}`; a live socket at the
  address is reused. `Stop` (containerd's `delete` after a dead shim) reconciles from `vesper-task.json` (a
  stopped or deleted task keeps its exit status and time, a running one is reported as 137, the VMM pid is
  carried), then runs `vesper-microvm cleanup <id>`, unmounts `<bundle>/rootfs` and removes `rootfs.ext4`. `Info` returns the runtime name and
  version.
- `internal/bundle`: `Validate(bundle, mounts) (*specs.Spec, error)` rejects, before any mount, a relative or
  symlinked bundle, a missing or invalid `config.json`, a spec without `ociVersion` or `process.args`, a
  relative `cwd`, an unexpected `root.path`, incomplete mounts, or no mounts with an empty `rootfs/`; errors are
  `*ValidationError{Bundle, Reason, Hint}`. `Inspect(mounts) Layers{Kind, Lower, Upper, Key}`: `overlay` (lower
  chain, sha256 key), `mounts` (identity key) or `directory` (no key). `Meta` (`task, bundle, image, cache,
  cached, sha256, bytes, built_at, build_ms, strategy, layers, fallback`) with `WriteMeta` (atomic, one line as
  `vesper-rootfs.json`) and `ReadMeta`.
- `internal/backend`: `Backend` interface (`Prepare(ctx, task, bundle, mounts) (image, bundle.Meta, error)`, `Launch(ctx, id, image, cmdB64)
  (pid, error)`, `Stop`, `Cleanup`, `LogPath(id)`, `Alive(id)`); `MicroVM` (`NewMicroVM()`): `Prepare` is block-first: with a layer key it
  builds `/var/lib/vesper/rootfs-cache/<key>.ext4` once (`mke2fs -q -F -t ext4 -d <bundle>/rootfs -L vesper-task
  -E root_owner=0:0`, used KiB + 20 % + 4 MiB, at least 16 MiB; temp file and `.sha256` sidecar renamed into
  place), verifies the digest on every reuse and copies a private `<bundle>/rootfs.ext4`; without a key
  (directory rootfs) it builds directly per task (`fallback: true`, `strategy: direct`); failures are
  `*PrepareError{Step, Err, Hint}`; the `Meta` record is written to the bundle. It shells out to
  `/usr/local/bin/vesper-microvm launch|stop|cleanup|status`;
  `Launch` returns pid 0 when the workload exited before `LAUNCHED` carried a pid. `EncodeCommand(args, env,
  cwd)` builds `cd CWD || exit 126; export K=V; …; exec ARGS` with every word single-quoted, base64-encoded;
  `RootfsImageName = "rootfs.ext4"`.
- `internal/task`: `Service` (ttrpc `TTRPCTaskService`; `NewService(ctx, publisher, shutdown, backend)`;
  `RegisterTTRPC`). One task per shim. `Create`: rejects a second task (`AlreadyExists`), terminal
  (`NotImplemented`), checkpoints (`NotImplemented`), a bundle without `config.json` or `process.args`
  (`bundle.Validate` → `InvalidArgument` with the diagnosis); mounts `Rootfs` onto `<bundle>/rootfs`, prepares the drive (`FailedPrecondition` with the step and hint on failure), unmounts, records stdio paths,
  publishes `/tasks/create`, returns the shim pid. `Start` (exec id → `NotImplemented`; not created →
  `FailedPrecondition`) launches, marks running, starts the watcher, publishes `/tasks/start`. `Kill`: created →
  exits 137 immediately; running → `backend.Stop`; stopped → `NotFound`. `Delete`: running →
  `FailedPrecondition`; otherwise cleanup, image removal, unmount, `/tasks/delete`, shim shutdown. `State`,
  `Wait` (blocks on the exit channel), `Pids` (the VMM pid while running), `Connect`, `Shutdown`; `Pause`,
  `Resume`, `Checkpoint`, `Exec`, `ResizePty`, `CloseIO`, `Update` → `NotImplemented`; `Stats` (Linux build,
  `stats.go`) reads the VMM's cgroup v2 directory from `/proc/<pid>/cgroup` (`memory.current/max/swap.current`,
  `memory.stat`, `cpu.stat`, `pids.current/max`) into `cgroups/v2 Metrics` (type URL
  `io.containerd.cgroups.v2.Metrics`), `NotFound` unless the task is running; the non-Linux stub returns
  `NotImplemented`. `Task` state
  machine: created → running → stopped → deleted (`MarkRunning`, `MarkExited` once, `MarkDeleted`;
  `Exited()` channel). `StateFileName = "vesper-task.json"`: `State{id, phase, pid, image, exit_status,
  exited_at, shim_pid, updated_at}` persisted atomically on create, start, exit, kill and delete;
  `ReadState(bundle)`. `followLog(path, out, alive)` tails the microVM console log (which may not exist yet),
  writes lines between `VESPER-WORKLOAD-START` and `VESPER-WORKLOAD-EXIT <n>` to `out`, returns `n`, or 137
  after three quiet polls with the VMM gone.

## Invariants

- The shim never deletes a running task and never starts a task twice; both are `FailedPrecondition`.
- `MarkExited` is idempotent, so a kill racing the natural exit records one status.
- Every `Create` that fails leaves no mount behind; every `Delete` leaves no jail, cgroup, image or VMM.
- The image must contain `/bin/sh`; terminal, stdin, exec and resize are not supported in this slice (P3.2).
- One cached drive per layer chain; a task never boots a drive whose digest differs from its sidecar.
- The shim runs with the framework's reaper and subreaper disabled; it waits on its own children itself.
- Every task transition is on disk before it is reported; a dead shim never leaves a microVM behind.

## Verification

    cd shim && go vet ./... && go test ./...        # make shim-test
    make shim-build                                  # build/shim/containerd-shim-vesper-v2 (linux/arm64, static)
    make systemvm-up && make systemvm-shim-suite     # GH #22 checklist through ctr (21 steps)
    make systemvm-bundle-suite                       # GH #24 checklist: block-first cache, layered image, dedup (15 steps)
    make systemvm-restart-suite                      # GH #25 checklist: containerd restart, shim crash, in-flight (18 steps)

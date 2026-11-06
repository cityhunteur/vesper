# Host to System VM vsock control channel (WOR-25 / GH #13)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

## Purpose / Big Picture

GitHub #13 (Linear WOR-25, mirror #67) asks for vsock communication primitives between macOS and the System VM:
server and client, handshake with version negotiation, keepalive and reconnect, proven with a handshake, idle
keepalive, reconnect after a drop and a payload above 1 MB. After this plan `make systemvm-up` keeps the VM
running with a host-side bridge socket, and `systemvmctl control …` talks to a guest agent over vsock through that
bridge. Slice 1 builds the guest-side foundation: pinned extra packages (`socat`, `containerd`, Docker) installed at
boot through an overlay (ADR 0002), so later slices can run an agent and services inside the guest.

## Progress

- [x] (2026-11-06 18:05Z) Slice 1: packages lock and resolver, overlay tree with boot-time install, deterministic
  overlay-and-initramfs builder, image lock schema 4 with the overlay section, `make systemvm-overlay` and
  `make systemvm-prepare`, probe extended to require the installed binaries; 5/5 boots at about 2.0 s ready time.
- [x] (2026-11-06 18:25Z) Slice 1: PR #122 merged as `e4fed7c` after a CI fix (verify-image tolerated a missing overlay), CI green, tracker updated.
- [x] (2026-11-06 18:40Z) Slice 2 (merged with slice 3): launcher serves the bridge (`CONNECT <port>` preface,
  mode 0600) and gains `--keep-running` semantics and SIGTERM handling; overlay runs `vesper-packages` (OpenRC
  oneshot) and the supervised `vesper-agent` (socat + shell, protocol 1); `src/control.rs` client with handshake,
  keepalive, reconnect with backoff, streaming echo and a suite; `systemvmctl control …`; `make systemvm-up` /
  `systemvm-down` / `systemvm-control-suite`; ADR 0003. Suite passed against the real guest: handshake 1 ms,
  11 keepalive pings over 20 s, reconnect in 1 attempt after a server-side drop, 2 MiB echoed intact in 11 ms.
- [x] (2026-11-06 18:55Z) Slice 2: PR #123 merged as `cf091e9`, CI green, GH #13 checklist and WOR-25 updated,
  milestone entry written; plan moved to completed.

## Surprises & Discoveries

- Observation: an overlay on a second ISO device is never applied; the Alpine init only searches the boot medium
  and the explicit `apkovl=/dev/vdb:iso9660:/…` form did nothing on 3.22.
  Evidence: `/tmp` experiments on 2026-11-06, console logs without "Loading user settings".
- Observation: an ISO built with macOS `hdiutil makehybrid -iso -joliet` is mounted by the guest as plain
  ISO 9660: dots stripped (`localhostapkovltar.gz`), so the init cannot find `.boot_repository`.
  Evidence: emergency-shell listing of `/dev/vda` captured through the probe.
- Observation: `apk add` of files outside a repository needs `--force-non-repository` on a diskless system.
  Evidence: first overlay boot logged `vesper: package install FAILED`; manual run printed the apk error.
- Observation: installing the 18 packages at boot costs about 0.9 s before the login prompt (ready time 1.05 s
  to 2.0 s).
  Evidence: `run/WOR-25/systemvm/boot-report-5.json`.
- Observation: a client that writes a large `ECHO` payload before reading deadlocks once the socket buffers
  fill, because the agent streams bytes back as they arrive; the client now writes from a second thread.
  Evidence: `write echo payload: Resource temporarily unavailable (os error 35)` on the first 2 MiB attempt.
- Observation: a background launcher that inherits the caller's stdin/stdout keeps `make systemvm-up` (and any
  agent harness) blocked; `up.sh` now detaches with `nohup … < /dev/null`.
  Evidence: the first `make systemvm-up` never returned although the VM was running.
- Observation: the first CI run failed because `make systemvm-fetch` verified the image before the overlay step
  and required the derived initramfs; the local tree already had the file, so the sequence was never exercised.
  Evidence: PR #122 run log; fixed by making the check opt-in and reproducing with the file deleted.

## Decision Log

- Decision: deliver guest customisation through a cpio archive appended to the pinned initramfs (ADR 0002).
  Rationale: no new host tooling, the pinned ISO and initramfs stay byte-identical, deterministic output.
  Date/Author: 2026-11-06, agent.
- Decision: pin the package closure with a repository resolver (`resolve-apks.py`) that treats everything on the
  boot ISO as present, and verify every package digest in Rust before building the overlay.
  Rationale: same "parse at the boundary, pin every byte" rule as the image lock.
  Date/Author: 2026-11-06, agent.
- Decision: `socat` is the guest-side vsock primitive for slice 2.
  Rationale: packaged by Alpine, supports `VSOCK-LISTEN`, no code to write in the guest.
  Date/Author: 2026-11-06, agent.
- Decision: the launcher serves the bridge socket; the agent is a shell line protocol; the bridge is owner-only
  (ADR 0003).
  Rationale: only the VM-owning process can reach guest vsock; the proxy convention already existed; a typed agent
  is deferred (TD-026); the host-visible socket is recorded as TD-027 for a human decision before Phase 1 exit.
  Date/Author: 2026-11-06, agent.
- Decision: package installation is an OpenRC oneshot (`vesper-packages`) that other services `need`, not a
  `local.d` script.
  Rationale: `after local` did not order `vesper-agent` after the install and cost a 60 s start-pre timeout;
  `need` is enforced.
  Date/Author: 2026-11-06, agent.

## Outcomes & Retrospective

Slices 1 and 2 delivered the guest foundation and the control channel; every GH #13 checklist item is proven
against the real guest (`run/WOR-25/systemvm/control-report.json`). Landed as PRs #122 and #123 (merges `e4fed7c`,
`cf091e9`); WOR-25 is Done. Gaps: shell agent (TD-026), host-visible bridge
(TD-027), packages reinstalled per boot (TD-025).

## Context and Orientation

`src/systemvm.rs`, `src/systemvm/image_lock.rs` and `src/systemvm/packages.rs` hold the library; `systemvmctl`
the CLI; `launcher/` the Virtualization.framework helper; `systemvm/overlay/` the guest overlay tree;
`scripts/systemvm/` the wrappers. Terms: *overlay* is the Alpine `apkovl` tarball; *derived initramfs* is the
pinned initramfs plus the overlay archive; *bridge* is the host Unix socket `vesper-proxy` dials for
`vsock://<cid>:<port>` upstreams (`docs/process/systemvm-bringup.md`, "Proxy bridge convention").
Guides: design-v0 "Phase 1", ADR 0001 and 0002, `src/README.md`, `launcher/README.md`.

## Plan of Work

Slice 1 (done): see Progress. Slice 2 (done): the launcher reads `vsock_bridge_path` from the launch config,
listens, reads `CONNECT <port>\n`, connects with `VZVirtioSocketDevice`, relays; the overlay runs
`etc/init.d/vesper-agent` (`socat VSOCK-LISTEN:1024,fork EXEC:/usr/local/bin/vesper-agent`, protocol 1);
`src/control.rs` client and suite; `make systemvm-up` runs the launcher with `--keep-running`.

## Concrete Steps

    cd <repo root>
    make systemvm-prepare        # fetch, overlay, launch config, launcher
    make systemvm-boot-smoke     # each boot installs the packages and the probe prints VESPER-PROBE-OK

## Validation and Acceptance

Slice 1: `cargo test systemvm` (13 tests), `make systemvm-overlay` twice (second run reuses the cache),
one probed boot whose console shows `vesper: packages ready`, `VESPER-EXTRAS-OK` and `VESPER-PROBE-OK`,
`make systemvm-boot-smoke` 5/5 under the threshold, docs and structure sensors, the `System VM Image` workflow.
Slice 2: `cargo test control` (3 tests against a fake bridge), `make systemvm-up` (3 s), `make
systemvm-control-suite` with `passed: true`, `make systemvm-down` (3 s, bridge socket removed).
Issue exit: the four GH #13 checklist items proven by `run/WOR-25/systemvm/control-report.json`.

## Idempotence and Recovery

All targets are safe to repeat; delete `build/systemvm/initramfs-vesper` to force an overlay rebuild and
`build/systemvm/cache/apks` to force package downloads.

## Artifacts and Notes

    $ make systemvm-boot-smoke
    boot smoke passed: successful=5 failed=0 max_boot_ms=2033 (threshold 5000)

    $ make systemvm-up && make systemvm-control-suite && make systemvm-down
    systemvm-up: running (pid 4247); bridge run/systemvm/vsock/3.sock; ...
    ok   handshake: protocol 1 host vesper in 1 ms (1 ms)
    ok   keepalive: 11 pings over 20 s (20076 ms)
    ok   reconnect: reconnected after a server-side drop in 1 attempt(s) (4 ms)
    ok   large_payload: 2097152 bytes echoed intact in 11 ms (46 ms)
    control suite passed (report: run/systemvm/control-report.json)
    systemvm-down: guest acknowledged shutdown
    systemvm-down: stopped

    $ grep -nE "vesper: |VESPER-[A-Z]+-OK" run/systemvm/console.log
    116: * Starting local ...vesper: installing 18 pinned packages
    117:vesper: packages ready
    142:VESPER-NAT-OK
    147:VESPER-EXTRAS-OK
    150:VESPER-PROBE-OK

## Interfaces and Dependencies

`vesper::systemvm::{PackagesLock, Package, load_packages_lock, validate_packages_lock, packages_list,
verify_packages, OverlaySettings, ControlSettings}`; `ImageManifest.overlay_initrd_{path,sha256}`;
`LaunchConfig.{vsock_bridge_path, agent_port}`; `vesper::control::{ControlEndpoint, Session, connect,
connect_with_retry, run_suite, write_report}`; `systemvmctl package-list | verify-packages | control …`. Guest packages (Alpine 3.22, aarch64): `socat`, `containerd`, `containerd-openrc`,
`docker-engine`, `docker-cli`, `docker-openrc` and their closure (18 files). No new crate dependencies.

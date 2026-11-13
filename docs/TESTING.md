---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-12
---

# Testing

This page explains what kinds of tests exist, where they live, how to run exactly the ones you need, and what evidence
a change needs before it counts as done. While iterating, run the narrowest relevant check. Before declaring done, run
the broader suite. `cargo test` was run successfully on 2026-11-05.

## Test inventory

| Category | Implemented? | Location | Framework | Command | Needs |
|---|---|---|---|---|---|
| Unit | Yes | `#[cfg(test)]` modules in `src/*.rs`; API handler tests in `src/api/tests.rs` | Rust test harness, `tokio::test` for proxy, `tempfile` | `cargo test --lib` | nothing |
| Component | Yes (router level) | `src/api/tests.rs`, `tests/api_contract.rs` drive the axum router with `tower::ServiceExt::oneshot` | Rust, `tower` | `cargo test --test api_contract` | nothing |
| Integration | Partial | `tests/contract_golden.rs` loads the vendored swagger and golden JSON from disk | Rust | `cargo test --test contract_golden` | repo checkout |
| Process (vesperd socket) | Yes | `tests/vesperd_socket.rs` starts the `vesperd` binary: owner-only default directory and socket, a second `vesperd` refused, a live or non-socket path never removed, a stale socket replaced, a shared directory refused (TD-050) | Rust | `cargo test --test vesperd_socket` | nothing |
| Process (vesperd System VM backend) | Yes | `tests/vesperd_systemvm.rs` starts `vesperd --backend systemvm` against a fake bridge and agent: Firecracker's bytes relayed unchanged, SIGTERM closes the lease and exits 0 (also with stdout closed), a `READY` version other than the pinned swagger's refused with no socket, the lease closing makes `vesperd` exit 1 and remove its socket, `ERR busy` retried, a refused `FCAPI` closes the client with no bytes, a second `vesperd` on the socket launches nothing, bad arguments refused (ADR 0007 slice 4). `src/proxy.rs` and `src/proxy/relay.rs` unit tests cover the preface, a refused `FCAPI`, an overlong or missing greeting, the per-read bound with a 4 MiB transfer and a half-close, the idle timeout, the per-request ceiling, the connection cap, an upstream error passed through byte for byte, a refused live socket and eight racing `claim_socket` calls | Rust | `cargo test --test vesperd_systemvm`; `cargo test --lib proxy` | nothing |
| Contract | Yes | `contractgen --mode check`, `tests/api_contract.rs`, generated matrix validation | Rust binaries | `make contract-test`, `make surface-matrix-check`, `make behavior-matrix-check` | nothing |
| End-to-end | **Not implemented** | design-v0 describes Docker-replacement e2e; no code | n/a | n/a | a System VM launcher (TD-005) |
| Performance | **Not implemented** | design-v0 describes regression and perf gates; no code | n/a | n/a | n/a |
| Security / static analysis | Partial | `scripts/ci/validate-entitlements.sh`, `scripts/harness/check_structure.py` | bash, Python | `make validate-entitlements` (CI: `rust-tests.yml`), `python3 scripts/harness/check_structure.py --root .` | Python 3 |
| Runtime smoke | Yes | `make systemvm-boot-smoke` boots the pinned Alpine VM through `vesper-vmlauncher` N times, logs in, proves NAT (DHCP + gateway ping), powers off, and fails any boot over the lock's 5 s threshold; report in `run/systemvm/boot-report.json` | Rust + Swift | `make systemvm-fetch systemvm-launch-config vmlauncher-build systemvm-boot-smoke` | macOS on Apple silicon, Xcode CLT; not runnable in CI |
| Control channel | Yes | `systemvmctl control suite` runs the GH #13 checklist (handshake, keepalive, reconnect after a server-side drop, 2 MiB echo) against the running guest through the launcher's bridge; `src/control.rs` unit tests run the same client against a fake bridge | Rust | `make systemvm-up && make systemvm-control-suite && make systemvm-down`; `cargo test control` | macOS for the real guest; nothing for the unit tests |
| Runtime services | Yes | `systemvmctl control services-suite` checks that `/dev/net/tun` exists after boot (GH #173), then runs the GH #14 checklist per daemon (healthy at boot, SIGKILL, supervised recovery, startup and recovery lines in the guest logs) through the bridge | Rust | `make systemvm-up && make systemvm-services-suite && make systemvm-down`; `cargo test control` | macOS for the real guest |
| Docker ingress | Yes | `make docker-smoke` runs the GH #15 checklist with the macOS Docker CLI through `vesper-proxy`: owner-only socket, `docker version`, `docker info`, `docker import` of the offline trivial image, `docker run`, host socket enumeration, refused raw guest port | bash, Docker CLI | `make systemvm-up && make docker-up && make systemvm-trivial-image && make docker-smoke` | macOS, Docker CLI on the host |
| Firecracker lifecycle | Yes | `make systemvm-microvm-suite` launches a stock Firecracker v1.16.0 microVM under the jailer inside the System VM (nested KVM), checks uid/jail/cgroup/seccomp isolation, clean stop, cleanup after SIGKILL and guest power-off; report in `run/systemvm/microvm-report.json` | Rust + guest shell | `make systemvm-up && make systemvm-microvm-suite`; `cargo test control` | macOS on an M3 or later |
| OCI to ext4 rootfs | Yes | `make systemvm-rootfs-suite`: image import, two builds with one digest, cache hit, embedded provenance, invalid-image error, and a jailed microVM booting the rootfs; report in `run/systemvm/rootfs-report.json` | Rust + guest shell | `make systemvm-up && make systemvm-rootfs-suite` | macOS on an M3 or later |
| MicroVM per container | Yes | `make systemvm-workload-suite`: three concurrent workloads in three jailed microVMs, isolation between them, stop of one leaving the others, exit-code and output mapping, full removal; report in `run/systemvm/workload-report.json` | Rust + guest shell | `make systemvm-up && make systemvm-workload-suite` | macOS on an M3 or later |
| `vesperd` on Backend A (System VM side) | Yes | `cargo test control` (the `LAUNCHAPI`/`FCAPI` client and the whole checklist against fake agents) and `make systemvm-launchapi-suite` (20 steps, ADR 0007 slice 3: launch with the pinned Firecracker version, a duplicate id refused, `GET /` before boot, boot through the API socket, `GET /` `Running`, the guest boot marker, uid 20000 and cgroup `/vesperd/<id>` isolation, bad ids refused by `FCAPI` and `FCPARITY`, a shim microVM with the same id kept apart, teardown after the lease closes, after Firecracker is killed and after the lease shell is killed (periodic reaper), a reused id, the reaper ignoring a reused holder pid and sparing a new unrecorded jail, a default container unable to open `AF_VSOCK`, nothing left); report in `run/systemvm/launchapi-report.json` | Rust + guest shell | `make systemvm-up && make systemvm-launchapi-suite` | macOS on an M3 or later; the unit tests run anywhere |
| `vesperd` on Backend A (macOS side) | Yes | `make systemvm-vesperd-suite` (19 steps, ADR 0007 slice 4) starts `target/debug/vesperd --backend systemvm` and drives it only through its socket: socket mode 0600 and the pinned version, a second `vesperd` on the socket refused without a launch, `GET /` before boot, boot, `GET /` `Running`, the guest boot marker, a Firecracker 400 identical byte for byte through `vesperd` and over `FCAPI`, pause, `PUT /snapshot/create` timed against the per-request ceiling, resume, Firecracker's own 503 to an 11th connection, the 120 s idle timeout (while a direct `FCAPI` connection stays open), the same id refused after the 15 s wait, SIGTERM, SIGKILL and a killed Firecracker each leaving no microVM, `--tap tap0 --tap tap1` (both interfaces 204, an unlisted tap Firecracker's own 400, MMDS config and boot 204, taps owned by uid 20000 in the microVM's own network namespace, the namespace removed after SIGTERM; CE-0010), nothing left; about 2.5 min; report in `run/systemvm/vesperd-report.json` | Rust | `make systemvm-up && make systemvm-vesperd-suite` | macOS on an M3 or later |
| `vesperd` egress (ADR 0008) | Yes | `make systemvm-egress-suite` (10 steps) starts `vesperd --network nat --tap tap0:172.16.0.1/30` microVMs whose guest init probes from inside (`vesper_net=`, `vesper_probe=`): public TCP and DNS open (judged only when the System VM reaches the internet); the System VM's addresses (against TCP and UDP listeners), the vmnet gateway other than DNS, docker0, the veth pool and another lease's veths, RFC 1918, CGNAT, metadata, multicast, reserved, loopback and a spoofed source closed, with the namespace layer's drop counter rising; two concurrent leases on distinct veths; `isolation` `ipv6_off` and `egress_ok`; dockerd killed and respawned while a microVM runs, then its namespace filter opened: every deny still closed and public egress open, the root layer counting the drops; a microVM with no namespace filter, masquerade or reverse-path check: every probe closed, the raw rule counting the drops; the suite fails without internet; teardown, a `none` lease and the reaper after `kill-holder` leaving no veth; about 2 min; report in `run/systemvm/egress-report.json` | Rust + guest shell | `make systemvm-up && make systemvm-egress-suite` | macOS on an M3 or later, internet for the allowed probes |
| Behaviour-matrix replay | Yes | `make behavior-replay-check` (`tests/api_replay.rs`, in `cargo test`): every one of the 173 cases in `docs/generated/firecracker-api-behavior-matrix.json` has a script in `docs/specs/firecracker-api-behavior-replay.yaml` and replays on a fresh model router with its own temporary root, matching status, `fault_message`, `code` and the body assertions. `make systemvm-replay-suite` (ADR 0007 slice 6; needs `make systemvm-up-snapshot`) replays the same scripts through `vesperd --backend systemvm` on stock Firecracker, one microVM per case, about 400 ms each and 75 s in all; report in `run/systemvm/replay-report.json`, and `make systemvm-replay-publish` rewrites `docs/generated/backend-a-behavior-replay.json`. The suite fails only when a case cannot run; differences are findings (D-026) | Rust | `cargo test --test api_replay`; `make systemvm-up-snapshot && make systemvm-replay-suite` | the model replay runs anywhere; the Backend A replay needs macOS on an M3 or later |
| `vesperd` staging and snapshots | Yes | `cargo test --test vesperd_systemvm` and `cargo test --lib control::stage` (the forms, `DATA status`, `--stage` tokens and `vesperd stage|store` against a fake agent), and `make systemvm-snapshot-suite` (23 steps, ADR 0007 slice 5; needs the share `run/vesperd-share`, `make systemvm-up-snapshot`): a drive staged from the share at start and one while Firecracker runs, both opened by `PUT /drives`; a symlinked share or jail `SRC`, a symlinked `DEST`, `SRC` outside the share, store and jail, a root-only jail file (the uid 20000 read) and a copy past the free-space reserve each refused with nothing written; pause, create, copy into the store, stop; a new `vesperd` stages the snapshot by link (root-owned, 0444, two links), loads and resumes it, and the guest prints the same in-memory token with later ticks; a create onto the linked files fails (CE-0008); a bad `--stage` launches nothing; a timed create of a 640 MiB guest with 512 MiB dirtied under the default ceiling; `vesperd store rm`; nothing left; about 30 s; report in `run/systemvm/snapshot-report.json`. `make data-disk-suite` step `vesperd_store_survives_jails_swept` shows that a reboot keeps the store and removes the jails | Rust + guest shell | `make systemvm-up-snapshot && make systemvm-snapshot-suite` | macOS on an M3 or later; the unit and process tests run anywhere |
| Parity baseline | Yes | `make systemvm-parity-baseline`: 19 Firecracker API conformance checks against the stock release binary in a jailed microVM, two performance runs compared within 20 %, merged with the lock, parity metadata and compat registry into `run/systemvm/parity-baseline-report.json`; `make parity-baseline-check` validates the committed capture | Rust + guest shell + Python | `make systemvm-parity-baseline PERF_CYCLES=20 PARITY_FLAGS=--publish` | macOS on an M3 or later; the check runs anywhere |
| Guest exec | Yes | `make systemvm-exec-suite`: stdin and separated output, no-shell exec, terminal resize, interactive shell, exit codes 0/7/137/143, orphan cleanup and structured runtime errors; `run/systemvm/exec-report.json` | Go, Rust and guest shell | `make systemvm-up && make systemvm-exec-suite` | macOS on an M3 or later; Go and protocol unit tests run anywhere |
| Shim Task RPCs | Yes | `make shim-test` (Go unit tests: state machine, guest config, stream protocol and exec lifecycle) and `make systemvm-shim-suite` (22 steps through `ctr` with the Vesper shim: e2e exit code and output, running state, invalid sequences, kill, delete, three concurrent tasks, no leftovers); report in `run/systemvm/shim-report.json` | Go + Rust + guest shell | `make systemvm-up && make systemvm-shim-suite` | macOS on an M3 or later; the Go tests run anywhere |
| Block-first rootfs | Yes | Go unit tests (7 malformed bundles, layer keys, metadata, cache validity) and `make systemvm-bundle-suite` (15 steps: two-layer image boots, cache hit with same key and digest, one drive per chain, base snapshot shared); report in `run/systemvm/bundle-report.json` | Go + Rust + guest shell | `make systemvm-up && make systemvm-bundle-suite` | macOS on an M3 or later; the Go tests run anywhere |
| Shim restart | Yes | Go unit test (task record round trip) and `make systemvm-restart-suite` (18 steps: task survives a SIGKILLed containerd with the same VMM pid, dead shim leaves no microVM, in-flight `ctr run` fails cleanly, durable record); report in `run/systemvm/restart-report.json` | Go + Rust + guest shell | `make systemvm-up && make systemvm-restart-suite` | macOS on an M3 or later |
| Docker API conformance | Yes | `make docker-conformance`: 25 checks from macOS through `vesper-proxy` (protocol, context, images, run/exec/stop/kill/rm, pull, error passthrough, attached streams across a pause, an early-closed stdin and 7 s of output (GH #167), ingress boundary); report in `run/systemvm/docker-conformance-report.json` | bash + docker CLI | `make systemvm-up && make docker-up && make docker-conformance` | macOS on an M3 or later with outbound network |
| Published ports | Yes | `cargo test ports` (bindings, planning, events, HTTP framing, conflict status) and `make ports-suite` (12 steps, incl. a half-closed client getting a late reply (GH #167): localhost reachability, multiple mappings, dockerd and host conflicts, teardown on stop and rm, launcher survives a proxy restart, republish after restart); report in `run/systemvm/ports-report.json` | Rust + bash | `make systemvm-up && make docker-up && make ports-suite` | macOS on an M3 or later with outbound network |
| Bind mounts | Yes | `make binds-suite` (20 steps: shares at host paths, read, ownership mapping, chmod/chown, read-only at share level and `:ro`, promptness, inotify for guest writes, host edit and host create propagated within 1 s, host mtime preserved after one edit and after 200 rapid edits of one file, no echo storm, read-only share silent, host delete and rename seen on the parent directory, share-root delete not propagated, 500-file burst delivered, `:nowatch` share silent, path policy); report in `run/systemvm/binds-report.json`. `make binds-suite` never runs in CI (developer hardware only, TD-005). CI runs two self-tests without a VM: `make share-notify-selftest` (guest sink path policy, 20 checks, Linux) and `make share-watch-selftest` (launcher watcher: dry-run decisions, delivery to a socket sink, reconnect, queue limit, exit under a stalled sink; 20 checks, macOS, needs python3) | bash + docker CLI | `make systemvm-up-shares && make docker-up && make binds-suite` | macOS on an M3 or later with outbound network |
| System VM data disk | Yes | `cargo test data_disk` (device naming after the boot media and rootfs, the kernel command line, relative path and second disk rejected, configs without the field) and `make data-disk-suite` (9 steps, GH #164: disk in use, state directories on the disk, not reformatted on the second boot, image, stopped container and named volume survive a reboot, ready within 5000 ms with the disk mounted, a disk with another label never formatted, Docker works with the disk refused); restarts the System VM itself four times and leaves it running on the default disk; report in `run/systemvm/data-disk-report.json`. Developer hardware only, never in CI (TD-005). On 2026-11-11 (M3 Pro): 9/9, ready in 2.7 s | Rust + bash + Docker CLI | `make systemvm-prepare && make systemvm-up && make docker-up && make data-disk-suite` | macOS on an M3 or later with outbound network |
| DNS and network defaults | Yes | `make dns-suite` (12 steps: default-bridge resolver and external DNS/egress, compose embedded DNS, service discovery, aliases, external forwarding, NXDOMAIN, determinism across runs, cleanup); report in `run/systemvm/dns-report.json` | bash + docker compose | `make systemvm-up && make docker-up && make dns-suite` | macOS on an M3 or later with outbound network |
| Developer workflows | Yes | `make workflow-suite` (8 steps: compose up, build from a host context, logs -f streaming, interactive exec -it via expect, HUP/TERM/KILL signals, amd64 probe recorded as a gap); report with baseline metadata in `run/systemvm/workflow-report.json` | bash + docker compose + expect | `make systemvm-up && make docker-up && make workflow-suite` | macOS on an M3 or later with outbound network |
| Cross-architecture | Yes | `make crossarch-suite` (9 steps: binfmt registration, execution modes, `--platform linux/amd64` run, image and container architecture metadata, unsupported platform error, emulated-vs-native timing, native image untouched); report in `run/systemvm/crossarch-report.json` | bash + docker CLI | `make systemvm-up && make docker-up && make crossarch-suite` | macOS on an M3 or later with outbound network |
| ctr conformance | Yes | `make ctr-conformance` (shim 22 + bundle 15 + restart 18 + exec 19 steps, as recorded in `docs/specs/ctr-conformance-baseline.json` through `ctr`, incl. `task metrics`, assembled with the baseline revisions and the GH #26 checklist); `make ctr-conformance-check` validates the committed capture | Rust + guest shell + Python | `make systemvm-up && make ctr-conformance` | macOS on an M3 or later; the check runs anywhere |
| Reliability | Yes | `make systemvm-reliability` runs N probed boots (default 50) under the threshold, then the control, services and Docker suites on a kept-up VM, checks for leaked processes, sockets and pid files and for boot-time drift, and writes `run/systemvm/reliability-report.json` with actionable `failures[]` | bash, Python | `make systemvm-reliability` (`CYCLES=200 …`); `python3 -I scripts/systemvm/reliability_report.py --self-test` (CI) | macOS on Apple silicon for the run |
| Image reproducibility | Yes | `systemvmctl verify-image` checks the unpacked kernel, initramfs and ISO digests and the `expected_image_id` against `docs/specs/systemvm-image.lock.yaml`; `verify-packages` checks every guest package against `systemvm-packages.lock.yaml`; the derived initramfs must start with the pinned one | Rust, bash, Python | `make systemvm-fetch`, `make systemvm-overlay` (CI: `systemvm-image.yml`, Linux) | network on first run (ISO and packages cached afterwards); `bsdtar` |
| Documentation | Yes | `harness/tests/`, `harness/check_docs.py`, `scripts/harness/check_docs.py` | unittest | `python3 harness/check_docs.py --strict`, `python3 -m unittest discover -s harness/tests`, `python3 scripts/harness/check_docs.py --root .` | Python 3.11+ |

Suite sizes on 2026-11-05: `tests/api_contract.rs` 115 tests, `tests/contract_golden.rs` 1, `src/api/tests.rs` 67,
plus smaller unit modules in `api_matrix`, `api_behavior_matrix`, `proxy`, `baseline`, `compat`, `spec`, `platform`
and `systemvm`.

## Running targeted tests

```sh
cargo test --lib api::                         # every test under src/api (module path filter)
cargo test --test api_contract                 # one integration suite
cargo test --test api_contract guards          # tests whose name contains a substring (the state-guard suites)
cargo test proxy::                             # proxy unit tests (tokio)
cargo test -- --nocapture                      # show stdout from tests
```

## Running the full suite

```sh
cargo test                     # what baseline-governance.yml runs
make contract-check            # what contract-tests.yml runs first
make contract-test             # contract-check + the two integration suites
```

CI workflows and what each runs. All run on Linux except the `launcher-build` job of `systemvm-image.yml`, which
runs on `macos-15`; all are reproducible locally:

- `rust-tests.yml` (any `src/**`, `tests/**`, `Cargo.*`, `Makefile`, `third_party/**`, `docs/generated/**`,
  `docs/specs/*.yaml`, `packaging/**`, `scripts/ci/**` change): `make build`, `cargo test`,
  `make surface-matrix-check`, `make behavior-matrix-check`, `make validate-entitlements`.
- `contract-tests.yml` (swagger, `src/spec.rs`, `src/api.rs`, `contractgen`, golden file): golden check, `make contract-test`.
- `systemvm-image.yml` (image and packages locks, `systemvm/**`, `scripts/systemvm/**`, `launcher/**`,
  `scripts/macos/build-vmlauncher.sh`, `scripts/macos/share-watch-selftest.sh`, the launcher entitlements,
  `src/systemvm.rs`, `src/bin/systemvmctl.rs`, `Makefile`): job `reproducible-image` on Linux runs the systemvm unit
  tests, `make share-notify-selftest`, the pinned image fetch and the deterministic overlay build; job
  `launcher-build` on `macos-15` builds and signs the launcher and runs `make share-watch-selftest`. No job boots
  a VM, so `make binds-suite`, `make data-disk-suite` and the other System VM suites run only on developer hardware
  (TD-005).
- `shim-tests.yml` (`shim/**`, `build-shim.sh`): `go vet`, `go test`, cross-build, artifact upload.
- `ctr-conformance.yml` (the committed ctr capture, lock, assembler): self-test, `--validate`, artifact upload.
- `parity-baseline.yml` (the committed capture, lock, registry, archives, assembler): assembler self-test, `--validate` of `docs/specs/backend-a-parity-baseline.json`, artifact upload.
- `baseline-governance.yml` (lock file, `src/baseline.rs`, `baselinectl`): `cargo test`, metadata generate and validate.
- `compat-exceptions.yml` (registry, `src/compat.rs`, `compatexctl`): `cargo test`, `compatexctl`; a `workflow_dispatch`
  input enables the zero-open gate. There is no schedule, so a calendar expiry is caught only on the next run.
- `harness-docs.yml`, `harness-structure.yml`, `harness-quality-score.yml`: the Python sensors on docs and script changes.
- `harness-garbage-collection.yml`: weekly janitor scan with `--fail-on never` (non-blocking).
- `docs.yml` (every PR and push): `harness/check_docs.py --strict` and its unit tests.

## Fixtures and environments

- `third_party/firecracker/firecracker.yaml`: the pinned OpenAPI every router test loads.
- `testdata/contracts/firecracker-routes.golden.json`: golden route list, regenerated only by `contractgen`.
- `docs/specs/*.yaml` and `docs/generated/*.json`: lock, registries and matrices read by `baselinectl` tests.
- Handler tests that need files (kernel, drive, log and metrics paths) create them with the `tempfile` crate.
- Each test builds its own `AppState`, so suites run in parallel under the default Cargo runner; `cargo test` passed
  with parallel execution on 2026-11-05.

## Reproducing failures

- Re-run the exact CI command from the workflow file listed above; all are plain `make` or `cargo` invocations.
- `RUST_BACKTRACE=1 cargo test <name> -- --nocapture` for a panic in one test.
- There is no randomness to seed and no known flaky-test list (**Unknown** whether any test is flaky; none was
  observed).
- Runtime-sensitive work (proxy, System VM) must also leave artifacts under `run/<ISSUE-ID>/` and
  `.symphony/runs/<ISSUE-ID>/current/`; see [RELIABILITY.md](RELIABILITY.md).

## Definition of done

A change is complete when:

1. A test that fails without the change passes with it, or the issue or PR explains why no test is possible.
2. The targeted tests and every suite CI will run for the touched area pass locally, and CI is green for the pushed SHA.
3. For API or parity changes, `make contract-test`, `make surface-matrix-check` and `make behavior-matrix-check` pass;
   for governance changes, `baselinectl --mode generate` then `--mode validate` and `compatexctl` pass.
4. Behaviour you cannot see in tests is shown with evidence: a command transcript, proxy log, boot report or
   verification bundle.
5. `python3 harness/check_docs.py` and `python3 scripts/harness/check_docs.py --root .` pass, and the owning docs are
   updated.
6. The linked GitHub issue records the commit SHA, local verification, CI result and remaining gaps
   ([CLAUDE.md](../CLAUDE.md), [harness-engineering.md](process/harness-engineering.md)).

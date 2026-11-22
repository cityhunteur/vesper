# Remaining work programme: every open item except the Linux reference run

The owner asked on 2026-11-22 to "proceed with all outstanding items except the Linux reference run", to have the
open design decisions reviewed with EGM and a council, and to follow the council's recommendations. A first draft
of this plan went to four EGM reviewers and a four-member council, and a separate judge checked their 20
recommendations against the code (`run/egm/programme-2026-11-22-review.md`). The owner then approved the ask-first
items by name (D-066). This version follows the result (D-065).

## Purpose / Big Picture

After this programme:

- `docker run --runtime vesper` from macOS runs the container in its own Firecracker microVM, and the Phase 4
  suites pass with that runtime. runc stays dockerd's default for now (the D-015 fallback, approved as an interim
  step by the owner, D-066); GH #53 stays open until microVMs are the default.
- Every tech-debt row is done, or is a recorded deviation or compat exception, or waits on something only a person
  can give: money (the Linux run), hardware (a self-hosted runner), an Apple signing identity, or an independent
  security reviewer.
- The tracker matches the code.

Backend B (GH #56) is deferred (D-067): its prerequisites, the parity reference (CE-0003), the differential
harness (GH #87) and a design of the shim boundary (D-017), are not met.

## Progress

- [x] P0 EGM review, council, judge and owner sign-off (2026-11-22; D-065, D-066, D-067)
- [x] A Foundation: A1 toolchain pin (#234), A2 notices check in CI (#234), A3 file splits (#235), A4 formatting and lint (#237, #238), A5 checks (#239, #240, #241)
- [ ] B Docker containers in microVMs: B1 spike done (`run/B1/findings.md`), B2 ADR 0011 accepted (D-068); B3 slices 1 to 3 merged (#242, #244, #245; D-071, D-073), slices 4 to 6 open
- [x] C API and proxy fixes (#246, 2026-11-22; D-072): TD-019, TD-024, TD-017 and TD-021 done, TD-007 narrowed
- [ ] D Runtime fixes and smaller decisions: TD-060 done (#247); the rest open
- [ ] E Sandbox and signing (ADR 0012)
- [ ] F Evidence: nightly run, parser fuzzing, measurements
- [ ] G Deviations and compat exceptions recorded
- [ ] H Tracker truth
- [ ] Outcomes & Retrospective written; plan moved to completed

## Surprises & Discoveries

- P0: the first draft said DD-1 needed no new package and that `docker run --runtime vesper` would work after
  registering the runtime. Both were false: the package lock has no `iproute2`, and the authorisation plugin
  refuses every runtime but `vesper-runc` and `runc` (`shim/internal/authz/policy.go:179-180`).
- P0: a shim microVM with shares runs Firecracker in a private namespace that holds only `lo` and one tap (ADR 0009);
  a microVM container must also reach dockerd's veth, so the network design must keep the share server out of the
  container's namespace (council ARC-1, SEC-2).
- P0: each shim microVM copies its whole root drive into the tmpfs jail and books it (`vesper-microvm:750`), so a
  2 GiB System VM holds very few containers of ordinary image size (judge, REL-1).
- C: the plan asked for an idle timeout on the `vesper-proxy` relay (TD-021). Docker's attach, logs and events
  streams stay quiet for hours while healthy, so the relay got a connection cap and a preface bound instead (D-072).
- C: the first FIFO test of TD-024 passed with the bug in place: the request ran on the test's own thread, and a
  blocked runtime worker also stops tokio's timers. The test now spawns the request on the worker and waits on a
  std channel; it fails in 5 s with the bug.
- C: the per-request log line exists on the model backend only; the System VM backend relays bytes and logs per
  connection (review F1).
- P0: TD-004 is out of date: the launcher already uses Virtualization.framework's virtio-vsock device
  (`launcher/Sources/vesper-vmlauncher/main.swift:238`).

## Decision Log

- 2026-11-22: A2 found that the notices check passed only on macOS: `cargo tree` and `go list` resolved for the host, and CI's `CARGO_TERM_COLOR=always` coloured cargo's repeat marker. The script now lists the shipped targets (aarch64-apple-darwin for Rust, linux/arm64 for the shim) with colour off; that added three Go modules the old file missed.
- 2026-11-22 (D-068): ADR 0011 accepted after the B1 spike, EGM, a council, a Codex pre-review and a judge. The spike's main surprise: the shim reports its own pid from `Create`, so dockerd would have wired every veth into the System VM's root namespace.

- 2026-11-22 (D-065): this programme, as recommended by EGM, the council and the judge. Order: A1 and A2 first
  (they guard everything after), then B (the largest unknown and the highest value), with the rest of A, C and D as
  work beside B.
- 2026-11-22 (D-066, owner): opt-in microVM runtime as an interim step; `tracing-subscriber`, `proptest` (dev only)
  and the Alpine `iproute2-tc` package approved; ADR 0011 may amend ADR 0009 and ADR 0010 and change the runtime
  allow-list; sandbox and ad-hoc signing approved; UDP published ports on loopback approved; compat exceptions for
  UFFD snapshot load and for microVMs ending with their `vesperd` approved.
- 2026-11-22 (D-067): Backend B deferred.
- 2026-11-22 (D-072): `vesper-proxy` has a 256-connection cap and a 5 s preface bound, and no idle timeout.
- 2026-11-22 (D-073): ADR 0011 slice 3, user-defined networks: DNS over its own DNATed link; id-derived guest MACs.
- The council's acceptance is never the owner's consent: every further ask-first item that appears during the
  work is asked by name.

## Outcomes & Retrospective

Not yet written.

## Context and Orientation

- Tracker rows: `docs/exec-plans/tech-debt-tracker.md`. Decisions: `docs/design-docs/decision-log.md`. Compat
  exceptions: `docs/specs/compat-exceptions.yaml` (open on 2026-11-22: CE-0001, CE-0003, CE-0004, CE-0007, CE-0008,
  CE-0009).
- dockerd's config is written by `systemvm/overlay/usr/local/bin/vesper-containers:121` (`default-runtime`
  `vesper-runc`, `userns-remap` default, the `vesper-authz` plugin). The plugin is `shim/internal/authz/`.
- The shim (Go, ADR 0004) is `shim/`; it launches microVMs with `vesper-microvm` (`systemvm/overlay/usr/local/bin/`).
  Shares reach a microVM over NFS from `unfs3` in a private namespace (ADR 0009).
- The System VM suites run only on an M3 or later Mac (TD-005); CI runs `cargo test`, the shim's `go test`, the
  contract and governance checks.

## Plan of Work

### A Foundation (CI; one PR each)

| Step | Work | Done when | Runs on |
| --- | --- | --- | --- |
| A1 TD-009 | `rust-toolchain.toml` pinning the current stable with `rustfmt` and `clippy`; the six `dtolnay/rust-toolchain` steps take the same version; a pin-bump cadence in DEVELOPMENT.md | every Rust workflow green on the pinned version | CI |
| A2 | `make third-party-notices-check` in `rust-tests.yml` and `shim-tests.yml` | the check runs and passes in CI | CI |
| A3 TD-015, TD-023, TD-020, TD-022 | split `src/api/tests.rs` by route family and `src/api_behavior_matrix.rs` under 800 lines; move `RouteSpec` to `spec` (re-export from `api` for one release); delete the dead `PATCH /vm` arm | the structure sensor prints no warning; `cargo test` count unchanged | CI |
| A4 TD-006 | one formatting-only commit, its SHA in `.git-blame-ignore-revs`; then `make lint` (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`) in `rust-tests.yml` | lint step green | CI |
| A5 | TD-012 one docs checker; TD-016 a weekly `schedule:` on `compat-exceptions.yml`; TD-018 `use` trees and every `src/**/*.rs` in `check_structure.py`; TD-008 a test that only `src/bin/vesperd.rs` binds an API listener; TD-003 a check that every pinned route has behaviour cases and none answers `501`; TD-014 archive `docs/references/*`; TD-011 drop the unmet bundle expectations; TD-002 a decision-log line; TD-004 closed with the evidence above | each check fails on a planted fault and passes on `main` | CI |

### B Docker containers in microVMs (GH #53)

- **B1 spike (time-boxed, on the live System VM, evidence in `run/B1/`).** Answer, by measurement:
  1. Does dockerd attach the container's veth itself or through a prestart hook the shim does not run? Dump the
     `config.json` dockerd gives containerd (hooks, network namespace path).
  2. Which user namespace owns the container's network namespace under `userns-remap`?
  3. Can Firecracker, joined to the container's namespace, use a tap mirred to the veth with `tc`, while the share
     tap reaches `unfsd` through a veth pair into its private namespace, so the share server's stack sees no bridge
     packet? (Needs `iproute2-tc`, approved, and the `sch_ingress` and `act_mirred` modules.)
  4. Does the guest reach dockerd's embedded DNS (`127.0.0.11` on user-defined networks), or what carries it?
  5. Can the root drive be a block device or reflinked file on the data disk instead of a copy in tmpfs?
- **B2 ADR 0011**, with a council that includes one reviewer of another model family (the threat-table pre-review of
  TD-051, R-8). It settles: the spec-field table (honoured, ignored, refused), authz changes (allow the `vesper`
  runtime by exact name; refuse `--network container:`, network connect into a vesper container and the reverse,
  host devices, binds outside share mounts), the network and spoofing filters, mount classes (`resolv.conf`,
  `hosts`, `hostname`, named volumes, `tmpfs`), uid ownership under the remap, the root-drive model (TD-035,
  TD-025, TD-031), Pause and Resume by vCPU pause with Checkpoint refused (TD-042), `Update`, and the amendments to
  ADR 0009 and ADR 0010 by name. SECURITY.md then states what a microVM container reaches.
- **B3 its own ExecPlan and slices.** Acceptance: `docker run --runtime vesper` starts a microVM; the Phase 4 suites
  (`docker-conformance`, `ports-suite`, `binds-suite`, `dns-suite`, `workflow-suite`, `crossarch-suite`) pass with
  the runtime forced by a per-suite variable, within listed deviations; a five-service compose with images of
  ordinary size fits the default System VM, or the limit is recorded; p50 and p95 for `docker run` and `docker stop`;
  containerd and dockerd restarts; the memory booking is released when a container exits; `docker cp`, `diff`,
  `commit` and `export` work or are refused with a clear error, never a silent success.

### C API and proxy fixes (CI)

| Item | Work | Done when |
| --- | --- | --- |
| TD-019 | the body limit and body-read failures answer with an error envelope (`fault_message` naming the cause); a poisoned lock maps to a `500` envelope | tests for 413 and a broken body |
| TD-024 | caller-given host files are opened in `spawn_blocking` | a test with a FIFO as `path_on_host` that does not stall a second request |
| TD-017 | `vesper-proxy --entitlements-file`, the same check as `vesperd` | test |
| TD-021 | idle timeout on the `vesper-proxy` relay, reusing `src/proxy/relay.rs`'s timeout | test |
| TD-007 | `tracing-subscriber` (no `env-filter`): one span per request in `vesperd` with a request id, one per connection in `vesper-proxy`, to stderr | test that reads the span line |

### D Runtime fixes and smaller decisions (System VM suites)

| Item | Work |
| --- | --- |
| TD-041 | reclaim an exec's port and session directory when its streams and process are gone |
| TD-060 | keep the jailer's pid; stop the boot poll when it is gone; prune reaped console logs by age |
| TD-059 | judge the alone attempt against the classes; parse the last attempt's message |
| TD-064 | refuse or report a file with outside hard links before `import-volume`; failure hooks |
| TD-044 | a launcher mode that decodes a launch config and prints the shares, used by a test |
| TD-026 | keep the line protocol; reserved slots for control commands apart from `LAUNCHAPI` and `FCAPI`; an idle timeout on control shells (socat `-T`); a golden test of the verbs and replies |
| TD-037 | UDP published ports on loopback only (owner, D-066): an ADR 0005 amendment, a bounded flow table, idle timeouts; non-loopback binds stay a deviation |
| TD-039 | a decision first: how `host.docker.internal` is injected (dockerd `host-gateway-ip` plus an extra host for every container, or a DNS record in the System VM's resolver) and whether AAAA answers are dropped without an IPv6 route (it settles part of the IPv6 gate) |
| TD-053 | a decision first: parse `PUT /snapshot/create` in the relay to check the reserve (amends ADR 0007's opaque relay) or check the reserve on a timer and refuse new leases |

### E Sandbox and signing (ADR 0012)

ADR 0012 lists the final entitlement set for the launcher, `vesperd` and `vesper-proxy`, and how a sandboxed
launcher reaches user-named shares, the data disk and the bridge socket. Then App Sandbox or a seatbelt profile,
hardened runtime and ad-hoc signing for all three, `vesper-proxy` first; a sandbox test that runs real shares, the
data disk and `docker.sock`; a macOS CI job that runs `codesign --verify` and the key check (closes TD-010). No app
bundle, launchd agent or release (TD-046 keeps those for a release plan).

### F Evidence

- TD-005 narrowed: `make nightly` runs `make systemvm-reliability` and the suites, each report with the SHA and the
  baseline metadata, uploaded as a workflow artifact by a manual dispatch; a CI check warns on stale reports and
  blocks only parity claims that cite one. Owner-hardware evidence never counts as remote green. A launchd schedule
  is documented, not installed. The row stays open for a self-hosted runner.
- TD-047: request-parser property tests with `proptest` (dev only) now; the soak, iperf3, recovery, jailer
  equivalence and host-crash suites move to the parity-GA plan, recorded in the row.
- TD-033: after B, concurrent create rate, memory per microVM and a block I/O probe.
- TD-052: remove the two unmeasurable phrases from design-v0 with a decision row.

### G Deviations and compat exceptions

- TD-054: check whether Alpine's `linux-lts` flavour has `CONFIG_USERFAULTFD`; if it does, a pinned package change
  with its own review; if not, a compat exception (approved, D-066) with a revisit trigger.
- TD-057: a compat exception (approved, D-066): a `vesperd` or System VM restart ends its microVMs; RELIABILITY.md
  states this for `vesperd` leases and for shim tasks.
- TD-058: the VPN-prefix limit stays a deviation, stated for `vesperd` microVMs only.
- TD-038, TD-067, TD-074: deviations kept (D-038, D-046); no new work unless a user asks.
- TD-036: narrowed to what B does not cover, or closed.
- TD-040: no new interpreters until a user asks; a CPU and I/O benchmark step only.
- TD-013: left to the owner (the ADR index says the people who made those decisions write them).
- TD-051: the pre-review of B2; the row stays open for the independent human review.
- TD-046: the release plan (bundle, launchd agent, Developer ID) waits for the owner's Apple identity.

### H Tracker truth

- GH #96 mirrors closed #49: close with that note.
- GH #59: correct `go test ./...` to the Rust checks; close only when its criteria hold in CI.
- GH #103 keeps `go test` (the shim is Go); close only when its criteria hold.
- GH #56: record that #32 to #36 and #40 were closed with no code by the retired promotion workflows; the open
  mirrors are listed from GitHub.

## Concrete Steps

For each step:
1. Make a branch, then write the code and its tests together.
2. Run the narrowest check, then `cargo test`, `make shim-test` when `shim/` changes, `python3 harness/check_docs.py`
   and `python3 scripts/harness/check_structure.py --root .`. Runtime steps run their System VM suite on a freshly
   booted System VM, with the evidence in `run/<id>/`.
3. Runtime and security steps get an llm-council review with a judge before merge; designs get EGM.
4. Open the PR, wait for green CI, squash-merge, and post the SHA and the local and remote results on the linked
   issue or tracker row.
5. Write a decision-log row for each decision.

## Validation and Acceptance

Each table above gives its own "done when". The programme is done when every open row on 2026-11-22 is done, a
recorded deviation or compat exception, or waits on a named person-only input, and B3's acceptance holds.

## Idempotence and Recovery

Each step is a separate PR and can be reverted alone. System VM changes are tested on a rebuilt overlay
(`make systemvm-overlay`, `make systemvm-up`) and recovered by rebuilding from `main`. The B1 spike changes nothing
in the tree; its probes run in a disposable System VM boot.

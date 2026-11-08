---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-06
---

# Observability

This page describes what the system emits, where to see it and how an agent can diagnose a problem without human help.
Only capabilities that exist are documented; missing ones are listed under [Gaps](#gaps).

## Signals

| Signal | Implemented? | Library / format | Local destination | Production destination |
|---|---|---|---|---|
| Structured logs | No | Plain `println!`/`eprintln!` only; no `log` or `tracing` crate in `Cargo.toml` | `run/vesperd.log`, `run/vesper-proxy.log` when started by `make dev-up` | n/a |
| Metrics | No | `PUT /metrics` validates the path by creating the file empty (`create` + `append` open), stores it in `AppState`, and never writes to it ([src/api/logger_metrics.rs](../src/api/logger_metrics.rs)) | n/a | n/a |
| Traces | No | none | n/a | n/a |
| Health / readiness | Partial | `GET /` returns instance info and `GET /version` the pinned Firecracker version ([src/api.rs](../src/api.rs)); `vesperd` prints `vesperd listening on <socket>` once bound | the API socket | n/a |
| Dashboards | No | none | n/a | n/a |
| Alerts | No | none | n/a | n/a |

What the binaries do emit (**Verified** by grep of `src/`): each tool prints one success line to stdout
(`generated metadata at …`, `golden contract is up to date: …`, `wrote image manifest: …`) and the error string to
stderr with exit code 1. `vesper-proxy` prints `vesper-proxy failed: <err>` on fatal errors only; per-connection
failures are not logged.

## Correlation

None. There are no request IDs or trace context. The only correlation handle is the deterministic `ErrorEnvelope`,
whose `path`, `method`, `code` and `baseline_firecracker_revision` fields identify the route template and the pinned
baseline a response was produced against.

## Key metrics and events

The durable signals this project relies on are artifacts rather than live telemetry:

- `run/systemvm/boot.log` (one `result=… duration_ms=… boot_ms=…` line per iteration plus the launcher's
  `stdout=` JSON lines) and `run/systemvm/boot-report.json` (`iterations`, `successful`, `failed`, free-text
  `failures`, `durations_ms`, `max_duration_ms`, `mean_duration_ms`, `boot_ms`, `max_boot_ms`,
  `max_boot_allowed_ms`) from `systemvmctl boot-smoke`; `run/systemvm/console.log` (guest serial console of the
  last boot, including the probe's login, DHCP lease and `VESPER-NAT-OK`) and JSON status lines (`ready`,
  `probe-ok`, `stopped`, …) from `vesper-vmlauncher`; `run/systemvm/vmlauncher.log` (those lines for the VM kept
  up by `make systemvm-up`, plus bridge errors on stderr) and `run/systemvm/control-report.json` and `run/systemvm/services-report.json`
  (`steps[]` with `name`, `ok`, `detail`, `elapsed_ms`, and `passed`) from `systemvmctl control suite` and
  `services-suite`; the agent's `HEALTH` line (`packages_ready`, `services_ready`, OpenRC state per service,
  `containerd_ok`, `docker_ok`, `docker_version`); guest logs through `systemvmctl control logs
  <messages|containerd|docker>` (`/var/log/messages` holds the `supervise-daemon` respawn lines);
  `run/vesper/vesper-proxy.log` and `run/systemvm/docker-smoke-report.json` from `make docker-up` and
  `make docker-smoke`; `run/systemvm/reliability-report.json` (sections per suite, boot statistics and drift, leak
  snapshots, `failures[] {step, reason, evidence}`, `passed`) and `run/systemvm/reliability.log` from
  `make systemvm-reliability`; `run/systemvm/microvm-report.json` from `make systemvm-microvm-suite` and the
  microVM serial logs `/var/log/vesper-microvm-<id>.log` in the guest (`systemvmctl control microvm log <id>`);
  `run/systemvm/rootfs-report.json` from `make systemvm-rootfs-suite` and the provenance sidecars
  `/var/lib/vesper/rootfs/<id16>.ext4.provenance.json` in the guest (`systemvmctl control rootfs provenance <path>`);
  `run/systemvm/workload-report.json` from `make systemvm-workload-suite`, `systemvmctl control workload ps` and
  `systemvmctl control workload logs <name>` (the output between the workload markers);
  `run/systemvm/parity-baseline-report.json` from `make systemvm-parity-baseline` (conformance checks with each
  response, both performance runs, compat exception summary) and `systemvmctl control fcparity api <id> GET /`
  for a live microVM's state; `run/systemvm/shim-report.json` from `make systemvm-shim-suite`, `systemvmctl
  control ctr state <name>` and `ctr -n vesper task ls` in the guest; the shim logs through containerd's shim log
  FIFO (`ctr` prints them with `--debug`), and each task's console is `/var/log/vesper-microvm-<task>.log`; `run/systemvm/bundle-report.json` from
  `make systemvm-bundle-suite`, `systemvmctl control ctr meta <task>` (the task's `vesper-rootfs.json`: strategy,
  cached, key, layers, digest, build time) and `systemvmctl control ctr cache` (cached root drives); `run/systemvm/restart-report.json` from
  `make systemvm-restart-suite` and `systemvmctl control ctr state-file <task>` (the task's `vesper-task.json`:
  phase, VMM pid, exit, shim pid, last update); `run/systemvm/docker-conformance-report.json` from
  `make docker-conformance` (each step's dockerd response or error text); `run/vesper/ports.json` (the live
  published-port mappings with state and error) and `ports:` lines in `run/vesper/vesper-proxy.log`;
  `run/systemvm/ports-report.json` from `make ports-suite`; `systemvmctl control share list|probe` for the
  guest's virtio-fs mounts and `run/systemvm/binds-report.json` from `make binds-suite`.
- `build/systemvm/image-manifest.json` (SHA-256 digests, stable `image_id`) and `build/systemvm/launch-config.json`.
- `artifacts/parity-metadata.json` from `baselinectl --mode generate`.
- JSON and Markdown reports from the Python sensors (`--json-out`, `--markdown-out`), uploaded by the `harness-*`
  workflows.
- `.symphony/runs/<ISSUE-ID>/current/` verification bundles ([RELIABILITY.md](RELIABILITY.md)).

## Agent diagnosis loop

1. **Start an isolated instance.** `make dev-up` in the worktree; sockets, pids and logs land under that worktree's
   `run/`, so parallel worktrees do not collide.
2. **Reproduce the issue.** Talk to the API over the socket, for example
   `curl --unix-socket run/vesperd.sock -X PUT http://localhost/machine-config -d '{"vcpu_count":2,"mem_size_mib":512}'`,
   and compare the body with the expected `ErrorEnvelope` or status in
   [docs/generated/firecracker-api-behavior-matrix.json](generated/firecracker-api-behavior-matrix.json).
3. **Inspect logs.** `tail run/vesperd.log run/vesper-proxy.log`. Expect only the startup line and fatal errors.
4. **Query metrics and traces.** **Not supported.**
5. **Validate the fix.** Re-run the targeted test (`cargo test --test api_contract <name>`) and the reproduction.
6. **Capture evidence.** `scripts/harness/init_issue_run.sh <ISSUE-ID>` then
   `scripts/harness/collect_verification_bundle.sh <ISSUE-ID>` copies boot reports, proxy logs and request/response
   logs into `.symphony/runs/<ISSUE-ID>/current/`; paste the transcript into the issue.

## Gaps

- No structured logging or log levels in any binary (TD-007).
- `PUT /metrics` and `PUT /logger` create the target file and store its path; nothing is ever emitted to it (TD-007).
- `make dev-up` writes `run/vesper-proxy.log`, but the bundle collector only picks up `run/<ISSUE-ID>/proxy/proxy.log`
  or `run/proxy/proxy.log`; copy it by hand (TD-011).
- No per-connection logging in `vesper-proxy`, so forwarding failures are invisible without a packet capture (TD-007).
- No request or trace identifiers (TD-007).
- `collect_verification_bundle.sh` expects `run/<ISSUE-ID>/api/requests.log` and `responses.log`, but no code writes
  them; they must be produced by hand (TD-011).

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

- `run/systemvm/boot.log` (one `result=` line per iteration) and `run/systemvm/boot-report.json` (`iterations`,
  `successful`, `failed`, free-text `failures`) from `systemvmctl boot-smoke`.
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

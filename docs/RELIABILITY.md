---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-11
---

# Vesper Reliability and Legibility

Harness engineering for Vesper means making runtime behavior legible to the agent and durable for reviewers.

## Principle

For runtime-sensitive work, terminal output is not enough. The repository and issue bundle need durable artifacts that can be re-read by a later agent or reviewer.

## Required artifact classes

### 1. Command transcripts

Always preserve:
- exact commands run,
- exit status,
- relevant environment flags,
- the Git SHA.

### 2. Control-plane artifacts

When API behavior changes, preserve:
- request / response examples,
- deterministic error envelopes,
- route and state-machine test outputs.

### 3. Proxy artifacts

When transport changes, preserve:
- proxy logs,
- target parsing examples,
- socket-forwarding proof (unix, tcp, or vsock as relevant).

### 4. System VM artifacts

When System VM behavior changes, preserve:
- `build/systemvm/image-manifest.json` if generated,
- `build/systemvm/launch-config.json` if generated,
- `run/systemvm/boot.log`,
- `run/systemvm/boot-report.json`,
- any console or launcher logs.

### 5. Governance artifacts

When baseline or compat rules change, preserve:
- baseline metadata,
- compat registry validation output,
- any new exception IDs and rationale.

## Standard directories

Use:
- `run/<ISSUE-ID>/` for worktree-local runtime artifacts,
- `.symphony/runs/<ISSUE-ID>/current/` for durable verification bundles.

The helper scripts:
- `scripts/harness/init_issue_run.sh`
- `scripts/harness/collect_verification_bundle.sh`

exist to keep these directories predictable. `init_issue_run.sh` creates `run/<ISSUE-ID>/{api,proxy,systemvm}` and
`.symphony/runs/<ISSUE-ID>/current/{api,proxy,systemvm,council}`; the collector reads `run/<ISSUE-ID>/proxy/proxy.log`,
`run/<ISSUE-ID>/api/{requests,responses}.log` and `run/<ISSUE-ID>/systemvm/{boot.log,boot-report.json}`. Nothing writes
into `run/<ISSUE-ID>/` automatically: `make dev-up` writes `run/vesperd.log` and `run/vesper-proxy.log`, and
`make systemvm-boot-smoke` writes `run/systemvm/`, so copy artifacts into the issue directory before collecting.
`.symphony/runs/` is gitignored; a bundle is durable only once its contents are attached to the GitHub issue or PR.

## Stage-gated reliability expectations

### API / parity work

Minimum:
- targeted regression,
- broad contract regression,
- deterministic error proof if the endpoint can fail.

### Proxy work

Minimum:
- targeted proxy regression,
- direct forwarding proof,
- captured connection logs (from an external capture; `vesper-proxy` logs nothing per connection, TD-007).

### System VM work

Minimum:
- targeted `cargo test systemvm`,
- `make systemvm-fetch` green (digests and `image_id` verified),
- launch-config artifact,
- `make systemvm-boot-smoke` report with zero failures on Apple silicon, copied with `console.log` into
  `run/<ISSUE-ID>/systemvm/`; the 200-boot reliability bar from design-v0 for any change to the launcher, the
  image lock or the kernel command line,
- `make systemvm-up && make systemvm-control-suite && make systemvm-down` with `passed: true` in
  `run/systemvm/control-report.json` for any change to the launcher bridge, the overlay services or the agent,
- `make systemvm-services-suite` with `passed: true` in `run/systemvm/services-report.json` for any change to
  the guest services, the package pins or cgroup configuration,
- `make systemvm-reliability` (50 cycles; 200 for launcher, lock or kernel command-line changes) with
  `passed: true` in `run/systemvm/reliability-report.json`, copied into `run/<ISSUE-ID>/systemvm/`.

### Governance work

Minimum:
- validation command output,
- updated docs or registry evidence,
- milestone note explaining the policy impact.

## Reliability budgets

The design spec remains the normative source for hard targets. For local issue work:
- prefer 1–5 iteration local boot proof for fast validation,
- reserve 200-iteration stage gates for dedicated verification environments,
- do not infer long-run reliability from a single successful boot.

## Escalation rule

If a change cannot be made legible with the current artifacts, stop and extend the harness before shipping more code.

## Bridge client death (fixed 2026-11-08)

`vesper-vmlauncher` relays bridge bytes with raw `write()`; until WOR-34 a client (vesper-proxy) that died while
the guest was still sending made the relay take SIGPIPE and the whole VM went with it. The launcher now ignores
SIGPIPE and sets `SO_NOSIGPIPE` on bridge clients; `make ports-suite` keeps a step that restarts the proxy under
a live event stream and checks the launcher pid survives.

## Share watcher (GH #150)

The launcher's share watcher sends the paths of host edits on watched read-write shares to the guest's sink
(`vesper-share notify`, guest vsock port 2401), which replays them as inotify events
([systemvm-bringup.md](process/systemvm-bringup.md)). It is a convenience layered on the shares, not part of
them.

A watcher or sink failure is degraded, never fatal. Events stop; the VM, Docker and the shares keep working, and
files stay current on access. The cases:

- The FSEvents stream cannot start: status line `share-watch-failed` and stderr
  `share-watch: FSEventStreamCreate failed; host edits will not reach the guest` (or `FSEventStreamStart`). No
  events until the launcher restarts.
- The sink is not reachable or goes away: stderr `share-watch: write to guest port 2401 failed (…); reconnecting`.
  The watcher retries every 2 s and logs `connected to guest port 2401 (connection N)` when it is back; the
  guest's `supervise-daemon` restarts the listener, which runs at most four handlers (`max-children=4`).
- The sink stops reading: the watcher queues lines and retries the write every 50 ms (`stalls` grows).

The watcher cannot hold up shutdown. Its writes are non-blocking and the exit report reads counters published
under a lock, so SIGTERM and a guest shutdown never wait for the sink. Until 2026-11-11 a sink that stopped
reading could block SIGTERM until the launcher was killed; the council's runtime judge rejected GH #150 for it
([decision log](design-docs/decision-log.md) D-006). `make share-watch-selftest` keeps the check
`stalled_sink_sigterm_exits`, which reports `hung` when the blocking write is restored. Measured on a booted VM
with the sink handler stopped and a 30,000-file host burst (`run/GH-150/m5-stalled-sink.txt`, local): the
launcher exited 0.18 s after SIGTERM with `queued=26149 stalls=621 rescans=5`, and 8.05 s after a guest
shutdown with status `stopped`.

Notifications can be lost; file contents cannot. The known losses:

- The queue holds 100,000 lines. Beyond that the watcher drops the oldest lines not yet on the wire (`dropped`)
  and forgets them, so a later edit of the same path is sent again.
- A rescan after FSEvents drops events visits at most 50,000 entries and sends only those modified in the last
  60 s.
- Lines already written when the sink handler dies can be lost, normally one batch of up to 1,000: they are
  counted `sent` and are not sent again until the path changes again.

The `share-watch-summary` status line, printed before `terminating` or `stopped`, carries the counters as of the
watcher's last write attempt:

| Counter | Meaning |
|---|---|
| `forwarded` | Paths accepted for sending; equals `sent` + `queued` + `dropped` |
| `sent` | Lines written to the sink |
| `echoes` | Events dropped because the path's mtime and size were already sent; mostly the guest's own replay coming back |
| `ignored` | Events outside the watched shares, for a share's root directory, or with a newline in the path |
| `dropped` | Lines discarded because the queue passed its limit |
| `connects` | Connections made to the sink; more than 1 means it reconnected |
| `stalls` | Write attempts that found the sink not reading |
| `rescans` | Times FSEvents reported dropped events and the watcher walked the directory; each logs `FSEvents dropped events under <path>; rescan found <n> recently changed path(s)` |
| `queued` | Lines still waiting for the sink |

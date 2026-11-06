---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-06
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
  the guest services, the package pins or cgroup configuration.

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

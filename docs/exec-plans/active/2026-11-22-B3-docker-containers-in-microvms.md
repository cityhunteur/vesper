# B3: Docker containers in microVMs (ADR 0011)

The ExecPlan for step B3 of the 2026-11-22 programme: build ADR 0011 in its six slices, so that
`docker run --runtime vesper` from macOS runs a container in a jailed Firecracker microVM. The owner approved the
opt-in runtime, `iproute2-tc` and the ADR's changes to ADR 0009 and ADR 0010 (D-066); the ADR is accepted (D-068).

## Purpose / Big Picture

After this plan a developer on an M3 or later Mac runs `docker run --runtime vesper alpine …` and gets the same
container as with runc, except that it runs in its own microVM: its address, published ports, peers by name on a
user-defined network, outbound access, writes that survive `docker stop` and `docker start`, and shares from
macOS. Each Docker command that cannot be honest about a microVM is refused with a message that names ADR 0011.

## Progress

- [x] Slice 1: `iproute2-tc` with `libelf` and `zstd-libs` in the package lock, no other pin moved; the boot probe
  checks `tc` (2026-11-22, `run/B3/s1-*.log`)
- [ ] Slice 2: the default bridge (`daemon.json`, plugin allow and create refusals, holder and namespace, root drive
  with ownership shift on the data disk, data link with allow-list, `resolv.conf`/`hosts`/`hostname`, spec table,
  cleanup scopes, sweep; `make systemvm-docker-microvm-suite`)
- [ ] Slice 3: user-defined networks (the DNS link); `dns-suite` and compose steps with `VESPER_DOCKER_RUNTIME=vesper`
- [ ] Slice 4: shares for `vesper` containers and `ctr` tasks; the Phase 4 suites with the runtime forced; latency,
  density and restart measurements
- [ ] Slice 5: honesty refusals (`cp`, `diff`, `export`, `commit`, `connect`) and Pause and Resume
- [ ] Slice 6: only if slice 4's numbers call for it, a shared base drive (TD-035)
- [ ] Outcomes & Retrospective written; plan moved to completed

## Surprises & Discoveries

- Slice 1: a full re-resolve of the package lock would also have moved `runc` from 1.3.4 to 1.3.7, so the three new
  packages were added by hand from Alpine's signed index, with their digests; `zlib` and `libcap2` are on the boot
  image and are not pinned, as the resolver treats them.

## Decision Log

- 2026-11-22: slice order as ADR 0011 states (the scope review's order): every create refusal lands with slice 2,
  so the runtime is never on before its refusals.

## Outcomes & Retrospective

Not yet written.

## Context and Orientation

- ADR 0011 is the design; `run/B1/findings.md` the spike; `run/egm/ADR-0011-review.md` the reviews.
- The shim is `shim/` (Go; `make shim-test`); its microVMs are launched by
  `systemvm/overlay/usr/local/bin/vesper-microvm`; the plugin is `shim/internal/authz/`; dockerd's config is written
  by `systemvm/overlay/usr/local/bin/vesper-containers`.
- Every suite here needs the System VM: an M3 or later Mac with nested virtualisation (docs/TESTING.md).

## Plan of Work

Each slice is one PR with an llm-council review (security, reliability, correctness and a judge) before merge,
and its suite on a freshly booted System VM with the evidence in `run/B3/`. Slice 2 is the largest; it may ship as
two PRs (2a the shim and plugin with unit tests, 2b the System VM scripts and the suite), but the runtime is
registered in `daemon.json` only in the PR that also carries every refusal.

## Concrete Steps

1. Branch; code and tests together; `make shim-test`, `cargo test`, `make lint`.
2. `make systemvm-overlay && make systemvm-up`, then the slice's suite and the regression suites it touches.
3. Council and judge; fixes; the suites again.
4. PR, green CI, squash-merge; a comment on GH #53 with the SHA, local and remote results and the gaps.

## Validation and Acceptance

Slice 2: `make systemvm-docker-microvm-suite` passes every step ADR 0011 names for it, and fails if the data link,
the allow-list or a refusal is removed. Slice 4: each Phase 4 suite passes with `VESPER_DOCKER_RUNTIME=vesper`,
or each failing step is a recorded deviation. The plan is done when slices 1 to 5 hold, and slice 6 is built or
recorded as not needed with slice 4's numbers.

## Idempotence and Recovery

Each slice can be reverted alone. Before slice 2 the runtime is not registered, so nothing a user does reaches
the new code. A System VM changed by a failed run is recovered by rebuilding the overlay from `main` and booting
again; `vesper-ctr leftovers` and the new sweep report what is left.

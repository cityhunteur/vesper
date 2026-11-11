---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-11
---

# Specifications and baselines

The normative sources for Vesper's behaviour and parity claims. [CLAUDE.md](../../CLAUDE.md) names the two
Markdown documents as the authoritative design and parity references; the YAML files are read by the binaries.

Both Markdown documents are `status: draft`. The design states requirements that the code does not yet meet: its
section [Implementation status](design-v0.md#implementation-status-as-of-2026-11-11-commit-9c5a865) lists the gaps at
commit `9c5a865`. After that date the status is kept in the parity matrix and in the
[milestone log](../progress/symphony-milestones.md).

| File | Role | Read by |
|---|---|---|
| [design-v0.md](design-v0.md) | Normative product and architecture design (draft): requirements, implementation status at `9c5a865`, backends, phases, verification and test strategy, security model | humans, council rubric |
| [firecracker-feature-parity.md](firecracker-feature-parity.md) | Parity matrix (draft; route and field level) and the GitHub issue eval audit with successor issues; holds the implementation status after 2026-11-11 | humans, milestone reports |
| [design-v0.baselines.lock.yaml](design-v0.baselines.lock.yaml) | Pinned upstream revisions for Firecracker, containerd and Apple Containerization, plus required parity metadata keys | `vesperd`, `baselinectl` (`src/baseline.rs`) |
| [backend-a-parity-baseline.json](backend-a-parity-baseline.json) | Committed Backend A parity and performance capture (GH #21): lock revisions, Firecracker release and API version, compat exception summary, 19 conformance checks, two performance runs. Captured at the previous Firecracker pin `083c4cf`, before GH #162 moved the lock to v1.16.0; it needs a re-capture | `make parity-baseline-check`; `Parity Baseline` workflow validates and publishes it |
| [ctr-conformance-baseline.json](ctr-conformance-baseline.json) | Committed ctr conformance capture for the Vesper runtime (GH #26): lock revisions, 74 steps in four suites (`shim`, `bundle`, `restart`, `exec`), checklist mapping; generated 2026-11-09 at the previous Firecracker pin `083c4cf`, before GH #162 moved the lock to v1.16.0; it needs a re-capture | `make ctr-conformance-check`; `ctr Conformance` workflow validates and publishes it |
| [compat-exceptions.yaml](compat-exceptions.yaml) | Registry of allowed behaviour deviations before GA (CE-0001 upstream pytest suite not run, CE-0003 no Linux same-host baseline, CE-0004 in-guest security posture override; all open, expiry 2027-03-31. CE-0002, release binary vs pinned commit, was closed by GH #162) | `compatexctl` (`src/compat.rs`) |
| [firecracker-api-baseline-captures.yaml](firecracker-api-baseline-captures.yaml) | Empty placeholder (`captures: []`) for matrix-backed placeholder routes; no route uses it and generation requires it to stay empty | `baselinectl` (`src/api_matrix.rs`) |
| [firecracker-api-behavior-captures.yaml](firecracker-api-behavior-captures.yaml) | Upstream evidence for route-level status and error behaviour | `baselinectl` (`src/api_behavior_matrix.rs`) |
| [systemvm-packages.lock.yaml](systemvm-packages.lock.yaml) | Pinned Alpine packages (URL, SHA-256, size) installed into the System VM at boot through the overlay: `socat`, `containerd` with `ctr`, Docker, `e2fsprogs`, `e2fsprogs-extra`, `qemu-x86_64`, `runc`, `tini-static` and their closure (22 files), plus digest-pinned binary archives (`archives`: Firecracker and jailer v1.16.0 from `systemvm/archives.yaml`) | `systemvmctl package-list` / `verify-packages` (`src/systemvm/packages.rs`), `scripts/systemvm/fetch-packages.sh`, `build-overlay.sh` |
| [systemvm-image.lock.yaml](systemvm-image.lock.yaml) | Pinned System VM boot image (Alpine virt ISO): URL and SHA-256, kernel and initramfs members with unpack method and digests, boot media, kernel command line, console ready pattern, 5 s boot threshold, nested-virtualization policy (`auto`), guest probe (NAT proof, installed packages, agent up, KVM presence, power-off), overlay section (ADR 0002), control channel (bridge dir, cid, agent port; ADR 0003), expected `image_id` | `systemvmctl lock-env` / `verify-image` / `launch-config --from-lock` (`src/systemvm/image_lock.rs`), `scripts/systemvm/fetch-image.sh`, `systemvm-image.yml` |
| `assets/` | Mermaid sources and PNG renders of the design-v0 component and lifecycle diagrams. Both show the as-built flow of 2026-11-11; regenerate a PNG with `mmdc -i <name>.mmd -o <name>.png -b white -s 2` after changing its source | design-v0.md |

Changing any YAML file follows [baseline-governance.md](../process/baseline-governance.md),
[compat-exceptions.md](../process/compat-exceptions.md) or, for the image lock,
[systemvm-bringup.md](../process/systemvm-bringup.md).

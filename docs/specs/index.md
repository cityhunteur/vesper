---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-06
---

# Specifications and baselines

The normative sources for Vesper's behaviour and parity claims. [CLAUDE.md](../../CLAUDE.md) names the two
Markdown documents as the authoritative design and parity references; the YAML files are read by the binaries.

| File | Role | Read by |
|---|---|---|
| [design-v0.md](design-v0.md) | Normative product and architecture design: requirements, backends, phases, verification and test strategy, security model | humans, council rubric |
| [firecracker-feature-parity.md](firecracker-feature-parity.md) | Parity matrix (route and field level) and the GitHub issue eval audit | humans, milestone reports |
| [design-v0.baselines.lock.yaml](design-v0.baselines.lock.yaml) | Pinned upstream revisions for Firecracker, containerd and Apple Containerization, plus required parity metadata keys | `vesperd`, `baselinectl` (`src/baseline.rs`) |
| [compat-exceptions.yaml](compat-exceptions.yaml) | Registry of allowed behaviour deviations before GA | `compatexctl` (`src/compat.rs`) |
| [firecracker-api-baseline-captures.yaml](firecracker-api-baseline-captures.yaml) | Upstream evidence for matrix-backed placeholder routes | `baselinectl` (`src/api_matrix.rs`) |
| [firecracker-api-behavior-captures.yaml](firecracker-api-behavior-captures.yaml) | Upstream evidence for route-level status and error behaviour | `baselinectl` (`src/api_behavior_matrix.rs`) |
| [systemvm-packages.lock.yaml](systemvm-packages.lock.yaml) | Pinned Alpine packages (URL, SHA-256, size) installed into the System VM at boot through the overlay: `socat`, `containerd` with `ctr`, Docker, `e2fsprogs`, `e2fsprogs-extra` and their closure (21 files), plus digest-pinned binary archives (`archives`: Firecracker and jailer v1.16.0 from `systemvm/archives.yaml`) | `systemvmctl package-list` / `verify-packages` (`src/systemvm/packages.rs`), `scripts/systemvm/fetch-packages.sh`, `build-overlay.sh` |
| [systemvm-image.lock.yaml](systemvm-image.lock.yaml) | Pinned System VM boot image (Alpine virt ISO): URL and SHA-256, kernel and initramfs members with unpack method and digests, boot media, kernel command line, console ready pattern, 5 s boot threshold, nested-virtualization policy (`auto`), guest probe (NAT proof, installed packages, agent up, KVM presence, power-off), overlay section (ADR 0002), control channel (bridge dir, cid, agent port; ADR 0003), expected `image_id` | `systemvmctl lock-env` / `verify-image` / `launch-config --from-lock` (`src/systemvm.rs`), `scripts/systemvm/fetch-image.sh`, `systemvm-image.yml` |
| `assets/` | Mermaid sources and PNG renders of the design-v0 component and lifecycle diagrams | design-v0.md |

Changing any YAML file follows [baseline-governance.md](../process/baseline-governance.md),
[compat-exceptions.md](../process/compat-exceptions.md) or, for the image lock,
[systemvm-bringup.md](../process/systemvm-bringup.md).

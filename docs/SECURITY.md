---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-06
---

# Vesper Security Review Map

Vesper’s security model is part of its parity contract. Treat this file as the entry point for security-sensitive work.

## Trust boundaries

1. Host macOS control plane
   - `vesperd`
   - host-visible Unix sockets
   - entitlement-bearing binaries
2. Linux System VM
   - containerd and internal runtime surfaces
   - optional stock Firecracker and jailer for Backend A
3. Guest microVM workloads
   - isolated workloads behind the control plane

## Host exposure rules

- `vesperd` is the public control-plane endpoint.
- `vesper-proxy` may expose a host-visible socket only for the intended Docker or container runtime UX.
- Raw Firecracker and raw containerd sockets stay private to the System VM.
- Shipped defaults (`src/bin/vesperd.rs`, `src/bin/vesper-proxy.rs`): `vesperd` binds `/tmp/vesperd.sock` with mode
  `0o660`; `vesper-proxy` listens on `/tmp/vesper/docker.sock` with mode `0o600` (`--listen-mode`) and dials
  `vsock://3:2375`, which is bridged through a host Unix socket `/tmp/vesper/vsock/<cid>.sock` with a `CONNECT
  <port>` preface. Those are world-writable temp directories; `make dev-up`, `make systemvm-up` and `make docker-up`
  use `run/` instead, and packaged builds must pass explicit paths.
- Host-visible sockets with the System VM running (`make systemvm-up && make docker-up`), all under `run/` and
  owner-only: `run/vesper/docker.sock` (the Docker ingress, served by `vesper-proxy`) and `run/systemvm/vsock/3.sock`
  (the launcher's vsock bridge, which reaches any guest vsock port; TD-027). Inside the guest only two vsock ports
  listen: 1024 (`vesper-agent`) and 2375 (relay to `/var/run/docker.sock`). `containerd.sock` and `docker.sock`
  themselves never leave the guest, and a `CONNECT` to a guest port without a listener is refused.
  `make docker-smoke` checks the socket list and the refusal (`no_raw_host_sockets`, `raw_guest_port_denied`).

## Entitlements

- Plists: `packaging/macos/vesperd.entitlements` (`com.apple.security.app-sandbox`, `com.apple.security.virtualization`)
  and `packaging/macos/vesper-proxy.entitlements` (`com.apple.security.app-sandbox`). `make validate-entitlements`
  lints them; with `plutil` present it checks syntax only, and the required-key check runs on the Python path
  (`VALIDATE_ENTITLEMENTS_FORCE_PYTHON=1`). CI runs it in `rust-tests.yml`.
- `vesperd --entitlements-file <plist>` checks only that the literal `<key>com.apple.security.virtualization</key>`
  appears (`src/platform.rs`); the value is not inspected, and without the flag no check runs. `vesper-proxy` has no
  entitlements flag.

## Security-sensitive changes

Treat these as security-sensitive:
- entitlement changes,
- socket exposure changes,
- filesystem boundary changes,
- sandbox or process-lifecycle changes,
- proxy target parsing,
- any change that widens host ↔ System VM reachability.

## Required checks

At minimum, use:
```bash
make validate-entitlements
cargo test
python3 scripts/harness/check_structure.py --root .
python3 scripts/harness/janitor_scan.py --root .
```

`rust-tests.yml` runs `cargo test` and `make validate-entitlements` in CI for any `src/**` or `packaging/**` change.
Plus runtime evidence when the change affects real transport or VM lifecycle.

## Review rules

- Do not broaden entitlements without a concrete requirement and proof.
- Do not expose a new host-visible socket unless the design doc or execution plan says why.
- Do not accept undocumented compat drift as a silent temporary state.
- Keep boundary parsing explicit and deterministic.

## Links

- [`specs/design-v0.md`](specs/design-v0.md)
- [`process/baseline-governance.md`](process/baseline-governance.md)
- [`process/compat-exceptions.md`](process/compat-exceptions.md)
- [`RELIABILITY.md`](RELIABILITY.md)

## Published ports (GH #28, ADR 0005)

`vesper-proxy --publish-ports` (on by `make docker-up`) is the first host-visible TCP surface Vesper opens: one
listener on `127.0.0.1:<host port>` per TCP `-p` binding of a running container, closed when the container
stops or the proxy exits. Listeners are loopback only; a binding to any other host ip is recorded as
`unsupported` and not opened. Every connection travels the bridge to the guest gateway `vesper-port-relay`, which
only connects to guest loopback ports, so nothing in the guest becomes reachable that dockerd did not publish.
The live set is `run/vesper/ports.json`. Widening to non-loopback addresses is a human decision.

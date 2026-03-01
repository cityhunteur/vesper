# Vesper — Firecracker-compatible microVM runtime for macOS

## Repository Layout

```
cmd/           CLI binaries (vesperd, vesper-proxy, baselinectl, compatexctl, contractgen)
internal/      Core library packages
  api/         API server, routing, state machine, deterministic error responses
  baseline/    Baseline lock parsing, parity metadata generation & validation
  compat/      Compatibility exception registry and lifecycle policy checks
  platform/    macOS entitlements and platform feature detection
  proxy/       Unix/TCP/vsock request forwarding
  spec/        OpenAPI spec loading and contract enforcement
  buildinfo/   Build metadata
packaging/     Platform-specific packaging (macOS entitlements)
scripts/       Development and deployment scripts
testdata/      Test fixtures (golden contracts, baseline locks)
third_party/   Vendored specs (Firecracker OpenAPI)
docs/          Specifications, devlog, process docs
```

## Build & Test Commands

```bash
make build              # Build all binaries
make test               # go test ./...
make contract-test      # Golden contract check + spec/api tests
make contract-check     # Validate golden contract file
make contract-generate  # Regenerate golden contract
make validate-entitlements  # plutil -lint on macOS entitlements
make build-macos        # macOS-specific build
```

## Eval Dispatch Rules

When running evals for issues, map `phase:X` + `area:Y` labels to test commands:

| Phase | Area       | Commands |
|-------|------------|----------|
| 0     | api        | `make contract-test` + `go test ./internal/api/...` |
| 0     | platform   | `make validate-entitlements` + `go test ./internal/platform/...` |
| 0     | proxy      | `go test ./internal/proxy/...` |
| 0     | qa         | `make contract-test` + `go test ./internal/baseline/... ./internal/compat/...` |
| 0     | baseline   | `go test ./internal/baseline/...` |
| 0     | compat     | `go test ./internal/compat/...` |
| 1-5   | (any)      | `go test ./...` (full suite) |
| epic  | (any)      | `go test ./...` (full suite) |

## Baseline Pinning Requirements

- Every parity claim references a pinned Firecracker baseline revision
- Baseline lock file: `docs/specs/design-v0.baselines.lock.yaml`
- Metadata must include: backend, host-os, host-cpu, timestamp, git-sha
- CI validates lock parse + metadata generation via `baselinectl`

## Architectural Rules

1. **Single API endpoint**: `vesperd` exposes one Unix socket; all Firecracker API routes go through it
2. **No raw socket access**: Clients connect through `vesper-proxy`, never directly to vesperd
3. **NAT by default**: Guest networking uses NAT; no bridged mode unless explicitly configured
4. **State machine guards**: Pre-boot vs runtime actions are enforced; invalid transitions return deterministic errors
5. **Deterministic error envelopes**: All error responses include baseline revision and fault detail
6. **Parity is versioned**: Parity claims are against pinned baselines, not moving upstream HEAD
7. **Compat exceptions have expiry**: Every deviation from Firecracker behavior must have an expiry date and lifecycle policy

## Go Module

- Module: `github.com/cityhunteur/vesper`
- Go version: 1.23.0

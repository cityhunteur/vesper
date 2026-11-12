# scripts/licenses

## Overview

Keeps `THIRD_PARTY_NOTICES.md` at the repository root accurate. `third_party_notices.py` lists the Rust crates the
Vesper binaries link (`cargo tree -e normal`), the Go modules the shim links (`go list -deps` in `shim/`) and a
fixed table of vendored and build-time components, with their licences.

## Interface

- `python3 scripts/licenses/third_party_notices.py` (`make third-party-notices`) rewrites `THIRD_PARTY_NOTICES.md`.
- `--check` (`make third-party-notices-check`) exits `1` if the file differs from a fresh render; run it after any
  change to `Cargo.lock` or `shim/go.sum`.
- Needs `cargo` and `go` on `PATH` and the Go module cache populated (`go mod download` in `shim/`).
- Go licences are detected from each module's `LICENSE`/`COPYING` file; a module whose licence is not recognised is
  printed to stderr and listed as `UNKNOWN`.
- Python 3.11+ standard library only.

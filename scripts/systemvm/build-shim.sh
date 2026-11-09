#!/usr/bin/env bash
# Cross-compile containerd-shim-vesper-v2 (Go, static linux/arm64) into build/shim/ for the guest overlay.
# Usage: build-shim.sh   (needs the Go toolchain; version stamped from git)
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="${ROOT_DIR}/build/shim/containerd-shim-vesper-v2"
command -v go >/dev/null || { echo "build-shim: go toolchain not found (see docs/DEVELOPMENT.md)" >&2; exit 2; }
mkdir -p "${ROOT_DIR}/build/shim"
VERSION="$(git -C "${ROOT_DIR}" rev-parse --short HEAD 2>/dev/null || echo dev)"
cd "${ROOT_DIR}/shim"
GOOS=linux GOARCH=arm64 CGO_ENABLED=0 GOFLAGS=-mod=mod go build -trimpath \
  -ldflags="-s -w -buildid= -X github.com/cityhunteur/vesper/shim/internal/manager.Version=${VERSION}" \
  -o "${OUT}" ./cmd/containerd-shim-vesper-v2
echo "built ${OUT} ($(stat -f %z "${OUT}" 2>/dev/null || stat -c %s "${OUT}") bytes, version ${VERSION})"

GOOS=linux GOARCH=arm64 CGO_ENABLED=0 GOFLAGS=-mod=mod go build -trimpath \
  -ldflags="-s -w -buildid=" -o "${ROOT_DIR}/build/shim/vesper-console" ./cmd/vesper-console

#!/usr/bin/env bash
set -euo pipefail

# update-issue-checklists.sh
# Appends phase-specific Parity Eval Checklists to all 40 Vesper issues.
# Idempotent: skips issues that already have the checklist section.

REPO="${REPO:-cityhunteur/vesper}"
MARKER="## Parity Eval Checklist"

append_checklist() {
  local issue_num="$1"
  local checklist="$2"

  echo "Processing issue #${issue_num}..."

  # Fetch current body
  local body
  body=$(gh issue view "$issue_num" --repo "$REPO" --json body --jq '.body // ""')

  # Idempotency check
  if echo "$body" | grep -qF "$MARKER"; then
    echo "  Skipped: checklist already exists"
    return
  fi

  # Append checklist
  local new_body="${body}

---

${MARKER}

${checklist}"

  gh issue edit "$issue_num" --repo "$REPO" --body "$new_body"
  echo "  Done: checklist appended"
}

# ─── Phase 0 ────────────────────────────────────────────────

append_checklist 2 "$(cat <<'EOF'
**Phase 0 — API Surface (#2)**

- [ ] Golden contract file matches generated output (`make contract-check`)
- [ ] All Firecracker routes reachable and return correct status codes
- [ ] Error envelopes include baseline revision field
- [ ] State machine pre-boot/runtime guards enforced on every route
EOF
)"

append_checklist 8 "$(cat <<'EOF'
**Phase 0 — State Machine (#8)**

- [ ] Pre-boot actions blocked during runtime state
- [ ] Runtime actions blocked during pre-boot state
- [ ] Invalid state transitions return deterministic error payloads
- [ ] Error response format matches Firecracker error envelope spec
EOF
)"

append_checklist 9 "$(cat <<'EOF'
**Phase 0 — vesper-proxy (#9)**

- [ ] Unix socket round-trip echo succeeds
- [ ] Broken upstream connection triggers recovery/retry
- [ ] vsock target address parsing handles valid and invalid inputs
- [ ] Proxy forwards requests without modifying payload
EOF
)"

append_checklist 10 "$(cat <<'EOF'
**Phase 0 — Packaging (#10)**

- [ ] `plutil -lint` passes on all entitlement files
- [ ] `make build` succeeds on macOS
- [ ] Fresh-environment launch completes without errors
- [ ] Entitlements contain required capabilities (hypervisor, networking)
EOF
)"

append_checklist 11 "$(cat <<'EOF'
**Phase 0 — Contract Tests (#11)**

- [ ] Golden file matches generated contract (`make contract-check`)
- [ ] Endpoint coverage count matches expected route count
- [ ] State machine regression tests pass
- [ ] Contract generation is deterministic (repeated runs produce same output)
EOF
)"

append_checklist 37 "$(cat <<'EOF'
**Phase 0 — Baseline Lock (#37)**

- [ ] Lock file parses without errors (`baselinectl --mode validate`)
- [ ] Metadata generation includes all required keys (backend, host-os, host-cpu, timestamp, git-sha)
- [ ] CI guardrails reject lock files with missing or invalid fields
- [ ] Lock file format is forward-compatible
EOF
)"

append_checklist 38 "$(cat <<'EOF'
**Phase 0 — Compat Exceptions (#38)**

- [ ] Registry schema validates (`compatexctl --registry`)
- [ ] Expired exceptions are flagged
- [ ] Parity-GA zero-open gate blocks when open exceptions exist
- [ ] Exception lifecycle transitions are enforced (open → mitigated → closed)
EOF
)"

append_checklist 1 "$(cat <<'EOF'
**Phase 0 — Epic (#1)**

- [ ] All Phase 0 sub-issues (#2, #8, #9, #10, #11, #37, #38) pass their eval checklists
- [ ] `make contract-test` passes
- [ ] `make validate-entitlements` passes
- [ ] `cargo test` passes with zero failures
- [ ] Baseline lock and compat exception CI workflows green
EOF
)"

# ─── Phase 1 ────────────────────────────────────────────────

append_checklist 12 "$(cat <<'EOF'
**Phase 1 — System VM Boot (#12)**

- [ ] 200-cycle boot reliability test passes (no hangs or crashes)
- [ ] Kernel boots to healthy state with console output
- [ ] NAT networking is functional from guest
- [ ] Boot time within acceptable threshold
EOF
)"

append_checklist 13 "$(cat <<'EOF'
**Phase 1 — vsock (#13)**

- [ ] Handshake completes between host and guest
- [ ] Keepalive maintains connection under idle conditions
- [ ] Reconnect succeeds after connection drop
- [ ] Large payload transfer (>1MB) completes without corruption
EOF
)"

append_checklist 14 "$(cat <<'EOF'
**Phase 1 — containerd (#14)**

- [ ] containerd service starts successfully inside system VM
- [ ] Health check endpoint responds with healthy status
- [ ] Service restarts cleanly after simulated crash
- [ ] Logs capture startup and recovery events
EOF
)"

append_checklist 15 "$(cat <<'EOF'
**Phase 1 — Docker Socket (#15)**

- [ ] Docker socket path exists and is accessible
- [ ] Proxy forwards Docker API requests to containerd
- [ ] Raw socket access to vesperd is denied
- [ ] `docker info` returns valid response through proxy
EOF
)"

append_checklist 16 "$(cat <<'EOF'
**Phase 1 — Reliability (#16)**

- [ ] N-cycle boot stress test passes (configurable N, default 50)
- [ ] Socket smoke test validates vsock + Docker socket
- [ ] Machine-readable health report generated in JSON format
- [ ] No resource leaks detected across stress cycles
EOF
)"

append_checklist 3 "$(cat <<'EOF'
**Phase 1 — Epic (#3)**

- [ ] All Phase 1 sub-issues (#12, #13, #14, #15, #16) pass their eval checklists
- [ ] System VM boots reliably across 200 cycles
- [ ] vsock, containerd, and Docker socket integration verified
- [ ] `cargo test` passes with zero failures
EOF
)"

# ─── Phase 2 ────────────────────────────────────────────────

append_checklist 17 "$(cat <<'EOF'
**Phase 2 — Nested Virtualization (#17)**

- [ ] M3+ chip detection works correctly
- [ ] Fallback routing activates on unsupported hardware
- [ ] Diagnostic messages clearly indicate hardware capability
- [ ] No crash or hang on M1/M2 when nested virt unavailable
EOF
)"

append_checklist 18 "$(cat <<'EOF'
**Phase 2 — Firecracker Lifecycle (#18)**

- [ ] microVM launch completes successfully
- [ ] Teardown cleans up all resources (sockets, processes, mounts)
- [ ] Jailer isolation enforced (chroot, cgroup, seccomp)
- [ ] Cleanup runs even after abnormal termination
EOF
)"

append_checklist 19 "$(cat <<'EOF'
**Phase 2 — OCI-to-ext4 (#19)**

- [ ] Generated rootfs boots in microVM
- [ ] Rootfs generation is reproducible (same input → same output hash)
- [ ] Provenance metadata embedded in image (source OCI digest, timestamp)
- [ ] Invalid OCI images produce clear error messages
EOF
)"

append_checklist 20 "$(cat <<'EOF'
**Phase 2 — microVM-per-Container (#20)**

- [ ] Container lifecycle maps 1:1 to microVM lifecycle
- [ ] Isolation verified between concurrent microVMs
- [ ] Multiple concurrent VMs run without resource conflicts
- [ ] Container stop triggers microVM teardown
EOF
)"

append_checklist 21 "$(cat <<'EOF'
**Phase 2 — Parity Baseline (#21)**

- [ ] Upstream Firecracker integration suite passes against parity backend
- [ ] Performance baselines captured and within acceptable range
- [ ] Compat exception summary generated for any deviations
- [ ] Baseline revision pinned and documented
EOF
)"

append_checklist 4 "$(cat <<'EOF'
**Phase 2 — Epic (#4)**

- [ ] All Phase 2 sub-issues (#17, #18, #19, #20, #21) pass their eval checklists
- [ ] Firecracker microVM lifecycle end-to-end verified
- [ ] OCI-to-ext4 pipeline produces bootable rootfs
- [ ] `cargo test` passes with zero failures
EOF
)"

# ─── Phase 3 ────────────────────────────────────────────────

append_checklist 22 "$(cat <<'EOF'
**Phase 3 — Task RPCs (#22)**

- [ ] Create, Start, Kill, Delete, State, Wait RPCs all functional
- [ ] End-to-end task lifecycle (create → start → wait → delete) succeeds
- [ ] Invalid RPC sequences return appropriate errors
- [ ] Concurrent task operations handled correctly
EOF
)"

append_checklist 23 "$(cat <<'EOF'
**Phase 3 — Exec/Pty (#23)**

- [ ] TTY mode: interactive shell works with proper terminal handling
- [ ] Non-TTY mode: stdin/stdout/stderr properly separated
- [ ] Exit codes propagated correctly from guest process
- [ ] Signal propagation (SIGTERM, SIGKILL) reaches guest process
EOF
)"

append_checklist 24 "$(cat <<'EOF'
**Phase 3 — OCI Unpack (#24)**

- [ ] OCI bundle validation catches malformed bundles
- [ ] Block-first rootfs strategy produces bootable filesystem
- [ ] Failure diagnostics include actionable error messages
- [ ] Layer deduplication works for multi-layer images
EOF
)"

append_checklist 25 "$(cat <<'EOF'
**Phase 3 — Shim Restart (#25)**

- [ ] Shim reconnects after containerd restart without losing containers
- [ ] State reconciliation restores accurate container status
- [ ] In-flight operations resume or fail gracefully
- [ ] No orphaned microVMs after shim reconnect
EOF
)"

append_checklist 26 "$(cat <<'EOF'
**Phase 3 — ctr Conformance (#26)**

- [ ] `ctr run` creates and starts container
- [ ] `ctr exec` attaches to running container
- [ ] `ctr kill` + `ctr metrics` return expected data
- [ ] CI integration runs full ctr conformance suite
EOF
)"

append_checklist 5 "$(cat <<'EOF'
**Phase 3 — Epic (#5)**

- [ ] All Phase 3 sub-issues (#22, #23, #24, #25, #26) pass their eval checklists
- [ ] containerd shim v2 protocol fully implemented
- [ ] ctr conformance suite passes
- [ ] `cargo test` passes with zero failures
EOF
)"

# ─── Phase 4 ────────────────────────────────────────────────

append_checklist 27 "$(cat <<'EOF'
**Phase 4 — Docker API (#27)**

- [ ] Core Docker CLI commands work (run, exec, ps, stop, rm, images, pull)
- [ ] Auth and context management functional
- [ ] Ingress boundary enforced (only Docker API, no direct containerd)
- [ ] Unsupported commands return clear error messages
EOF
)"

append_checklist 28 "$(cat <<'EOF'
**Phase 4 — Port Publishing (#28)**

- [ ] `-p` published ports reachable on localhost
- [ ] Port conflict detection and clear error reporting
- [ ] Port mappings cleaned up on container stop
- [ ] Multiple port mappings per container work correctly
EOF
)"

append_checklist 29 "$(cat <<'EOF'
**Phase 4 — Bind Mounts (#29)**

- [ ] virtio-fs mounts host directories into guest
- [ ] uid/gid mapping preserves ownership semantics
- [ ] inotify/watch events propagate across mount boundary
- [ ] Read-only mounts enforced correctly
EOF
)"

append_checklist 30 "$(cat <<'EOF'
**Phase 4 — DNS/Network (#30)**

- [ ] Container DNS resolver returns correct records
- [ ] Service discovery between compose containers works
- [ ] Compose network aliases resolve correctly
- [ ] External DNS queries forwarded properly
EOF
)"

append_checklist 31 "$(cat <<'EOF'
**Phase 4 — Workflow Suite (#31)**

- [ ] `docker compose up` starts multi-service application
- [ ] `docker build` produces images from Dockerfile
- [ ] `docker logs -f` streams logs in real-time
- [ ] `docker exec -it` provides interactive terminal
- [ ] Stop signals propagate correctly to containers
- [ ] `--platform linux/amd64` cross-compilation works
EOF
)"

append_checklist 39 "$(cat <<'EOF'
**Phase 4 — Cross-Architecture (#39)**

- [ ] binfmt_misc + qemu-user registered for target architectures
- [ ] Architecture metadata surfaced in image/container inspect
- [ ] Unsupported architecture combinations return clear errors
- [ ] Cross-arch containers run with acceptable performance
EOF
)"

append_checklist 6 "$(cat <<'EOF'
**Phase 4 — Epic (#6)**

- [ ] All Phase 4 sub-issues (#27, #28, #29, #30, #31, #39) pass their eval checklists
- [ ] Docker CLI workflow end-to-end verified
- [ ] Compose up/down with networking and volumes works
- [ ] `cargo test` passes with zero failures
EOF
)"

# ─── Phase 5 ────────────────────────────────────────────────

append_checklist 32 "$(cat <<'EOF'
**Phase 5 — Native Boot (#32)**

- [ ] Guest boots via HVF backend with console output
- [ ] Boot diagnostics capture timing and stage information
- [ ] Boot is reproducible across runs
- [ ] Failure modes produce actionable diagnostics
EOF
)"

append_checklist 33 "$(cat <<'EOF'
**Phase 5 — Virtio Devices (#33)**

- [ ] Device enumeration lists all configured virtio devices
- [ ] Feature bit negotiation matches Firecracker behavior
- [ ] Runtime device updates (e.g., balloon resize) applied correctly
- [ ] Unknown device types handled gracefully
EOF
)"

append_checklist 34 "$(cat <<'EOF'
**Phase 5 — MMDS + Rate Limit (#34)**

- [ ] MMDS GET requests return configured metadata
- [ ] Token-bucket rate limiting enforces configured limits
- [ ] Throttled requests receive appropriate 429/retry-after responses
- [ ] Rate limit behavior matches Firecracker MMDS spec
EOF
)"

append_checklist 35 "$(cat <<'EOF'
**Phase 5 — Snapshots (#35)**

- [ ] Full snapshot roundtrip (create → restore) succeeds
- [ ] Diff snapshot captures only changed state
- [ ] Cross-process restore works (snapshot on process A, restore on B)
- [ ] Corrupted snapshot files detected and rejected with clear error
EOF
)"

append_checklist 36 "$(cat <<'EOF'
**Phase 5 — Parity Harness (#36)**

- [ ] Differential A-vs-B testing produces comparison report
- [ ] Chaos/failure injection tests run without crashing harness
- [ ] GA sign-off checklist generated from parity results
- [ ] All compat exceptions accounted for in final report
EOF
)"

append_checklist 40 "$(cat <<'EOF'
**Phase 5 — Native Networking (#40)**

- [ ] System VM gateway integration routes traffic correctly
- [ ] Network policy enforcement blocks unauthorized traffic
- [ ] Isolation between microVM networks verified
- [ ] DNS and NAT function through native networking stack
EOF
)"

append_checklist 7 "$(cat <<'EOF'
**Phase 5 — Epic (#7)**

- [ ] All Phase 5 sub-issues (#32, #33, #34, #35, #36, #40) pass their eval checklists
- [ ] Native HVF backend boots and runs microVMs
- [ ] Parity harness produces GA sign-off report
- [ ] Zero open compat exceptions (`compatexctl --require-zero-open`)
- [ ] `cargo test` passes with zero failures
EOF
)"

echo ""
echo "All issues processed."

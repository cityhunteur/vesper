## Code Review Council Report

**⛔ Do not merge** · 2 blockers
🔴 0 Critical · 🟠 2 High · 🟡 8 Medium · 🔵 10 Low · ▲ 0 coverage gaps
`systemvm/overlay/usr/local/bin/vesper-microvm` +70 more (71 files, +1396 −74) · POSIX shell, Bash, Go, Rust, Python
Agents: Codex · Claude · Antigravity · Practices (11) · Judge

A remapped container can still reach root: it can pre-create a state directory that root tools later use (#1). Upgraded data disks also keep their old world-readable private files, and some confinement checks pass without testing anything. The mapped-boot ordering, the opposite-mode refusal and the plugin's fail-closed handling are sound, and no reviewer proved an escape through them.

### Next steps
1. **Fix #1** before merge → `grep -n 'mkdir -p' systemvm/overlay/usr/local/bin/vesper-microvm systemvm/overlay/usr/local/bin/vesper-memadmit`, then the runtime probe in #1 on a disposable VM.
2. **Fix #2** before merge → `sed -n '52,58p' systemvm/overlay/usr/local/bin/vesper-rootfs; sed -n '86,92p' shim/internal/backend/backend.go`, then add a committed regression test for upgrades.
3. **Resolve the unverified** #9 and #10 → run the checks given in each block.
4. **Probe areas no lens reached**: the mode when there is no data disk (tmpfs, so no record), other directories that dockerd can pre-create besides those in #1, and whether socat really sets `SOCAT_PEERADDR` for `VSOCK-LISTEN`.
5. **Follow up** after merge: #3–#8, #11–#20.

### 🟠 High

**#1 · A container can pre-create, and then own, directories that root tools adopt with `mkdir -p`**
`systemvm/overlay/usr/local/bin/vesper-microvm:269` · Correctness / Security · Claude F1 · ✓ verified

`checkCreate` never inspects `Binds`. As a result, `docker run -v /var/lib/vesper/microvm:/m` lets dockerd create the missing source directory owned by uid 100000. Nothing creates `/var/lib/vesper/microvm`, `/srv/jailer` or `/run/vesper/mem` at boot. `prepare()` skips the kernel when `$MVM/Image` exists and accepts a matching `initrd.sha256`, so every later microVM boots files the container planted. Separately, `vesper-memadmit` runs `with_lock`, which uses chmod but never chown, then writes as root through a symlink the container planted. The judge confirmed this from the code. The claim about moby's bind-source creation comes from recall and was not run.
- **Verify:** on a fresh boot before any microVM launch, run `docker run --rm -v /var/lib/vesper/microvm:/m alpine:3.22 sh -c 'stat -c %u /m; touch /m/Image && echo GOT'`. Then run `systemvmctl control microvm prepare` and check whether it uses the planted `Image`.
- **Fix:** in `vesper-services`, before dockerd starts, create `/var/lib/vesper/microvm`, `/srv/jailer/firecracker`, `/run/vesper/mem`, `/var/log/vesperd` and the plugin directories as root-owned 0700. Do the same for the stores when there is no data disk. Make each tool refuse a state directory that is not owned by uid 0, and add this probe to `confine-suite.sh`.

**#2 · Existing private files keep their old readable modes after an upgrade**
`systemvm/overlay/usr/local/bin/vesper-data:63` · Security / Privacy · Codex F1, Antigravity F1, Practice rules F2, Practice security-design-1 F2, Practice security-design-2 F2, Practice security-operations-1 F1, Practice security-operations-2 F2, Practice sre-foundations-1 F1, Practice sre-foundations-2 F1, Practice sre-integrity F1, Practice sre-organisation F1, Practice sre-runtime F1 · RULE.03, RULE.04, SR05.01, SR12.06, SRE17.06, SRE26.14 +29 more · ✓ verified

The judge rated this High; most reviewers rated it Critical. `vesper-data` changes only directory modes. `vesper-rootfs:52-56` and `backend.go:88-91` reuse a cached image without running chmod. The sweep keeps store snapshots that the base revision wrote as 0444. dockerd binds a single file as root, so a known `old.mem` or `<id16>.ext4` stays readable by uid 100000 even though its directory is now private. Codex's failing `TestGH194LegacyCacheIsPrivate` is not in the tree.
- **Verify:** on a disposable fixture upgraded from a pre-change disk, run `docker run --rm -v /var/lib/vesper/state/vesperd/store/OLD.mem:/x:ro alpine:3.22 head -c 16 /x`. The read must be refused.
- **Fix:** before dockerd starts, migrate existing regular files in the owned stores to their intended modes. Snapshots become `root:vesperd-jail 0440`; images and records become 0600. Fail closed if the migration fails, enforce file modes on cache hits in both `vesper-rootfs` and the shim, and commit a regression test for upgrades.

### 🟡 Medium

**#3 · An interrupted upstream run leaves an unmapped launch config that `docker-up` accepts**
`scripts/systemvm/upstream-suite/run.sh:250` · Rollout and migration · Claude F2, Practice rules F1, Practice security-design-2 F1, Practice security-operations-1 F3, Practice security-operations-2 F3, Practice sre-foundations-1 F3, Practice sre-foundations-2 F3, Practice sre-organisation F3, Practice sre-runtime F2, Practice sre-state F1 · RULE.02, RULE.09, SR08.09, SR14.15, SRE25.10, SRE27.08 +20 more · ✓ verified

The judge rated this Medium; the practice reviewers rated it Critical or High. The judge gave Medium because the failure needs a SIGKILL, a crash or a power loss, and `docker-up` prints a warning. `mv "$CFG.new" "$CFG"` persists the unmapped token and the suite disk, and only the EXIT trap restores the config. `docker-up.sh:43` accepts `want == got == unmapped`.
- **Verify:** `sed -n '233,251p' scripts/systemvm/upstream-suite/run.sh; sed -n '24,59p' scripts/systemvm/docker-up.sh`. When both sides say `unmapped`, no branch refuses.
- **Fix:** pass the suite a separate launch config so the normal config is never changed. Make `docker-up` refuse `unmapped` unless an opt-in set only by the runner is present (for example `VESPER_ALLOW_UNMAPPED=1`), or while `launch-config.upstream-saved.json` exists and no runner holds the lock.

**#4 · The vsock confinement step passes when no probe runs, and the peer check is never exercised**
`scripts/systemvm/confine-suite.sh:118` · Testing · Codex F2, Claude F3, Practice rules F4, Practice security-design-1 F4, Practice security-design-2 F3, Practice security-operations-1 F5, Practice security-operations-2 F4, Practice sre-foundations-1 F4, Practice sre-foundations-2 F5, Practice sre-runtime F4 · RULE.11, SR05.08, SR13.02, SRE02.06, SREE.04 +12 more · ✓ verified

The step passes on any output without `VESPER-AGENT|PONG|HTTP/1` and never checks the exit status of `docker run`. The committed report shows only transport errors, so no connection reached `vsock_host_only`. That check also returns 0 when `SOCAT_PEERADDR` is unset, and nothing tests it.
- **Verify:** `bash -c 'out="docker: container failed to start"; if printf "%s" "$out" | grep -q "VESPER-AGENT\|PONG\|HTTP/1"; then echo FAIL; else echo PASS; fi'` prints PASS. `grep -rn SOCAT_PEERADDR scripts tests src` prints nothing.
- **Fix:** require the probe container to succeed, and require exactly six results that are recognised denials. Add a hermetic test of `vesper-port-relay-conn` with `SOCAT_PEERADDR=3` (expect `ERR`, rc 1) and with `=2`. Fail closed when `SOCAT_PID` is set but `SOCAT_PEERADDR` is not.

**#5 · A new rootfs image stays readable until `mke2fs` has finished writing it**
`systemvm/overlay/usr/local/bin/vesper-rootfs:79` · Privacy · Practice rules F3, Practice security-operations-1 F2, Practice sre-foundations-2 F2 · RULE.03, SR12.10, SRE17.10 +3 more · ✓ verified

The judge rated this Medium; the reviewers rated it Critical. Exploiting it means winning a race on a predictable path. The script runs under `umask 022` and applies `chmod 0600` only at line 83, after `mke2fs`. `backend.go:161` has the same ordering.
- **Verify:** `sed -n '75,85p' systemvm/overlay/usr/local/bin/vesper-rootfs; sed -n '155,165p' shim/internal/backend/backend.go`
- **Fix:** create the output file with mode 0600 before `mke2fs` runs, or apply `umask 077` to the image file only and leave the unpacked tree's modes as they are.

**#6 · The upstream suite's data disk is created without owner-only mode**
`scripts/systemvm/upstream-suite/run.sh:229` · Privacy · Practice security-design-1 F3, Practice sre-integrity F2 · SR01.01, SR06.16, SRE26.03 · ✓ verified

The judge rated this Medium; the reviewers rated it Critical. `dd` creates the disk under the caller's umask. `generate-launch-config.sh:30` runs `chmod 600` on its disk; this script does not.
- **Verify:** `sed -n '228,232p' scripts/systemvm/upstream-suite/run.sh; sed -n '25,31p' scripts/systemvm/generate-launch-config.sh`
- **Fix:** create the disk under `umask 077`, and enforce 0600 on an existing suite disk before the VM boots with it.

**#7 · The share-ownership regression (TD-062) is hidden by `sleep 1.5`, not fixed**
`scripts/systemvm/binds-suite.sh:36` · API and compatibility · Practice security-design-1 F5, Practice security-operations-1 F6, Practice sre-foundations-1 F5, Practice sre-foundations-2 F4 · SR04.10, SR13.03, SRE02.06, SRE17.03 +5 more · ✓ verified

The judge rated this Medium because the gap is documented; the reviewers rated it High. Right after a root-side lookup, or after the host-edit replay, a container sees `nobody`, so `git`, `chmod` and `touch` fail. Claude adds that a single refresh after `runc create` does not survive the root `touch` in `vesper-share touch`.
- **Verify:** on a disposable VM, `docker run --rm -v "$SHARE:/src" alpine:3.22 sh -c 'stat -c "%u:%g" /src; chmod "$(stat -c %a /src)" /src'` right after a root-side lookup.
- **Fix:** keep a regression test that runs at once, with no sleep. Fix both producers: the startup lookup, and the replay `touch` (run it as uid 100000).

**#8 · The share-server confinement check has no working positive control**
`scripts/systemvm/confine-suite.sh:99` · Testing · Practice sre-foundations-2 F5, Practice sre-integrity F3, Practice sre-organisation F4 · SRE26.18, SRE32.03, SRE12.02 +3 more · ✓ verified

`test-share-reach` probes root's own namespace (`vesper-microvm:206`), not the listener's. The committed report records `REACH root closed` and still passes, which contradicts the comment at line 99. `test-share-listen` prints `LISTEN` without checking that socat is still running.
- **Verify:** `rg -n 'REACH root|share_server_closed_to_containers' run/GH-194/m5-confine-report.json`
- **Fix:** before accepting any denials, require one successful connection from inside the target namespace, and check that the listener is alive.

**#9 · Container and dockerd logs may be world-readable until line 68 runs, and again after a respawn**
`systemvm/overlay/etc/init.d/vesper-services:55` · Reliability · Antigravity F5 · ? unverified

The `chmod 0600` runs only after `rc-service docker start`. The judge found that `LOGPROXY_CHMOD`/`log_mode` are written to `conf.d` beforehand and persist, so this race exists only if the packages ignore those settings.
- **Verify:** on a disposable VM, run `KILL docker`, let it respawn, then `stat -c %a /var/log/docker.log /var/log/containerd.log`.
- **Fix:** create the logs as 0600 before starting the services, or send them through `vesper-boundlog`.

**#10 · `HEALTH` runs blocking `ctr`, `docker version` and `find` on every poll**
`systemvm/overlay/usr/local/bin/vesper-agent:65` · Scalability and performance · Antigravity F7 · ? unverified

The judge noted that 2 s + 5 s stays under the 10 s `io_timeout`, that the loop breaks early, and that nobody measured it.
- **Verify:** time `systemvmctl control health` 20 times with `vesper-authz` stopped and a large `/var/lib/docker`.
- **Fix:** record `legacy_store` once in `prepare`, and skip `docker version` when the socket is absent.

### 🔵 Low

- **#11** `scripts/systemvm/upstream-suite/run.sh:35` · Security · Practice security-design-1 F1, Practice security-operations-1 F4, Practice security-operations-2 F1, Practice sre-foundations-1 F2, Practice sre-foundations-2 F6, Practice sre-organisation F2, Practice sre-runtime F3 · SR05.13, SR14.01, SRE32.04 · ✓ verified — the claim "outside every share" for the saved config is unchecked. The judge rated this Low (reviewers said Critical): a read-write repo share already lets a container edit the Makefile → add a canonical ancestor check, or fix the comment.
- **#12** `systemvm/overlay/etc/init.d/vesper-services` · API and compatibility · Claude F4 · ✓ verified — a refused boot surfaces as a 65 s `probe-failed`, and `docker-up`'s `refused` branch can never run → make `up.sh` grep `console.log` for `CONTAINERS refused` when the probe fails.
- **#13** `scripts/systemvm/confine-requests.py:80` · Testing · Claude F7, Antigravity F8 · ✓ verified — it accepts router 301/404 as plugin refusals, step 3 has no probe that must succeed, and a 204 response can stall the read for 20 s. Antigravity rated it Medium → require 403 with `vesper:` in the body, send `Connection: close`, and add one probe that must succeed.
- **#14** `scripts/systemvm/confine-suite.sh:72` · Testing · Antigravity F9 · ✓ verified — `p()` reports ENOENT the same as EACCES (Antigravity rated it Medium) → check each fixture exists first, and match the denial text.
- **#15** `systemvm/overlay/usr/local/bin/vesper-boundlog:20` · Reliability · Antigravity F2 · ✓ verified — the byte counter restarts at 0 on every respawn, so the log never rotates (Antigravity rated it High) → start `n` from the current file size.
- **#16** `systemvm/overlay/usr/local/bin/vesper-ctr:37` · Design and maintainability · Claude F5 · ✓ verified — root writes fixed names in `/tmp` that a container can reach → use `mktemp -d` under `/run/vesper/ctr`.
- **#17** `systemvm/overlay/usr/local/bin/vesper-containers:36` · Design and maintainability · Claude F6 · ✓ verified — `refuse()` deletes `daemon.json`, so a later dockerd start would run unmapped → write a config that cannot serve instead.
- **#18** `systemvm/overlay/usr/local/bin/vesper-vesperd:160` · Security · Antigravity F4 · ✓ verified — the jail parent directories are 0755 (this predates the change; Antigravity rated it Medium) → use 0711.
- **#19** `scripts/systemvm/upstream-suite/run.sh:238` · Reliability · Antigravity F6 · ✓ verified — `vesper_data=` is not added when the base config has `VESPER_DATA_DISK=none` (Antigravity rated it Medium) → derive the device token as `data_disk.rs` does.
- **#20** `systemvm/overlay/etc/init.d/vesper-docker-relay:11` · Security · Antigravity F10 · ✓ verified — the relay has no peer check and no `max-children` cap (already documented in SECURITY.md:316) → put a `vsock_host_only` wrapper in front of it.

### Practices
770 rules and recommendations checked · 185 satisfied · 117 gaps · 111 assurance gaps · 353 n/a · 4 exceptions · 0 not evaluated

| Area | Human reviewer | Sat | Gap | Assur | N/A | Exc | Not eval |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Purpose and risk | Tech lead | 33 | 11 | 10 | 29 | 1 | 0 |
| Identity and data | Security and privacy | 17 | 8 | 6 | 10 | 1 | 0 |
| Interfaces and state | Service owner | 30 | 8 | 9 | 49 | 1 | 0 |
| Capacity and failure | SRE | 17 | 10 | 24 | 52 | 1 | 0 |
| Isolation and recovery | SRE and security | 9 | 21 | 17 | 45 | 0 | 0 |
| Verification | Test owner | 6 | 15 | 7 | 12 | 0 | 0 |
| Delivery | Release owner | 30 | 23 | 21 | 13 | 0 | 0 |
| Operation | On-call lead | 18 | 7 | 7 | 51 | 0 | 0 |
| Organisation | Engineering manager | 25 | 14 | 10 | 92 | 0 | 0 |

Sat = satisfied with evidence · Gap = defect, see the finding it cites · Assur = evidence missing, no proven defect · Exc = recorded exception · Not eval = no evidence, reviewer failed, or a gap claim the judge found false

**Purpose and risk** → Tech lead
- Gap `RULE.02` · Practice rules F1 · ✓ verified — An interrupted upstream run leaves ordinary startup unmapped
- Gap `SR01.01` · Practice security-design-1 F2 · ✓ verified — Upgrading leaves existing private state world-readable
- Gap `SR01.06` · Practice security-design-1 F4 · ✓ verified — The vsock confinement step passes when its probe never runs
- Gap `SR04.10` · Practice security-design-1 F5 · ✓ verified — Immediate ownership-sensitive operations on shares now fail
- Gap `SREP.02` · Practice sre-foundations-1 F1 · ✓ verified — Existing private state keeps its old readable permissions
- Gap `SRE01.07` · Practice sre-foundations-1 F3 · ✓ verified — An interrupted upstream run leaves the unmapped configuration reusable
- Gap `SRE01.09` · Practice sre-foundations-1 F5 · ✓ verified — The bind regression is delayed past, rather than resolved
- Gap `SRE02.06` · Practice sre-foundations-1 F4 · ✓ verified — The vsock confinement check passes when the probe cannot run
- Gap `SRE03.03` · Practice sre-foundations-1 F1 · ✓ verified — Existing private state keeps its old readable permissions
- Gap `SRE04.10` · Practice sre-foundations-1 F4 · ✓ verified — The vsock confinement check passes when the probe cannot run
- Gap `SRE18.12` · Practice sre-foundations-2 F4 · ✓ verified — Share ownership regression breaks immediate container operations
- Assurance `SR01.02` — Mode mismatch is fail-closed, but abrupt host interruption and subsequent recovery are not demonstrated by th…
- Assurance `SR01.08` — ExecPlan describes rollback, but evidence does not establish recovery after an untrappable runner interruptio…
- Assurance `SR02.07` — Automated predictable-mistake checks exist, but isolation of recovery configuration is incomplete; see F1.
- Assurance `SR04.03` — Disk/cache growth and retained stores are documented, but this delta supplies no partial-restore or recovery-…
- Assurance `SR04.07` — Refused disks retain the documented tmpfs fallback; mode and persistence recovery under partial mount failure…
- Assurance `SRA.01` — ADR 0010 catalogues confinement risks and mitigations, but no likelihood/impact ranking is evidenced. No code…
- Assurance `SRE01.06` — Plugin recovery evidence exists; new upstream-boot recovery was explicitly not exercised.
- Assurance `SRE02.03` — Mode records use ordinary file writes; crash-consistency and partial-write recovery evidence is absent.
- Assurance `SRE04.04` — The 50-call latency check reports only a mean; tail behaviour is not established.
- Assurance `SRE04.06` — The 200 ms probe budget has no recorded rationale or stated workload validity.
- Exception `SRE03.05` — ADR 0010 explicitly distinguishes normal mapped operation from the upstream suite’s unmapped exception.

**Identity and data** → Security and privacy
- Gap `RULE.03` · Practice rules F1 · ✓ verified — An interrupted upstream run leaves ordinary startup unmapped
- Gap `RULE.04` · Practice rules F2 · ✓ verified — Existing persistent files retain their readable permissions
- Gap `SR03.02` · Practice security-design-1 F1 · ✓ verified — A writable ancestor share exposes the saved boot configuration
- Gap `SR05.01` · Practice security-design-1 F1 · ✓ verified — A writable ancestor share exposes the saved boot configuration
- Gap `SR05.08` · Practice security-design-1 F4 · ✓ verified — The vsock confinement step passes when its probe never runs
- Gap `SR05.13` · Practice security-design-1 F1 · ✓ verified — A writable ancestor share exposes the saved boot configuration
- Gap `SR11.07` · Practice security-design-2 F3 · ✓ verified — The vsock confinement check passes when its probe never executes
- Gap `SR11.09` · Practice security-design-2 F1 · ✓ verified — Interrupted upstream runs leave normal startup unmapped
- Assurance `SR03.05` — Refusals are logged and policy tests exist; successful sensitive operations lack comparable audit evidence.
- Assurance `SR05.05` — Refusal logs are bounded diagnostics; no complete tamper-resistant action audit is demonstrated or claimed.
- Assurance `SR05.12` — Configuration is generated from reviewed code and health-reported; staged deployment evidence is absent.
- Assurance `SR05.16` — Unit tests and runtime status inspect policy operation, but protected distribution depends on the unverified…
- Assurance `SR11.04` — Matching JSON semantics and noncanonical-path rejection are implemented; exhaustive agreement with dockerd ac…
- Assurance `SR11.06` — The ADR records Docker/kernel versions and runtime probes; comprehensive hardening of third-party runtime pat…
- Exception `SR03.07` — ADR 0010 deliberately rejects privileged/host-namespace requests, BuildKit and unmeasured routes while retain…

**Interfaces and state** → Service owner
- Gap `SR06.02` · Practice security-design-1 F4 · ✓ verified — The vsock confinement step passes when its probe never runs
- Gap `SR06.03` · Practice security-design-1 F5 · ✓ verified — Immediate ownership-sensitive operations on shares now fail
- Gap `SR06.12` · Practice security-design-1 F1 · ✓ verified — A writable ancestor share exposes the saved boot configuration
- Gap `SR06.16` · Practice security-design-1 F2 · ✓ verified — Upgrading leaves existing private state world-readable
- Gap `SR12.06` · Practice security-operations-1 F1 · ✓ verified — Existing snapshot files remain world-readable after upgrade
- Gap `SR12.10` · Practice security-operations-1 F2 · ✓ verified — Root-drive files become private only after their contents are written
- Gap `SR12.11` · Practice security-operations-1 F1 · ✓ verified — Existing snapshot files remain world-readable after upgrade
- Gap `SRE25.10` · Practice sre-state F1 · ✓ verified — Interrupted upstream runs leave unmapped mode in the normal launch configuration
- Assurance `SR06.11` — The local owner/workload authority split is documented; privileged Docker archive and other daemon-mediated o…
- Assurance `SR12.03` — Peer rejection precedes handler work; successful non-host connections exercising that rejection are not evide…
- Assurance `SR12.04` — vesper-services orders dependencies and bounds the socket wait, but socket existence does not establish plugi…
- Assurance `SR12.09` — Go vet runs in shim CI; no equivalent fast checks for the new confinement shell policy were found.
- Assurance `SR12.17` — Read shim CI’s vet/test commands; no race-detector or sanitizer execution is evidenced for this change.
- Assurance `SRE09.06` — Milestones and artifacts make the change reviewable; interrupted exception recovery and upstream execution re…
- Assurance `SRE24.02` — Reapers log cleanup outcomes and expose manual inspection/reaping, but the inspected paths provide no indepen…
- Assurance `SRE25.02` — The runner checks collected test count, disk space and elapsed time, but the plan explicitly says the new unm…
- Assurance `SRE25.06` — Upstream stage logs and `runner.log` preserve progress, but the new confinement runner writes its JSON report…
- Exception `SRE09.01` — ADR 0010 permits an unmapped upstream-test boot with its own disk and explicitly weaker guarantees.

**Capacity and failure** → SRE
- Gap `SR08.01` · Practice security-design-2 F1 · ✓ verified — Interrupted upstream runs leave normal startup unmapped
- Gap `SR08.03` · Practice security-design-2 F2 · ✓ verified — Persisted private files retain their old readable permissions
- Gap `SR08.09` · Practice security-design-2 F1 · ✓ verified — Interrupted upstream runs leave normal startup unmapped
- Gap `SR08.11` · Practice security-design-2 F2 · ✓ verified — Persisted private files retain their old readable permissions
- Gap `SR08.12` · Practice security-design-2 F3 · ✓ verified — The vsock confinement check passes when its probe never executes
- Gap `SR08.23` · Practice security-design-2 F3 · ✓ verified — The vsock confinement check passes when its probe never executes
- Gap `SR08.24` · Practice security-design-2 F3 · ✓ verified — The vsock confinement check passes when its probe never executes
- Gap `SRE22.11` · Practice sre-runtime F2 · ✓ verified — An interrupted upstream run leaves ordinary startup configured as unmapped
- Gap `SREB.01` · Practice sre-runtime F3 · ✓ verified — Saved configuration is not necessarily outside inherited writable shares
- Gap `SREB.02` · Practice sre-runtime F1 · ✓ verified — Existing persistent files remain readable through single-file binds
- Assurance `RULE.07` — The committed report demonstrates one plugin-loss/recovery case. It does not establish behaviour under concur…
- Assurance `SR08.02` — Read authz refusal logging, bounded logs and the ADR threat model; probing detection or anomaly handling is n…
- Assurance `SR08.08` — HEALTH exposes mode and plugin state; remaining capacity and defence engagement are not measured.
- Assurance `SR08.21` — Authz loss/recovery evidence exists; long-term policy drift, dependency growth and continuing exercise are no…
- Assurance `SR10.01` — Bounded logs and plugin-message limits address resource costs; sustained plugin saturation evidence is absent.
- Assurance `SR10.06` — The suite records call latency, but no correlation with CPU, memory or network saturation is provided.
- Assurance `SR10.14` — Bounded refusal logging is present; sustained availability and resource costs under malicious Docker traffic…
- Assurance `SRE20.03` — vesper-services stops Docker before the plugin; graceful treatment of active requests during this transition…
- Assurance `SRE20.05` — The plugin becomes a per-request dependency; connection reuse and connection-count behaviour are not establis…
- Assurance `SRE21.01` — Refusal logs are bounded, but useful throughput during plugin saturation is not demonstrated.
- Assurance `SRE21.02` — The report measures Docker version latency, not plugin CPU, memory or saturation capacity.
- Assurance `SRE21.07` — Message and log sizes are bounded; process-wide plugin saturation protection is not demonstrated.
- Assurance `SRE21.11` — No connection-churn or concurrent startup measurement accompanies the new plugin dependency.
- Assurance `SRE21.12` — Fifty sequential version calls establish neither an overload limit nor recovery after overload.
- Assurance `SRE22.01` — Plugin loss and recovery are exercised, but slow-plugin/in-flight-request feedback is not measured.
- Assurance `SRE22.02` — Committed suites cover functionality and plugin outage, not realistic saturation.
- Assurance `SRE22.03` — Plugin message size is bounded; aggregate HTTP concurrency and queue behaviour lack evidence.
- Assurance `SRE22.07` — No evidence establishes cancellation of plugin work after an upstream request is abandoned.
- Assurance `SRE22.08` — The latency probe reports an average; slow-tail and stuck-plugin behaviour remain unmeasured.
- Assurance `SRE22.12` — Runtime reports do not establish plugin capacity within a committed resource allocation.
- Assurance `SRE22.13` — No ramp, burst, failure-limit and recovery experiment is supplied.
- Assurance `SRE22.15` — Plugin absence is tested; a dependency that accepts requests but never answers is not.
- Assurance `SRE22.19` — Fail-closed plugin loss is evidenced; saturation and slow-dependency consequences of the added dependency rem…
- Assurance `SREB.07` — Bounded messages/logs help, but overload admission, deadlines and recovery are not demonstrated.
- Exception `SR08.13` — ADR 0010 explicitly accepts one shared container mapping and no isolation between containers.

**Isolation and recovery** → SRE and security
- Gap `RULE.09` · Practice rules F1 · ✓ verified — An interrupted upstream run leaves ordinary startup unmapped
- Gap `SR09.01` · Practice security-design-2 F1 · ✓ verified — Interrupted upstream runs leave normal startup unmapped
- Gap `SR09.13` · Practice security-design-2 F1 · ✓ verified — Interrupted upstream runs leave normal startup unmapped
- Gap `SR09.14` · Practice security-design-2 F1 · ✓ verified — Interrupted upstream runs leave normal startup unmapped
- Gap `SR09.15` · Practice security-design-2 F1 · ✓ verified — Interrupted upstream runs leave normal startup unmapped
- Gap `SR09.18` · Practice security-design-2 F2 · ✓ verified — Persisted private files retain their old readable permissions
- Gap `SR09.21` · Practice security-design-2 F3 · ✓ verified — The vsock confinement check passes when its probe never executes
- Gap `SR16.08` · Practice security-operations-1 F3 · ✓ verified — An interrupted upstream run leaves a reusable unmapped launch configuration
- Gap `SR16.10` · Practice security-operations-1 F3 · ✓ verified — An interrupted upstream run leaves a reusable unmapped launch configuration
- Gap `SR16.13` · Practice security-operations-1 F5 · ✓ verified — The vsock confinement step can pass without executing a probe
- Gap `SR18.02` · Practice security-operations-2 F3 · ✓ verified — An interrupted upstream run leaves the normal startup configuration unmapped
- Gap `SR18.03` · Practice security-operations-2 F2 · ✓ verified — Existing snapshot files retain world-readable permissions after upgrade
- Gap `SR18.04` · Practice security-operations-2 F1 · ✓ verified — Writable ancestor shares expose the supposedly private boot configuration
- Gap `SR18.05` · Practice security-operations-2 F2 · ✓ verified — Existing snapshot files retain world-readable permissions after upgrade
- Gap `SR18.07` · Practice security-operations-2 F3 · ✓ verified — An interrupted upstream run leaves the normal startup configuration unmapped
- Gap `SR18.10` · Practice security-operations-2 F2 · ✓ verified — Existing snapshot files retain world-readable permissions after upgrade
- Gap `SRE26.03` · Practice sre-integrity F2 · ✓ verified — The upstream runner creates a publicly readable data disk
- Gap `SRE26.14` · Practice sre-integrity F1 · ✓ verified — Existing persistent files retain their readable modes
- Gap `SRE26.18` · Practice sre-integrity F3 · ✓ verified — The share confinement check lacks a working-listener control
- Gap `SRE26.20` · Practice sre-integrity F3 · ✓ verified — The share confinement check lacks a working-listener control
- Gap `SRE26.28` · Practice sre-integrity F1 · ✓ verified — Existing persistent files retain their readable modes
- Assurance `SR09.02` — Normal boot/build mechanisms are reused; safe emergency rollback of this security change is not demonstrated.
- Assurance `SR09.06` — The ExecPlan permits older-overlay rollback, which removes this confinement; no enforceable permitted-version…
- Assurance `SR09.08` — No minimum security version prevents rollback to an overlay without confinement; necessity and acceptance are…
- Assurance `SR09.09` — The disk records mapping mode, but not the confinement version or actual security update history.
- Assurance `SR09.24` — HEALTH distinguishes plugin and Docker state; end-to-end emergency access-policy strength and usability are n…
- Assurance `SR09.26` — Recovery instructions exist in the ExecPlan and runner messages; interrupted-run recovery practice is not sho…
- Assurance `SR16.01` — Restoration logic and recovery guidance exist; abrupt host-loss recovery is incomplete under F3.
- Assurance `SR16.09` — Named controls and console evidence are prestaged; failed-boot HEALTH is unavailable according to the plan.
- Assurance `SR16.15` — Plugin-loss and wrong-mode reboot captures exist; interrupted-config recovery and upgrade ACL preservation ar…
- Assurance `SR18.06` — TD-061 and TD-062 record remaining work and priority; an explicit lifetime and committed removal schedule are…
- Assurance `SRE26.02` — The plan describes config restoration and plugin recovery, but supplies no measured recovery or acceptable-lo…
- Assurance `SRE26.05` — SECURITY.md states that deleting the data image erases all container state and nothing else holds a copy; no…
- Assurance `SRE26.06` — The suite covers wrong-mode boots and plugin loss, but evidence is missing for partial mounts, record corrupt…
- Assurance `SRE26.11` — Permissions and mode checks add preventive layers, but independently recoverable copies and ongoing data vali…
- Assurance `SRE26.21` — The report demonstrates a normal boot after a wrong-mode refusal; the plan explicitly says the new upstream u…
- Assurance `SRE26.22` — No evidence demonstrates complete restoration after interruption of the new upstream disk/config transition.
- Assurance `SRE26.27` — The upstream runner hashes replacement configs and installs restoration traps, but interruption and concurren…

**Verification** → Test owner
- Gap `RULE.11` · Practice rules F4 · ✓ verified — The vsock confinement test accepts probe failures as protection
- Gap `SR13.02` · Practice security-operations-1 F5 · ✓ verified — The vsock confinement step can pass without executing a probe
- Gap `SR13.03` · Practice security-operations-1 F5 · ✓ verified — The vsock confinement step can pass without executing a probe
- Gap `SR13.05` · Practice security-operations-1 F6 · ✓ verified — The ownership regression is hidden by delaying the compatibility assertions
- Gap `SR13.10` · Practice security-operations-1 F5 · ✓ verified — The vsock confinement step can pass without executing a probe
- Gap `SRE17.03` · Practice sre-foundations-2 F4 · ✓ verified — Share ownership regression breaks immediate container operations
- Gap `SRE17.06` · Practice sre-foundations-2 F1 · ✓ verified — Existing persistent files retain their readable modes
- Gap `SRE17.07` · Practice sre-foundations-2 F4 · ✓ verified — Share ownership regression breaks immediate container operations
- Gap `SRE17.10` · Practice sre-foundations-2 F2 · ✓ verified — Root images are readable while being constructed
- Gap `SRE17.12` · Practice sre-foundations-2 F3 · ✓ verified — An interrupted upstream run leaves normal startup unmapped
- Gap `SRE17.13` · Practice sre-foundations-2 F3 · ✓ verified — An interrupted upstream run leaves normal startup unmapped
- Gap `SRE17.18` · Practice sre-foundations-2 F2 · ✓ verified — Root images are readable while being constructed
- Gap `SRE17.19` · Practice sre-foundations-2 F1 · ✓ verified — Existing persistent files retain their readable modes
- Gap `SRE17.20` · Practice sre-foundations-2 F2 · ✓ verified — Root images are readable while being constructed
- Gap `SRE17.21` · Practice sre-foundations-2 F1 · ✓ verified — Existing persistent files retain their readable modes
- Assurance `SR13.01` — Policy unit tests and runtime captures exist; mode preparation lacks hermetic failure-path coverage.
- Assurance `SR13.04` — Existing backend interface supports fakes; new shell boot-policy branches have no comparable isolated test se…
- Assurance `SR13.07` — Runtime reports are present, but instrumented builds and symbolised sanitizer diagnostics are not.
- Assurance `SR13.08` — authz tests cover malformed and duplicate JSON; fuzzing of request-policy interpretation is not evidenced.
- Assurance `SR13.13` — No continuous fuzzing or ownership/corpus workflow was found in the inspected CI.
- Assurance `SRE17.05` — Source and evidence are versioned, but green CI tied to the reviewed HEAD was not established from the inspec…
- Assurance `SRE17.15` — Battery reruns and causes are recorded, but repeatability and failure frequency of TD-062 are not quantified.

**Delivery** → Release owner
- Gap `RULE.13` · Practice rules F1 · ✓ verified — An interrupted upstream run leaves ordinary startup unmapped
- Gap `SR07.08` · Practice security-design-1 F2 · ✓ verified — Upgrading leaves existing private state world-readable
- Gap `SR07.11` · Practice security-design-1 F5 · ✓ verified — Immediate ownership-sensitive operations on shares now fail
- Gap `SR14.01` · Practice security-operations-1 F4 · ✓ verified — The saved configuration is not necessarily outside writable shares
- Gap `SR14.05` · Practice security-operations-1 F1 · ✓ verified — Existing snapshot files remain world-readable after upgrade
- Gap `SR14.15` · Practice security-operations-1 F3 · ✓ verified — An interrupted upstream run leaves a reusable unmapped launch configuration
- Gap `SRE07.03` · Practice sre-foundations-1 F3 · ✓ verified — An interrupted upstream run leaves the unmapped configuration reusable
- Gap `SRE07.06` · Practice sre-foundations-1 F1 · ✓ verified — Existing private state keeps its old readable permissions
- Gap `SRE07.07` · Practice sre-foundations-1 F3 · ✓ verified — An interrupted upstream run leaves the unmapped configuration reusable
- Gap `SRE07.08` · Practice sre-foundations-1 F2 · ✓ verified — Moving the saved config does not put it outside configured shares
- Gap `SRE08.04` · Practice sre-foundations-1 F2 · ✓ verified — Moving the saved config does not put it outside configured shares
- Gap `SRE08.07` · Practice sre-foundations-1 F4 · ✓ verified — The vsock confinement check passes when the probe cannot run
- Gap `SRE08.08` · Practice sre-foundations-1 F3 · ✓ verified — An interrupted upstream run leaves the unmapped configuration reusable
- Gap `SRE08.09` · Practice sre-foundations-1 F1 · ✓ verified — Existing private state keeps its old readable permissions
- Gap `SRE27.08` · Practice sre-runtime F2 · ✓ verified — An interrupted upstream run leaves ordinary startup configured as unmapped
- Gap `SRE27.10` · Practice sre-runtime F2 · ✓ verified — An interrupted upstream run leaves ordinary startup configured as unmapped
- Gap `SRE27.13` · Practice sre-runtime F1 · ✓ verified — Existing persistent files remain readable through single-file binds
- Gap `SRE27.15` · Practice sre-runtime F2 · ✓ verified — An interrupted upstream run leaves ordinary startup configured as unmapped
- Gap `SRE27.19` · Practice sre-runtime F1 · ✓ verified — Existing persistent files remain readable through single-file binds
- Gap `SREE.02` · Practice sre-runtime F2 · ✓ verified — An interrupted upstream run leaves ordinary startup configured as unmapped
- Gap `SREE.04` · Practice sre-runtime F1 · ✓ verified — Existing persistent files remain readable through single-file binds
- Gap `SREE.05` · Practice sre-runtime F1 · ✓ verified — Existing persistent files remain readable through single-file binds
- Gap `SREE.06` · Practice sre-runtime F2 · ✓ verified — An interrupted upstream run leaves ordinary startup configured as unmapped
- Assurance `RULE.12` — build-overlay.sh installs the cross-built plugin and verifies pinned package/image inputs; shim CI tests and…
- Assurance `SR07.01` — Milestones and retained before/after probes exist; gradual deployment and independent rollout instrumentation…
- Assurance `SR07.06` — Scope and exposure are documented, but this review has no rollout-timing or disclosure decision evidence.
- Assurance `SR14.02` — Prior council review is recorded; repository rules enforcing independent, non-bypassable review were not veri…
- Assurance `SR14.03` — Overlay/build steps are scripted and CI-reviewed; builder hardening and administrator bypass controls are not…
- Assurance `SR14.04` — Image CI verifies inputs and reproducibility; an unavoidable check of pipeline proof before local deployment…
- Assurance `SR14.08` — Build scripts stamp source revision and CI checks manifests; authenticated provenance for the complete deploy…
- Assurance `SR14.09` — Build flags are explicit; trusted signer/toolchain enforcement was not established.
- Assurance `SR14.10` — ADR 0010 defines boot-mode policy; complete environment-specific deployment admission is not evidenced.
- Assurance `SR14.11` — Digest checks exist, but exact-artefact authenticated provenance verification is not demonstrated.
- Assurance `SR14.13` — Archives/packages are pinned and overlay determinism is checked; complete host-toolchain hermeticity is not e…
- Assurance `SR14.16` — Console mode/refusal lines and HEALTH provide context, but do not bind the running daemon to an immutable con…
- Assurance `SR14.18` — Artefact manifests and source stamps exist; one unambiguous authenticated provenance record per deployed over…
- Assurance `SRE07.04` — Existing-suite artifacts exist; the new upstream unmapped turn-up path remains unexercised.
- Assurance `SRE08.05` — Battery artifacts describe a rebuilt overlay, but the summary does not establish an immutable binary/config i…
- Assurance `SRE08.10` — docker-up compares requested mode with a preparation marker; it does not inspect the running daemon’s effecti…
- Assurance `SRE27.06` — Overlay installation and HEALTH integration are present; operational monitoring of unexpected policy/configur…
- Assurance `SRE27.14` — Runtime battery exists, but no staged deployment, observation period or automatic halt evidence is supplied.
- Assurance `SRE27.17` — Sequential latency measurement is not nonlinear overload testing.
- Assurance `SREE.01` — Request paths and functional integration are covered; concurrent load and shared-service capacity impact rema…
- Assurance `SREE.03` — HEALTH and bounded refusal logs exist; HEALTH's recorded mode does not independently attest the live daemon c…

**Operation** → On-call lead
- Gap `SR15.07` · Practice security-operations-1 F2 · ✓ verified — Root-drive files become private only after their contents are written
- Gap `SRE06.03` · Practice sre-foundations-1 F4 · ✓ verified — The vsock confinement check passes when the probe cannot run
- Gap `SRE10.11` · Practice sre-foundations-1 F4 · ✓ verified — The vsock confinement check passes when the probe cannot run
- Gap `SRE12.02` · Practice sre-foundations-2 F5 · ✓ verified — Negative confinement probes can pass without testing their boundary
- Gap `SRE12.08` · Practice sre-foundations-2 F5 · ✓ verified — Negative confinement probes can pass without testing their boundary
- Gap `SRE12.09` · Practice sre-foundations-2 F3 · ✓ verified — An interrupted upstream run leaves normal startup unmapped
- Gap `SRE13.02` · Practice sre-foundations-2 F3 · ✓ verified — An interrupted upstream run leaves normal startup unmapped
- Assurance `RULE.10` — HEALTH exposes mode/plugin state and refusal logs are bounded and private. Mode comes from the preparation ma…
- Assurance `SR15.05` — Committed probes and captures support investigation, but upgrade and interruption reproductions for F1–F4 are…
- Assurance `SR15.10` — Authz logs timestamp refusals; complete access, successful state-change and ownership-change logging is not d…
- Assurance `SR15.11` — Logs are local bounded files; remote immutable retention is not evidenced.
- Assurance `SR15.13` — Agent, daemon and microVM evidence is available; end-to-end detection coverage and alert triage are not estab…
- Assurance `SRE06.04` — Plugin latency and refusals are observed; saturation and sustained error behaviour are not demonstrated.
- Assurance `SRE12.11` — The ownership explanation has recorded observations, but no verified corrective implementation; TD-062 remain…

**Organisation** → Engineering manager
- Gap `SR19.05` · Practice security-operations-2 F1 · ✓ verified — Writable ancestor shares expose the supposedly private boot configuration
- Gap `SR20.08` · Practice security-operations-2 F4 · ✓ verified — The vsock confinement check passes when no probe executes
- Gap `SR21.07` · Practice security-operations-2 F4 · ✓ verified — The vsock confinement check passes when no probe executes
- Gap `SR21.12` · Practice security-operations-2 F1 · ✓ verified — Writable ancestor shares expose the supposedly private boot configuration
- Gap `SR22.01` · Practice security-operations-2 F1 · ✓ verified — Writable ancestor shares expose the supposedly private boot configuration
- Gap `SRE29.06` · Practice sre-organisation F3 · ✓ verified — Abrupt runner loss leaves an unmapped configuration usable by ordinary startup
- Gap `SRE31.14` · Practice sre-organisation F1 · ✓ verified — Existing snapshot files remain readable by remapped containers
- Gap `SRE32.03` · Practice sre-organisation F4 · ✓ verified — The share-server confinement check lacks a successful control
- Gap `SRE32.04` · Practice sre-organisation F2 · ✓ verified — The saved normal configuration can still lie inside a writable share
- Gap `SRE33.04` · Practice sre-organisation F4 · ✓ verified — The share-server confinement check lacks a successful control
- Gap `SRE33.05` · Practice sre-organisation F1 · ✓ verified — Existing snapshot files remain readable by remapped containers
- Gap `SRE33.07` · Practice sre-organisation F2 · ✓ verified — The saved normal configuration can still lie inside a writable share
- Gap `SRE33.08` · Practice sre-organisation F4 · ✓ verified — The share-server confinement check lacks a successful control
- Gap `SRE34.03` · Practice sre-organisation F3 · ✓ verified — Abrupt runner loss leaves an unmapped configuration usable by ordinary startup
- Assurance `SR19.04` — supervise-daemon and fault evidence address service recovery; incremental release, automatic-update and relea…
- Assurance `SR20.02` — Kernel user namespaces and a reusable peer helper are used; completed specialist review of the integrated bou…
- Assurance `SR21.11` — The battery demonstrates internal testing, but staged adoption, opt-in rollout and progressively stronger enf…
- Assurance `SRE31.02` — Runtime reports connect behaviour to implementation; no service-review participation evidence.
- Assurance `SRE31.05` — ExecPlan records remaining M6 work; stakeholder follow-through is not established by this diff.
- Assurance `SRE31.10` — ADR, implementation and runtime artifacts exist; M6 remains explicitly incomplete.
- Assurance `SRE31.13` — Design records interfaces and failure behaviour; joint operational agreement is not evidenced.
- Assurance `SRE31.15` — Recorded latency and battery results exist; upstream unmapped-run validation remains explicitly missing.
- Assurance `SRE32.12` — Confinement automation exists, but F4 limits its evidence; broader application/platform ownership is unchange…
- Assurance `SRE34.02` — Interfaces and fallback rationale are documented; recovery and migration defects prevent full assurance.

### Coverage
A gap means nobody reviewed the lens, not that it is fine.

| Lens | Status | Evidence |
| --- | --- | --- |
| Security | covered | #1, #2, #11; Codex traced the authz routes |
| Privacy | covered | #2, #5, #6 |
| Supply chain | covered (thin) | Codex: no manifest changes |
| Correctness | covered | #1; Claude traced record and HEALTH |
| API and compatibility | covered | #7, #12 |
| Rollout and migration | covered | #3; the #2 migration gap came from others |
| Reliability | covered | #9, #15, #19 |
| Scalability and performance | covered (weak) | #10 unmeasured |
| Concurrency | covered (weak) | sound; dockerd respawn not examined |
| Observability | covered | #9, #12; log modes and HEALTH read |
| Testing | covered | #4, #8, #13, #14 |
| Design and maintainability | covered | #16, #17 |
| Readability and idioms | covered | Claude read every hunk: sound |

### Consensus
| # | Codex | Claude | Antigravity | Practices | Judge |
| --- | --- | --- | --- | --- | --- |
| 1 | ○ | ● | ○ | ○ | ✓ |
| 2 | ● | ○ | ● | ● | ✓ |
| 3 | ○ | ● | ○ | ● | ✓ |
| 4 | ● | ● | ○ | ● | ✓ |
| 5 | ○ | ○ | ○ | ● | ✓ |
| 6 | ○ | ○ | ○ | ● | ✓ |
| 7 | ○ | ○ | ○ | ● | ✓ |
| 8 | ○ | ○ | ○ | ● | ✓ |
| 9 | ○ | ○ | ● | ○ | ? |
| 10 | ○ | ○ | ● | ○ | ? |
| 11 | ○ | ○ | ○ | ● | ✓ |
| 12 | ○ | ● | ○ | ○ | ✓ |
| 13 | ○ | ● | ● | ○ | ✓ |
| 14 | ○ | ○ | ● | ○ | ✓ |
| 15 | ○ | ○ | ● | ○ | ✓ |
| 16 | ○ | ● | ○ | ○ | ✓ |
| 17 | ○ | ● | ○ | ○ | ✓ |
| 18 | ○ | ○ | ● | ○ | ✓ |
| 19 | ○ | ○ | ● | ○ | ✓ |
| 20 | ○ | ○ | ● | ○ | ✓ |

Dropped as false:
- Antigravity F3: Bash runs the EXIT trap on a fatal SIGTERM. A mapped disk also refuses the unmapped token.
- Practice sre-foundations-2 F7: a note about the review prompt is not a defect in the change.

● flagged · ○ not flagged · ✓ verified · ? unverified · ✗ false

### Strengths
- The mapped boot prepares the mode record and starts `vesper-authz` before dockerd, and a boot whose mode does not match the disk's record is refused (step 8 shows this).
- Container uid 100000 is kept separate from jail uids 10000 and 20000, and the plugin refuses requests when it is unavailable.
- Comments explain why, and the HEALTH and `CONTAINERS` verb parsing in `vesper-agent`, `src/control.rs` and `systemvmctl` is sound.

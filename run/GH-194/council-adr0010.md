## Code Review Council Report

**⛔ Do not merge** · 2 blockers
🔴 0 Critical · 🟠 2 High · 🟡 9 Medium · 🔵 4 Low · ▲ 3 coverage gaps
`docs/design-docs/adr/0010-no-container-is-systemvm-root.md` +9 more (10 files, +427 −12) · Markdown, POSIX shell, Go, Rust
Agents: Codex · Claude · Antigravity · Practices (11) · Judge

The ADR's authorisation plugin is a deny-list. It reads a request body that dockerd leaves out at 1 MiB and above, and enabling remapping breaks the share-network suite shipped in 8535750. Remapping is still the right mechanism, and the spike supports its central claim. The migration, the unmapped-boot exception, private-state protection and the spike's evidence all need work before the ADR can serve as an implementation contract.

### Next steps
1. **Fix #1** before merge → read `pkg/authorization/authz.go` at moby v28.3.3 (the body is passed only when it is JSON and smaller than 1 MiB), then restate ADR:55-60 as default-deny.
2. **Fix #2** before merge → `grep -rn -e '--privileged' systemvm/overlay scripts/systemvm` (it returns 4 callers), then list them all in item 4.
3. **Close the gap** ▲ Scalability and performance → measure the plugin's latency on every Docker call.
4. **Close the gap** ▲ Concurrency → analyse a dockerd restart while the plugin restarts, and the reaper and lock interaction under remapping.
5. **Close the gap** ▲ Readability and idioms → run `python3 harness/check_docs.py` and do a full readability pass.
6. **Follow up** after merge: #3–#15

### 🟠 High

**#1 · The authorisation plugin is a deny-list over a body dockerd does not always send**
`docs/design-docs/adr/0010-no-container-is-systemvm-root.md:55-60` · Security / API and compatibility · Codex F2, Claude F1 · ✓ verified

In dockerd 28.3.3, a plugin receives the request body only when it is JSON and its `ContentLength` is between 0 and 1 MiB. The create handler still decodes the full body. A create request padded past 1 MiB with `UsernsMode: host` therefore reaches a plugin that only refuses a visible `host` with no body, and the plugin allows it. ADR:92 ("one job with a small surface") is the deny-list shape that ADR:77-79 itself rejects. Codex rated this Medium; the judge rated it High.
- **Verify:** Confirm the predicate in moby v28.3.3 `pkg/authorization/authz.go`. Once the plugin is built, send `POST /v1.51/containers/create` with `"HostConfig":{"UsernsMode":"host"}` and a 1.1 MiB label, fixed-length and chunked. It must be refused.
- **Fix:** Specify the plugin as default-deny. Allow-list each method together with its cleaned, version-stripped path. On create, refuse unless the body is present, decodes into dockerd's own struct shape, and has an empty `UsernsMode`. Refuse `/plugins`, `/swarm`, `/services`, `/nodes`, `/tasks`, `/session` and `/grpc` until each one is shown to be mapped. An alternative is a dockerd patch or build without the `host` userns mode, at the cost of carrying a patch.

**#2 · Remapping breaks `make systemvm-sharenet-suite`, and item 4 does not list its caller**
`systemvm/overlay/usr/local/bin/vesper-microvm:209-214` · Rollout and migration · Claude F2, Antigravity F1 · ✓ verified

`test_share_reach` runs `docker run --rm --privileged --network "$mode"` for `bridge` and `host`. The spike shows dockerd refusing both (out.txt:72, 86). The exit code then hits the `ERR` branch, and `sharenet.rs:142-145` fails the suite. It also requires a `REACH container-host closed` row, which cannot exist under remapping. Item 4 names only `data-disk-suite.sh`.
- **Verify:** `grep -rn -e '--privileged' systemvm/overlay scripts/systemvm` returns vesper-microvm:211, data-disk-suite.sh:31 and upstream-suite/run.sh:245 and :285.
- **Fix:** List all four callers in item 4. Rewrite `test_share_reach` to use `--cap-add ALL --security-opt seccomp=unconfined -v /:/host` on `bridge`. In the same change, drop or redefine `container-host` in `sharenet.rs:145` and `src/README.md:471`.

### 🟡 Medium

**#3 · Migration hides named volumes and stopped containers; "pulled or built again" does not recover them**
`docs/design-docs/adr/0010-no-container-is-systemvm-root.md:105-107` · Rollout and migration · Claude F5, Practice rules F1, security-design-1 F1, security-design-2 F1, security-operations-1 F1, security-operations-2 F1, sre-foundations-1 F1, sre-foundations-2 F1, sre-integrity F1, sre-organisation F1, sre-runtime F1 · RULE.08, RULE.09, RULE.13, SR01.02, SR04.03, SR04.11, SR07.13, SR07.18, SR09.01, SR09.06, SR09.19, SR12.11, SR16.08, SR16.10, SR21.13, SR21.14, SR22.01, SREP.01, SREP.02, SRE07.03, SRE08.08, SRE08.09, SRE13.02, SRE17.09, SRE17.13, SRE17.21, SRE26.01, SRE26.04, SRE26.26, SRE26.28, SRE31.14, SRE34.03, SRE27.10, SRE27.13, SRE27.19, SREB.02, SREE.02, SREE.05 · ✓ verified

`data-disk-suite.sh:43,66-67` asserts that volume contents and stopped containers survive a reboot. After the switch, the same Compose project gets a new, empty volume with the same name. A rollback then exposes the old data and hides every write made since the switch. Most practice reviewers rated this High; the judge rated it Medium.
- **Verify:** `sed -n '40,67p' scripts/systemvm/data-disk-suite.sh`. On a disposable disk, seed a named volume, enable remapping, run `docker run --rm -v <vol>:/v alpine:3.22 cat /v/stamp`, write a second stamp, then roll back.
- **Fix:** Treat images (rebuildable) separately from volumes and container layers (data). Refuse automatic adoption of a populated legacy store until the operator chooses an ownership-aware export and import (shifting ownership by 100000) or an explicit fresh start. Define how rollback handles post-switch writes. Record the store mode on the disk.

**#4 · The unmapped boot reuses the legacy store, so old restart-policy containers start again with root authority**
`docs/design-docs/adr/0010-no-container-is-systemvm-root.md:67-71` · Security · Practice rules F2, Claude F5, Antigravity F6 · RULE.02, RULE.03, RULE.13 · ✓ verified

In unmapped mode, dockerd uses `/var/lib/docker` directly on the same data disk. Legacy `--restart=always` containers start again unmapped on every suite boot without any create request, so the create-time plugin cannot stop them. The second store also competes with the upstream suite's 45 GB free-space check. Remapping leaves `/var/lib/docker` with mode `drwx--x--- root 100000` (out.txt:24), and the spike never reverts that group, so an N-1 rollback inherits it.
- **Verify:** `grep -n -e 'UPSTREAM_MIN_FREE_GB' -e 'host_path' scripts/systemvm/upstream-suite/run.sh`. On a disposable disk, create a restart-policy container that writes a marker through a host bind, migrate, boot unmapped, and check that no marker appears.
- **Fix:** Give the upstream suite its own disposable data disk (or its own `data-root`) and minimal shares. Reject any configuration that combines the switch with the normal disk.

**#5 · The unmapped switch can be persisted from inside the VM, and only the untrusted side reports it**
`scripts/systemvm/upstream-suite/run.sh:29,33,199,229` · Rollout and migration · Claude F4, Antigravity F6 · ✓ verified

`SAVED` sits in `$B`, which line 229 shares read-write into the VM. `restore()` runs `cp "$SAVED" "$CFG"`. Any process in the unmapped boot, including third-party upstream test containers, can therefore write the switch into the next normal boot's config. That breaks "only the macOS side can set it". Guest flags travel only through `kernel_cmdline`. An aborted run leaves `vesper-proxy` serving an unmapped daemon, and `HEALTH` is produced inside the VM.
- **Verify:** `grep -n -e 'SAVED=' -e '^B=' -e 'cp "$SAVED"' -e 'host_path' scripts/systemvm/upstream-suite/run.sh`
- **Fix:** Keep `SAVED` outside every share, or hash it on macOS before restoring. Pass the switch for one boot as a launcher flag or kernel command-line token, not as a persisted config field. Have the launcher report the mode itself and treat `HEALTH` as a cross-check only. Add a test that a normal `make systemvm-up` after the suite reports remapped mode.

**#6 · Owner-only `/run/vesper` leaves logs, records and state files readable**
`docs/design-docs/adr/0010-no-container-is-systemvm-root.md:72` · Privacy · Codex F1, Antigravity F9, Practice security-operations-1 F2, sre-organisation F2 · SR12.02, SR12.06, SR15.07, SRE32.04, SRE33.05 · ✓ verified

`/var/log/vesper-microvm-<id>.log` and its `.log.1` are 0644, because `vesper-boundlog` sets no umask. Workload records carrying `cmd_b64` under `/var/lib/vesper/workloads` use default modes. `/var/log/vesperd`, `vesper-memadmit` and `vesper-rootfs` use a plain `mkdir -p`, and `vesper-vesperd` explicitly sets 0755. The record at `vesper-microvm:363` and the leases at `vesper-vesperd:174` are 0644, and rootful dockerd can bind a single file directly, bypassing a 0700 parent. Codex and sre-organisation rated this High; the judge rated it Medium. Antigravity was wrong on one point: `$STATE` is 0700.
- **Verify:** `grep -n "mkdir -p\|chmod \|umask" systemvm/overlay/usr/local/bin/vesper-{vesperd,microvm,memadmit,rootfs,boundlog,workload}`, then run Codex F1's Python harness against `vesper-boundlog`.
- **Fix:** Inventory private state beyond `/run`. Create files with `umask 077` or 0600, including on rotation and recreation, and repair existing files. Add negative tests for directory binds and direct file binds using the remapped uid.

**#7 · One shared remap uid lets any container with `-v /:/host` read another container's volumes**
`docs/design-docs/adr/0010-no-container-is-systemvm-root.md:102` · Security · Practice sre-runtime F2 · SREE.04 · ✓ verified

Every container runs as uid 100000. The moby local volume root is 0701, and `_data` belongs to the remapped root. A container can therefore read another task's 0600 secret through `/host/var/lib/docker/100000.100000/volumes/<name>/_data`, which contradicts the ADR's promise about "another task's private state". The reviewer rated this High; the judge rated it Medium.
- **Verify:** On an isolated VM with remapping, write a root-owned 0600 sentinel in volume `victim`, then read it from a different container that has only `-v /:/host`.
- **Fix:** Narrow the guarantee to host-root prevention plus root-owned microVM state. Alternatively, block binds that expose private stores, including ancestor binds and volume-driver aliases.

**#8 · The root channel in item 4 is reachable from `seccomp=unconfined`, and the verbs it needs do not exist**
`docs/design-docs/adr/0010-no-container-is-systemvm-root.md:61-66` · Design and maintainability · Claude F3, Antigravity F5 · ✓ verified

SECURITY.md:192-195 says a `seccomp=unconfined` container can open `AF_VSOCK`, which contradicts "confer nothing". The socat listener accepts any peer, and the probe at `vesper-vesperd:1181` tests only CIDs 1 and 3. `vesper-data` offers only `mount|status|stop`, but `data-disk-suite.sh` needs `/proc/mounts`, `e2label` and `blkid`. `run.sh:245`'s `modprobe tun` is redundant with `vesper-services:23`. Antigravity rated this High; the judge rated it Medium.
- **Verify:** `grep -n 'for cid in' systemvm/overlay/usr/local/bin/vesper-vesperd`, then read SECURITY.md:192-195 beside ADR:61-66.
- **Fix:** Correct the ADR. Make the agent refuse any peer whose CID is not 2, and block `vsock_loopback` from loading. Add unconfined probes for CID 1, CID 2 and the VM's own CID. Replace the generic root shell with narrow, validated verbs, and replace run.sh:245 with a `HEALTH` `"tun":true` check.

**#9 · The plugin has no startup ordering, readiness gate or supervision**
`systemvm/overlay/etc/init.d/vesper-services:39-47` · Reliability · Antigravity F2 · ✓ verified

Only containerd and docker start before `services-ready`. The boot probe (lock.yaml:78) and `HEALTH` both gate on `docker version`, so a plugin that is not up yet, or has crashed, fails the boot. Antigravity rated this High and called it a "deadlock"; the judge rated it Medium.
- **Verify:** `sed -n '39,47p' systemvm/overlay/etc/init.d/vesper-services`
- **Fix:** Run the plugin as a `supervise-daemon` service with bounded respawn. Remove stale sockets before binding, gate on `/Plugin.Activate`, and start it before `docker`. Test plugin loss during operation.

**#10 · The spike output does not support three ADR statements**
`run/GH-160/spike-userns-remap/spike.sh:21,29` · Correctness / Testing · Claude F6, Antigravity F8 · ✓ verified

Each probe runs twice (spike.sh:21). The `modprobe dummy` that succeeded at out.txt:159 ran without `CAP_SYS_MODULE`, so the module was already loaded and "cannot load a module of its choice" was never tested. `/dev/kvm` and `/dev/vda` were probed in the container's own `/dev`. `--cgroupns=host` was only tried together with `--privileged`. Under capadd, `nsenter -t 1` targeted the container's own PID 1. One correction: Antigravity's claim that `ip link` contradicts ADR:45 is wrong. The empty message means the first run succeeded. The judge rated Antigravity's part Low.
- **Verify:** `grep -n -e 'ip link add' -e 'modprobe dummy' -e '/dev/kvm' run/GH-160/spike-userns-remap/out.txt`
- **Fix:** Run each probe once. Use `modprobe` on a module that is not loaded, and check `lsmod` before and after. Add rows for `/host/dev/{kvm,vsock,vhost-vsock,net/tun,fuse}`, `--device /dev/kvm`, `--ipc=host`, `--uts=host` and `--cgroupns=host` alone. Reword ADR:46.

**#11 · Plugin route matching does not specify path canonicalization or the build, Swarm and gRPC routes**
`docs/design-docs/adr/0010-no-container-is-systemvm-root.md:55-60` · Security · Antigravity F4 · ? unverified

Version prefixes, query strings and double slashes can defeat naive prefix matching. `/build?networkmode=host`, `/grpc`, `/session` and Swarm task creation sit outside `/containers/create`. Antigravity rated this Critical; the judge rated it Medium and unverifiable, because the plugin does not exist yet and ADR:117 already lists these routes as unknown.
- **Verify:** After the plugin is built, run unit tests for `/v1.48/containers/create`, `//plugins/pull`, mixed case and `/build?networkmode=host`.
- **Fix:** Fold this into #1's allow-list: parse, run `path.Clean`, strip the version, and default-deny.

### 🔵 Low

- **#12** `spike.sh:6` · Testing · Practice security-design-2 F2, security-operations-1 F3, sre-foundations-1 F2, sre-foundations-2 F2 · SR08.23, SR11.07, SR13.02, SR13.03, SR13.10, SR15.03, SRE02.06, SRE04.10, SRE07.06, SRE12.08, SRE17.10 · ✓ verified — `rc=$?` reports `cut`'s exit status, so refusals show `rc=0` → capture Docker's status before filtering.
- **#13** `spike.sh:11,39,42-43` · Reliability · Practice sre-foundations-1 F3, sre-foundations-2 F3 · SREP.02, SRE07.03, SRE07.10, SRE12.09, SRE13.02, SRE17.10 · ✓ verified (judge Low, sre-foundations-1 claimed High) — the spike overwrites `daemon.json` and deletes stores with no saved state or trap → mark it not to be rerun, or restore through a trap.
- **#14** `ADR:113-115` · Design and maintainability · Claude F7, Antigravity F10 · ✓ verified (judge Low, Antigravity claimed High) — the shim is reachable only through `ctr`, not dockerd, so the "Unknown" targets the wrong client → restate it and add `ctr` and non-`moby` namespaces to milestone 1 as out of remapping's reach.
- **#15** `vesper-agent:54-84` · Observability · Antigravity F7 · ✓ verified (judge Low, reviewer claimed Medium) — `HEALTH` and `LOGS` have no plugin state, userns flag or denial log → add these fields and a bounded plugin log.

### Practices
770 rules and recommendations checked · 122 satisfied · 59 gaps · 206 assurance gaps · 377 n/a · 3 exceptions · 3 not evaluated

| Area | Human reviewer | Sat | Gap | Assur | N/A | Exc | Not eval |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Purpose and risk | Tech lead | 25 | 8 | 21 | 30 | 0 | 0 |
| Identity and data | Security and privacy | 12 | 2 | 14 | 11 | 2 | 1 |
| Interfaces and state | Service owner | 14 | 4 | 15 | 63 | 1 | 0 |
| Capacity and failure | SRE | 6 | 2 | 40 | 56 | 0 | 0 |
| Isolation and recovery | SRE and security | 4 | 10 | 29 | 49 | 0 | 0 |
| Verification | Test owner | 6 | 7 | 15 | 12 | 0 | 0 |
| Delivery | Release owner | 21 | 14 | 34 | 18 | 0 | 0 |
| Operation | On-call lead | 8 | 5 | 20 | 50 | 0 | 0 |
| Organisation | Engineering manager | 26 | 7 | 18 | 88 | 0 | 2 |

Sat = satisfied with evidence · Gap = defect, see the finding it cites · Assur = evidence missing, no proven defect · Exc = recorded exception · Not eval = no evidence, reviewer failed, or a gap claim the judge found false

**Purpose and risk** → Tech lead
- Gap `RULE.02` · Practice rules F2 · ✓ verified — The upstream exception reopens the legacy workload store
- Gap `SR01.02` · Practice security-design-1 F1 · ✓ verified — Existing volume data has no migration or reconciliation path
- Gap `SR04.03` · Practice security-design-1 F1 · ✓ verified — Existing volume data has no migration or reconciliation path
- Gap `SR04.11` · Practice security-design-1 F1 · ✓ verified — Existing volume data has no migration or reconciliation path
- Gap `SREP.01` · Practice sre-foundations-1 F1 · ✓ verified — The migration strands mutable volume data
- Gap `SREP.02` · Practice sre-foundations-1 F1 · ✓ verified — The migration strands mutable volume data
- Gap `SRE02.06` · Practice sre-foundations-1 F2 · ✓ verified — The spike records successful status for failed Docker commands
- Gap `SRE04.10` · Practice sre-foundations-1 F2 · ✓ verified — The spike records successful status for failed Docker commands
- Assurance `RULE.01` — ADR 0010:50–73 defines confinement invariants and mode reporting; migration recovery objectives and measurabl…
- Assurance `SR01.06` — ADR 0010:123–129 requires tests; plugin adversarial tests, staged deployment and provenance are not yet demon…
- Assurance `SR01.07` — The proposed plugin has no specified denial/audit logging contract or sensitive-body logging policy.
- Assurance `SR01.08` — Agent recovery is proposed, but plugin-outage and interrupted migration recovery have not been rehearsed.
- Assurance `SR02.07` — Automatic privilege checks are proposed, but effective-policy auditing and operator mistake checks remain uns…
- Assurance `SR04.04` — Existing boot ordering is visible in vesper-services; plugin bootstrap and effective-policy readiness remain…
- Assurance `SR04.07` — The unmapped fallback boot lacks specified workload, share and data-disk isolation.
- Assurance `SR04.10` — Confinement and compatibility suites are planned; upgrade fixtures and staged rollback validation are missing.
- Assurance `SRA.01` — Threat tables catalogue actors, assets and boundaries; likelihood/impact prioritisation for retained kernel e…
- Assurance `SRE01.01` — Owner and GH #194 are identified; plugin operational ownership and recovery contracts remain unspecified.
- Assurance `SRE01.05` — HEALTH reporting is proposed; actionable confinement/plugin failure reporting is unspecified.
- Assurance `SRE01.06` — Existing supervision is present; new plugin failure and recovery rehearsals are absent.
- Assurance `SRE01.07` — Implementation milestones exist; rollout detection and rollback validation are incomplete.
- Assurance `SRE01.08` — Retaining both stores can increase disk use; migration headroom evidence is absent.
- Assurance `SRE01.09` — Confinement and Docker suites are proposed but not yet demonstrated.
- Assurance `SRE02.01` — Existing daemons respawn; plugin supervision and limits are not designed in detail.
- Assurance `SRE02.03` — Existing ext4 persistence remains; mutable-state transition is unresolved.
- Assurance `SRE02.05` — HEALTH mode reporting is proposed; effective-policy identity/history is unspecified.
- Assurance `SRE04.02` — Desired containment behaviour is clear; acceptance probes are not yet established.
- Assurance `SRE04.03` — End-to-end Docker/share probes are planned; agent reachability remains unverified.
- Assurance `SRE18.09` — User-facing refusals are promised; migration and recovery guidance for existing installations is incomplete.

**Identity and data** → Security and privacy
- Gap `RULE.03` · Practice rules F2 · ✓ verified — The upstream exception reopens the legacy workload store
- Gap `SR11.07` · Practice security-design-2 F2 · ✓ verified — Spike records successful exit status when Docker fails
- Assurance `RULE.04` — Remapping is the proposed secure default and the plugin uses the standard interface; request parsing, body ha…
- Assurance `SR03.02` — ADR 0010:117 leaves route coverage open; direct sockets and retained-object paths have no supplied closure ev…
- Assurance `SR03.05` — Missing-body handling, route normalisation, policy tests and structured execution/denial auditing are unspeci…
- Assurance `SR03.06` — The additional service’s startup dependency, failure timing, supervision and maintenance contract are not def…
- Assurance `SR05.04` — The agent already exposes named operations, but the replacement for the operator’s root shell is undefined.
- Assurance `SR05.05` — Neither shell transcripts nor spike output establish granular, tamper-resistant authorisation audit records.
- Assurance `SR05.08` — The spike covers representative allowed/denied actions; complete forbidden-action and malformed-request tests…
- Assurance `SR05.09` — The spike used a live VM; a disposable confinement and migration test environment is not specified.
- Assurance `SR05.11` — The upstream exception is boot-scoped and reported, but restriction, monitoring and recovery tests remain und…
- Assurance `SR05.12` — Configuration is version-controlled; canary procedure and effective-policy health checks are not specified.
- Assurance `SR05.16` — Policy deployment, effective-policy inspection and debugging tools are not yet designed.
- Assurance `SR11.04` — Plugin and daemon decoding equivalence is not specified; adversarial interpretation tests are needed.
- Assurance `SR11.05` — Authorization is centralized, but the complete set of execution routes is explicitly unproven.
- Assurance `SR11.06` — A pinned-daemon spike exists; hardened BuildKit, Swarm, runtime and plugin interface coverage remains open.
- Exception `SR05.02` — D-018 and design-v0’s threat table trust same-user macOS callers and defer additional control-channel authent…
- Exception `SR05.14` — D-018 records why extra cryptographic authentication is deferred for the existing same-user control path.
- Not evaluated (1): SR11.02

**Interfaces and state** → Service owner
- Gap `RULE.08` · Practice rules F1 · ✓ verified — Switching stores silently replaces existing named volumes with empty ones
- Gap `SR12.02` · Practice security-operations-1 F2 · ✓ verified — Owner-only directories do not protect directly bind-mounted state files
- Gap `SR12.06` · Practice security-operations-1 F2 · ✓ verified — Owner-only directories do not protect directly bind-mounted state files
- Gap `SR12.11` · Practice security-operations-1 F1 · ✓ verified — Named volumes cannot be recovered by pulling or rebuilding images
- Assurance `SR06.03` — The namespace mental model is explicit; outage behaviour and the precise exception-mode model need completion.
- Assurance `SR06.05` — Central authorisation is proposed, but fail-closed handling of unavailable request bodies is not specified.
- Assurance `SR06.06` — The launch-config field, HEALTH representation and canonical API-path matching contract are undefined.
- Assurance `SR06.08` — Subordinate ranges are proposed, but stable allocation, overlap validation and identity persistence are not s…
- Assurance `SR06.12` — Kernel dependence is explicit; plugin, daemon, alternate launch paths and exception-mode TCB coverage remain…
- Assurance `SR06.13` — UID separation is supported by the spike; complete separation from private state and administrative interface…
- Assurance `SR06.16` — The safety-relevant create body can be unavailable to AuthZ; required validation at that boundary is unspecif…
- Assurance `SR12.03` — ADR:55–60 requires pre-action authorisation; request parsing, rejection cleanup and diagnostics are unspecifi…
- Assurance `SR12.04` — Plugin failure is intended to deny requests; startup ordering, bounded calls and recovery/readiness integrati…
- Assurance `SR12.07` — No XML change; secure plugin parsing and production configuration defaults await implementation.
- Assurance `SR12.10` — The plugin is intended as one shared enforcement point, but its safe parsing and exceptional routes are unpro…
- Assurance `SR12.16` — Subordinate IDs, range length and launch-mode representation are not specified as a validated configuration c…
- Assurance `SRE09.01` — Upstream-only lifetime is intended; persistent exceptional configuration and shared-state isolation need evid…
- Assurance `SRE09.04` — Plugin responsibility is narrow; proposed operator-shell API scope remains undefined.
- Assurance `SRE09.05` — Docker and agent interfaces remain; shim ownership and configuration compatibility are unresolved.
- Exception `SR06.10` — D-018 defers new transport authentication on the established same-user bridge path.

**Capacity and failure** → SRE
- Gap `SR08.23` · Practice security-design-2 F2 · ✓ verified — Spike records successful exit status when Docker fails
- Gap `SREB.02` · Practice sre-runtime F1 · ✓ verified — The migration strands persistent volumes and provides no state-compatible rollback
- Assurance `RULE.06` — The new authorisation service has no specified request, concurrency or timeout bounds; no resource-exhaustion…
- Assurance `RULE.07` — The ADR deliberately preserves security by refusing requests when authorisation is unavailable; degraded oper…
- Assurance `SR08.01` — ADR 0010:99–101 retains the kernel boundary limitation; combined containment and plugin-failure behaviour rem…
- Assurance `SR08.02` — ADR 0010 models broad-flag workloads and module autoload; probing detection and residual-interface coverage a…
- Assurance `SR08.03` — Remapping complements ADR 0009 isolation, but escape containment and complete API coverage are not demonstrat…
- Assurance `SR08.05` — Spike output is truncated and statuses are unreliable; allocation/failure-stage evidence for the proposed plu…
- Assurance `SR08.09` — ADR 0010:58 specifies denial on plugin failure; startup, restart and malformed-request enforcement need proof.
- Assurance `SR08.10` — Agent administration remains available independently of Docker, but replacement commands and recovery exercis…
- Assurance `SR08.11` — Namespace compartments are explicit; effective socket permissions and all cross-boundary paths need validatio…
- Assurance `SR08.12` — ADR 0010:126–127 plans forbidden-crossing tests; implementation and runtime evidence are absent.
- Assurance `SR08.17` — The plugin adds a common Docker dependency; restart ordering and known-good configuration recovery are unspec…
- Assurance `SR08.18` — No staged installation or populated-store upgrade validation is specified.
- Assurance `SR08.21` — The unmapped suite mode is regularly exercised in principle, but drift and safe-return checks are unspecified.
- Assurance `SR08.24` — Existing upstream restoration handles config hashes; isolation from ordinary workload testing in unsafe mode…
- Assurance `SRE20.02` — ADR 0010:55–60 adds an authorisation dependency without distinguishing unavailable, starting or stalled plugi…
- Assurance `SRE20.03` — Plugin shutdown ordering and handling of in-flight Docker requests are unspecified.
- Assurance `SRE20.04` — Existing vesper-services readiness marks daemon startup; plugin readiness and effective remapping verificatio…
- Assurance `SRE21.01` — Fail-closed rejection is specified at ADR 0010:58–59; behaviour when the plugin saturates is not established.
- Assurance `SRE21.02` — No resource measurements for the new per-request authorisation process.
- Assurance `SRE21.04` — No evidence that malformed or oversized authorisation requests are rejected with bounded resource cost.
- Assurance `SRE21.07` — Plugin concurrency, memory bounds and saturation behaviour are unspecified.
- Assurance `SRE21.12` — The spike measures confinement outcomes, not Docker throughput and latency with authorisation enabled.
- Assurance `SRE22.01` — Docker-to-plugin failure coupling is acknowledged by fail-closed behaviour, but stalls and resource feedback…
- Assurance `SRE22.02` — No capacity or failure-load evidence for the new authorisation dependency.
- Assurance `SRE22.03` — Plugin queue and concurrent-handler bounds are unspecified.
- Assurance `SRE22.06` — No authorisation call or plugin-handler deadline is specified.
- Assurance `SRE22.07` — Cancellation of authorisation work after Docker clients disconnect is unspecified.
- Assurance `SRE22.08` — Unresponsive-plugin behaviour and isolation from unrelated Docker requests are unproven.
- Assurance `SRE22.10` — Startup dependencies are proposed, but no dependency graph proves the plugin can start without Docker API acc…
- Assurance `SRE22.11` — ADR 0010:123–129 requires route and suite checks but does not specify plugin crash, stall or invalid-configur…
- Assurance `SRE22.12` — No plugin resource allocation or testing within that allocation is recorded.
- Assurance `SRE22.13` — No plugin saturation-and-recovery test is specified.
- Assurance `SRE22.15` — BuildKit and Compose are explicitly unknown at ADR 0010:116–117; stalled authorisation under these clients is…
- Assurance `SRE22.16` — HEALTH will report confinement mode, but plugin liveness versus effective serving readiness is unspecified.
- Assurance `SRE22.17` — Existing supervise-daemon recovery is documented; recovery of the proposed plugin and Docker dependency is no…
- Assurance `SRE22.19` — The security improvement adds a daemon-wide synchronous failure dependency without failure-load evidence.
- Assurance `SREB.01` — Subordinate-range semantics, configuration validation and verification of effective enforcement are unspecifi…
- Assurance `SREB.04` — HEALTH mode reporting is proposed; owned diagnostics for plugin failures and denials are unspecified.
- Assurance `SREB.07` — The new authorisation service has no documented bounded queues, deadlines or tested overload refusal.
- Assurance `SREB.10` — No rehearsal of plugin outage, failed boot configuration or interrupted unsafe-mode restoration is specified.

**Isolation and recovery** → SRE and security
- Gap `RULE.09` · Practice rules F1 · ✓ verified — Switching stores silently replaces existing named volumes with empty ones
- Gap `SR09.01` · Practice security-design-2 F1 · ✓ verified — Migration strands persistent volumes and permits silent replacement with empty volumes
- Gap `SR09.06` · Practice security-design-2 F1 · ✓ verified — Migration strands persistent volumes and permits silent replacement with empty volumes
- Gap `SR09.19` · Practice security-design-2 F1 · ✓ verified — Migration strands persistent volumes and permits silent replacement with empty volumes
- Gap `SR16.08` · Practice security-operations-1 F1 · ✓ verified — Named volumes cannot be recovered by pulling or rebuilding images
- Gap `SR16.10` · Practice security-operations-1 F1 · ✓ verified — Named volumes cannot be recovered by pulling or rebuilding images
- Gap `SRE26.01` · Practice sre-integrity F1 · ✓ verified — Treat existing volumes as user data, not rebuildable image state
- Gap `SRE26.04` · Practice sre-integrity F1 · ✓ verified — Treat existing volumes as user data, not rebuildable image state
- Gap `SRE26.26` · Practice sre-integrity F1 · ✓ verified — Treat existing volumes as user data, not rebuildable image state
- Gap `SRE26.28` · Practice sre-integrity F1 · ✓ verified — Treat existing volumes as user data, not rebuildable image state
- Assurance `SR09.02` — Overlay build and normal startup paths exist; emergency deployment and rollback of this configuration are uns…
- Assurance `SR09.09` — The suite switch is to appear in HEALTH, but effective mapping, policy identity and transition history are un…
- Assurance `SR09.14` — Overlay configuration and subordinate ranges describe desired state; deployed-state verification remains unde…
- Assurance `SR09.15` — Service supervision exists, but idempotent plugin activation and partial-install recovery are not designed.
- Assurance `SR09.18` — The old store is retained, but no verified backup or restore procedure is specified.
- Assurance `SR09.21` — Recovery tests for interrupted migration, plugin failure and unsafe-mode restoration are missing.
- Assurance `SR09.24` — Agent reachability is supported by existing seccomp/transport evidence; strongest accepted remapped configura…
- Assurance `SR09.26` — Moving operator procedures to the agent is planned; updated runbooks and usability validation are absent.
- Assurance `SR09.27` — Overlay provisioning can exercise configuration installation, but clean/populated-disk transition checks are…
- Assurance `SR16.01` — The exception mode and retained old store acknowledge recovery concerns, but return-to-normal criteria are ab…
- Assurance `SR16.09` — Agent access exists independently of Docker; authorisation-service failure and operator recovery have not bee…
- Assurance `SR16.13` — Existing persistence tests cover ordinary reboots; migration, rollback and effective confinement after altern…
- Assurance `SR16.15` — Plugin absence is intended to fail closed, but crash, timeout, restart and partial-startup tests are not spec…
- Assurance `SR16.16` — ADR plans an adversarial container/share test; plugin bypass routes and accepted-flag combinations remain unp…
- Assurance `SR16.17` — Spike artifacts and GH milestones exist; trustworthy runtime results and tracked closure of the remaining bou…
- Assurance `SRE26.02` — ADR 0010:119–129 proposes confinement and regression suites, but specifies no recovery time or acceptable los…
- Assurance `SRE26.03` — ADR 0010:75–107 discusses compatibility, memory cost, kernel exposure and share privacy, but does not establi…
- Assurance `SRE26.05` — Keeping the old store at ADR 0010:107 is preservation evidence, but no tested procedure restores its applicat…
- Assurance `SRE26.06` — The proposal considers hostile flags and plugin absence, but not interrupted migration, mapping-range changes…
- Assurance `SRE26.10` — ADR 0010:107 retains the old store until owner-approved deletion, but provides no criterion for completing re…
- Assurance `SRE26.11` — The proposal combines namespace protection, private state-directory permissions and a confinement suite, but…
- Assurance `SRE26.14` — backend.go:97–107,167 onwards records and verifies rootfs-cache digests; the proposal does not establish equi…
- Assurance `SRE26.17` — Neither the spike nor the implementation milestones measure migration/restore time or space at expected volum…
- Assurance `SRE26.18` — The spike checks namespace access and basic share I/O; data-disk-suite.sh checks persistence. Neither demonst…
- Assurance `SRE26.20` — Committed spike output and proposed suite reports preserve evidence, and HEALTH will report the exception mod…
- Assurance `SRE26.21` — The current reboot suite checks persistence within one configuration. ADR 0010:123–129 does not explicitly re…
- Assurance `SRE26.22` — No evidence demonstrates recovery completeness, temporary disk capacity, ownership post-processing or progres…
- Assurance `SRE26.24` — ADR 0010 retains old state, but gives operators no procedure for distinguishing an unexpectedly empty new vol…
- Assurance `SRE26.27` — No new destructive multi-stage migration is specified, so a timing defect is unproven; quiescence, cutover co…

**Verification** → Test owner
- Gap `SR13.02` · Practice security-operations-1 F3 · ✓ verified — The spike records successful exit codes for refused container runs
- Gap `SR13.03` · Practice security-operations-1 F3 · ✓ verified — The spike records successful exit codes for refused container runs
- Gap `SR13.10` · Practice security-operations-1 F3 · ✓ verified — The spike records successful exit codes for refused container runs
- Gap `SRE17.09` · Practice sre-foundations-2 F1 · ✓ verified — The migration strands persistent volumes and allows divergent stores
- Gap `SRE17.10` · Practice sre-foundations-2 F2 · ✓ verified — The spike records the filter’s status as Docker’s status
- Gap `SRE17.13` · Practice sre-foundations-2 F1 · ✓ verified — The migration strands persistent volumes and allows divergent stores
- Gap `SRE17.21` · Practice sre-foundations-2 F1 · ✓ verified — The migration strands persistent volumes and allows divergent stores
- Assurance `RULE.11` — The spike supports several remapping observations and the ADR requires confinement and Docker suites; migrati…
- Assurance `SR13.04` — No hermetic plugin fake, protocol fixture or failure-injection seam is provided yet.
- Assurance `SR13.08` — Plugin input fuzzing or equivalent malformed-request testing is unspecified.
- Assurance `SR13.12` — Representative flags are listed, but “every accepted flag” is not yet a defined interaction matrix.
- Assurance `SRE17.02` — Runtime suites are planned; plugin parser/unit tests and failure-injection layers are not specified.
- Assurance `SRE17.03` — Existing targeted suites exist; cheap policy checks and resource-regression acceptance are unspecified.
- Assurance `SRE17.05` — Source and evidence are versioned; affected-dependency CI and implementation results do not yet exist.
- Assurance `SRE17.06` — HEALTH will report the exception mode; independent comparison with effective daemon mapping/plugin configurat…
- Assurance `SRE17.07` — Maximum accepted flags are planned; plugin failure load, storage growth and resource limits lack evidence.
- Assurance `SRE17.08` — No canary exposure, incubation or progressive rollout procedure is recorded.
- Assurance `SRE17.12` — OpenRC supervises existing daemons; plugin crash/restart and daemon/plugin startup-order tests are unspecifie…
- Assurance `SRE17.16` — Remapping configuration will be versioned; effective-configuration validation and transition coverage remain…
- Assurance `SRE17.18` — Plugin and launch switch have no parser/schema design; malformed bodies, omitted bodies and API path handling…
- Assurance `SRE17.19` — Confinement validation is planned; an independent validator of effective mapping and policy is unspecified.
- Assurance `SRE17.22` — No fake daemon/plugin compatibility strategy or supported version-skew contract is supplied.

**Delivery** → Release owner
- Gap `RULE.13` · Practice rules F1 · ✓ verified — Switching stores silently replaces existing named volumes with empty ones
- Gap `SR07.13` · Practice security-design-1 F1 · ✓ verified — Existing volume data has no migration or reconciliation path
- Gap `SR07.18` · Practice security-design-1 F1 · ✓ verified — Existing volume data has no migration or reconciliation path
- Gap `SRE07.03` · Practice sre-foundations-1 F1 · ✓ verified — The migration strands mutable volume data
- Gap `SRE07.06` · Practice sre-foundations-1 F2 · ✓ verified — The spike records successful status for failed Docker commands
- Gap `SRE07.10` · Practice sre-foundations-1 F3 · ✓ verified — The spike’s “restore” can delete pre-existing configuration and data
- Gap `SRE08.08` · Practice sre-foundations-1 F1 · ✓ verified — The migration strands mutable volume data
- Gap `SRE08.09` · Practice sre-foundations-1 F1 · ✓ verified — The migration strands mutable volume data
- Gap `SRE27.10` · Practice sre-runtime F1 · ✓ verified — The migration strands persistent volumes and provides no state-compatible rollback
- Gap `SRE27.13` · Practice sre-runtime F1 · ✓ verified — The migration strands persistent volumes and provides no state-compatible rollback
- Gap `SRE27.19` · Practice sre-runtime F1 · ✓ verified — The migration strands persistent volumes and provides no state-compatible rollback
- Gap `SREE.02` · Practice sre-runtime F1 · ✓ verified — The migration strands persistent volumes and provides no state-compatible rollback
- Gap `SREE.04` · Practice sre-runtime F2 · ✓ verified — A shared remap UID does not isolate other tasks’ persisted data
- Gap `SREE.05` · Practice sre-runtime F1 · ✓ verified — The migration strands persistent volumes and provides no state-compatible rollback
- Assurance `SR07.01` — The change is documented and test-gated, but staged rollout and before/after instrumentation are unspecified.
- Assurance `SR07.08` — The future suite must establish the invariant across routes; current spike examples do not establish complete…
- Assurance `SR07.09` — The overlay is the standard distribution path, but exception-mode inventory and maintenance rules are incompl…
- Assurance `SR07.11` — No early cohort or phased rollout success criteria are specified beyond eventual suite completion.
- Assurance `SR07.14` — Privileged workloads are named, but build, Compose and other application compatibility remain explicitly unre…
- Assurance `SR14.01` — Runtime threats are described; plugin build, installation, discovery and configuration-tampering threats rema…
- Assurance `SR14.02` — Design review is occurring; independent review enforcement and bypass controls are not evidenced by this diff.
- Assurance `SR14.03` — Existing overlay build and OpenRC startup are scripted; plugin packaging and service ordering remain unspecif…
- Assurance `SR14.04` — No exact plugin artifact verification or required-stage deployment gate is described.
- Assurance `SR14.09` — Build inputs for the new plugin and validation of the exception-mode field are not specified.
- Assurance `SR14.10` — Default versus upstream-suite policy is stated; environment suitability and effective-mode acceptance rules r…
- Assurance `SR14.13` — Existing overlay build path was read; exact new plugin toolchain/build inputs and reproducibility are not pro…
- Assurance `SR14.15` — Docker HTTP authorisation is proposed; BuildKit, Swarm, alternative runtimes and direct containerd access are…
- Assurance `SR14.16` — HEALTH reporting is planned, but effective-policy records, rejection logs and checks of existing objects are…
- Assurance `SR14.19` — Agent access offers a Docker-independent recovery path; recovery from a broken plugin/configuration has no de…
- Assurance `SRE07.02` — Kernel enforcement is appropriate; plugin lifecycle and exceptional-mode restoration remain procedural.
- Assurance `SRE07.04` — Upstream-suite exception is identified; exceptional-boot recovery tests are absent.
- Assurance `SRE07.05` — Existing daemon restart checks do not establish plugin or confinement recovery.
- Assurance `SRE07.07` — Boot/configuration idempotency and repeated plugin failure handling are unspecified.
- Assurance `SRE07.08` — Agent replacement is explicit; peer reachability and new command authority require verification.
- Assurance `SRE07.09` — HEALTH reports mode by design; effective policy, fallback, and recovery drills are missing.
- Assurance `SRE08.02` — Overlay and suites are planned; no complete safe migration workflow exists.
- Assurance `SRE08.03` — Platform versions are named; plugin build/package reconstruction is not yet specified.
- Assurance `SRE08.05` — No deployed plugin/policy identity is specified.
- Assurance `SRE08.07` — Docker/confinement regression gates are planned; exact packaged combination remains untested.
- Assurance `SRE08.10` — HEALTH mode field is planned; actual mapping/plugin/configuration skew checks are unspecified.
- Assurance `SRE27.06` — Plugin service integration is proposed, but monitoring and effective-readiness integration are unspecified.
- Assurance `SRE27.08` — Plugin absence is covered; hangs, malformed configuration and recovery remain unproven.
- Assurance `SRE27.14` — No bounded rollout cohort, observation period or automatic halt condition is specified.
- Assurance `SRE27.15` — ADR 0010:67–71 scopes the unsafe switch to a boot and reports it, but isolation from normal state and interru…
- Assurance `SRE27.17` — No nonlinear failure-load test for plugin latency, denial logging or concurrency is specified.
- Assurance `SREE.01` — Docker workflow reruns are planned, but authorisation overhead and shared-service resource impact are unmeasu…
- Assurance `SREE.03` — HEALTH mode visibility is planned; end-to-end confinement health and bounded denial-log delivery are unspecif…
- Assurance `SREE.06` — Plugin recovery and unsafe-suite contingencies are unspecified. No code, tests, Docker or System VM operation…

**Operation** → On-call lead
- Gap `SR15.03` · Practice security-operations-1 F3 · ✓ verified — The spike records successful exit codes for refused container runs
- Gap `SR15.07` · Practice security-operations-1 F2 · ✓ verified — Owner-only directories do not protect directly bind-mounted state files
- Gap `SRE12.08` · Practice sre-foundations-2 F2 · ✓ verified — The spike records the filter’s status as Docker’s status
- Gap `SRE12.09` · Practice sre-foundations-2 F3 · ✓ verified — The spike cannot reliably restore the prior daemon configuration
- Gap `SRE13.02` · Practice sre-foundations-2 F1 · ✓ verified — The migration strands persistent volumes and allows divergent stores
- Assurance `RULE.10` — ADR 0010:69–70 proposes HEALTH mode reporting; effective mapping/plugin verification, denial evidence and mon…
- Assurance `RULE.14` — GH #194 and TD-061 give the work an owner and destination; operational recovery from plugin outage, interrupt…
- Assurance `SR15.05` — Spike reproduction exists, but isolated plugin, ownership and migration reproductions are still missing.
- Assurance `SR15.10` — Existing service logs and HEALTH were read; new authorisation and mode-transition event schemas are absent.
- Assurance `SR15.12` — Plugin request logging and retention are unspecified; Docker bodies and headers require an explicit minimisat…
- Assurance `SR15.13` — Confinement checks are planned, but detection of wrong mode or missing enforcement and its triage path are un…
- Assurance `SR15.16` — Root operations move to the existing agent; compatibility under accepted flags and plugin-outage recovery rem…
- Assurance `SRE06.01` — Existing logs and HEALTH provide foundations; plugin diagnostics are unspecified.
- Assurance `SRE06.02` — Fail-closed intent is simple; diagnosis of unavailable policy service is not defined.
- Assurance `SRE06.03` — External confinement tests and internal HEALTH are planned, without combined evidence.
- Assurance `SRE06.04` — HEALTH checks responsiveness; effective confinement and incorrect-success detection need evidence.
- Assurance `SRE06.05` — No disk-headroom check is proposed for coexistence of old and new stores.
- Assurance `SRE12.01` — ADR 0010 distinguishes proposed work and unknowns; module-autoload causality lacks before/after evidence.
- Assurance `SRE12.02` — Spike tests several flag combinations, but does not control prior module state or prove general virtio-fs UID…
- Assurance `SRE12.03` — GH #194, TD-061 and GH160 history are linked; issue contents and shared investigation ownership were not veri…
- Assurance `SRE12.05` — Agent HEALTH and LOGS exist; effective remapping, plugin health and denial diagnostics remain unspecified.
- Assurance `SRE12.11` — Root-file and bound-namespace denials are recorded; module-autoload attribution is not independently establis…
- Assurance `SRE12.12` — ADR names Compose, build and crossarch uncertainty; realistic workload/data-shape coverage remains outstandin…
- Assurance `SRE13.04` — No bounded rollout or mapped/unmapped combination validation is specified.
- Assurance `SRE13.06` — Re-pull/rebuild is proposed without disk-space, duration or offline-recovery evidence.

**Organisation** → Engineering manager
- Gap `SR21.13` · Practice security-operations-2 F1 · ✓ verified — Named-volume data has no migration or rollback path
- Gap `SR21.14` · Practice security-operations-2 F1 · ✓ verified — Named-volume data has no migration or rollback path
- Gap `SR22.01` · Practice security-operations-2 F1 · ✓ verified — Named-volume data has no migration or rollback path
- Gap `SRE31.14` · Practice sre-organisation F1 · ✓ verified — Treat persistent volumes separately from rebuildable images
- Gap `SRE32.04` · Practice sre-organisation F2 · ✓ verified — Protect private state outside `/run/vesper`
- Gap `SRE33.05` · Practice sre-organisation F2 · ✓ verified — Protect private state outside `/run/vesper`
- Gap `SRE34.03` · Practice sre-organisation F1 · ✓ verified — Treat persistent volumes separately from rebuildable images
- Assurance `SR19.04` — ADR lines 123–129 require runtime and compatibility suites, but staged rollout, release-health criteria and a…
- Assurance `SR20.02` — The proposed authorisation service is security infrastructure; its protocol handling, review and enforcement…
- Assurance `SR20.05` — Repository-owned configuration and automation are proposed, but validation of configuration drift and direct…
- Assurance `SR21.02` — The ADR is proposed and its owner is named; completed independent approval of the future enforcement implemen…
- Assurance `SR21.11` — Docker-suite reruns are planned, but fresh-disk, existing-disk and mixed-mode rollout evidence is absent.
- Assurance `SR21.12` — ADR lines 67–71 propose a host-controlled bypass and health reporting; audit, isolation and interrupted-run r…
- Assurance `SR21.17` — D-034 records the mechanism decision and GH #53 retains the stronger boundary; accountable acceptance of the…
- Assurance `SRE31.02` — The spike connects observations to configuration, but no completed production review or shared review outcome…
- Assurance `SRE31.05` — GH #194 and dependent work are named; implementation follow-through is not evidenced.
- Assurance `SRE31.15` — Compatibility reruns are planned; plugin resource cost, startup impact, duplicated-store capacity and release…
- Assurance `SRE32.03` — Planned tests cover relevant failures, but the complete readiness checklist and emergency recovery procedure…
- Assurance `SRE32.08` — HEALTH reporting and plugin fail-closed behaviour are specified, but supervision, diagnostics and recovery ha…
- Assurance `SRE32.12` — ADR 0010:123–129 promises automated confinement and compatibility checks; those checks do not yet exist.
- Assurance `SRE33.03` — Retained old storage plus rebuilt images consumes additional disk space; no sizing or capacity acceptance evi…
- Assurance `SRE33.04` — The spike records selected isolation behaviour; representative mixed-boot, recovery and complete confinement…
- Assurance `SRE33.07` — Automatic fail-closed authorization is specified, but outage, restart and recovery tests are absent.
- Assurance `SRE33.09` — Agent-based administration is proposed, but authorization-plugin failure recovery and safe restart authority…
- Assurance `SRE34.02` — Existing Docker and agent interfaces are reused; validated fallback and operator recovery remain unproven.
- Not evaluated (2): SR21.03, SRE33.08

### Coverage
A gap means nobody reviewed the lens, not that it is fine.

| Lens | Status | Evidence |
| --- | --- | --- |
| Scalability and performance | ▲ gap | Antigravity's only finding (F3) false |
| Concurrency | ▲ gap | Plugin and dockerd restart race unexamined |
| Readability and idioms | ▲ gap | Cross-read only; check_docs.py not run |
| Security | covered | #1, #4, #7, #11 |
| Privacy | covered | #6; boundlog run by Codex |
| Supply chain | covered | No deps or lockfile changes in diff |
| Correctness | covered | #10 |
| API and compatibility | covered | #1 |
| Rollout and migration | covered | #2, #3, #5 |
| Reliability | covered | #9, #13 |
| Observability | covered | #15 |
| Testing | covered | #10, #12 |
| Design and maintainability | covered | #8, #14 |

Areas no reviewer reached: docker-proxy published ports under remapping, the binfmt and cross-architecture path, binds of `/run/docker.sock` and the containerd socket, collisions of the subordinate uid range with other overlay users, and nftables and tap use inside container network namespaces.

### Consensus
| # | Codex | Claude | Antigravity | Practices | Judge |
| --- | --- | --- | --- | --- | --- |
| 1 | ● | ● | ○ | ○ | ✓ |
| 2 | ○ | ● | ● | ○ | ✓ |
| 3 | ○ | ● | ○ | ● | ✓ |
| 4 | ○ | ● | ● | ● | ✓ |
| 5 | ○ | ● | ● | ○ | ✓ |
| 6 | ● | ○ | ● | ● | ✓ |
| 7 | ○ | ○ | ○ | ● | ✓ |
| 8 | ○ | ● | ● | ○ | ✓ |
| 9 | ○ | ○ | ● | ○ | ✓ |
| 10 | ○ | ● | ● | ○ | ✓ |
| 11 | ○ | ○ | ● | ○ | ? |
| 12 | ○ | ○ | ○ | ● | ✓ |
| 13 | ○ | ○ | ○ | ● | ✓ |
| 14 | ○ | ● | ● | ○ | ✓ |
| 15 | ○ | ○ | ● | ○ | ✓ |

The reviewers disagreed mainly on severity. Practice reviewers rated #3 High, and Antigravity rated #11 Critical; the judge rated both Medium.

Dropped as false: Antigravity F3 (dockerd never sends tar or ≥1 MiB bodies to plugins). Also dropped: the context-file "do not relitigate" findings (Codex F3, security-design-2 F3, security-operations-1 F4, security-operations-2 F2, sre-foundations-1 F4, sre-foundations-2 F4, sre-organisation F3, sre-runtime F3, sre-state F1). That text is in the reviewers' prompt material, not a defect in the change.
● flagged · ○ not flagged · ✓ verified · ? unverified · ✗ false

### Strengths
- Remapping is the right primary mechanism. No reviewer found evidence against it, and a flag deny-list would have been weaker.
- The spike supports the central claim: dockerd refuses `--privileged` and host namespaces, and container root maps to 100000.
- Jailer uids 10000 and 20000 sit outside the 100000–165535 range. Pin the range and assert this in a test.

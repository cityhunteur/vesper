## Code Review Council Report

**✅ Ready to merge** · 0 blockers
🔴 0 Critical · 🟠 0 High · 🟡 4 Medium · 🔵 9 Low
`systemvm/overlay/usr/local/bin/vesper-vesperd` + guest init, Rust CLI, egress suite · POSIX sh, iptables, Rust
Agents: Codex · Claude · Antigravity · Judge

The verdict depends on one fact: none of the reviewers found a path to a denied destination, and every verified defect fails closed. Still, #1 breaks ADR 0008's own 1,024-flow limit, and the egress suite cannot detect rules that dockerd drops (#3), so fix both before the next parity claim. The opt-in `--network nat` flag, the `none` default and the way mixed helper versions fail closed are all solid.

### Next steps
1. **Fix #1 first**: it contradicts ADR 0008 line 66, and ADR line 99 states the same wrong rule order → `grep -n -e "dport 53" -e "connlimit" systemvm/overlay/usr/local/bin/vesper-vesperd`
2. **Fix #3** before you cite the egress suite as runtime evidence → `sed -n '75,79p;239,258p' src/control/suites/egress.rs`
3. **Get runtime evidence**: nobody ran anything against a live System VM. Run `egress-suite` twice in one boot, then flood port 53 from a `nat` guest and run `conntrack -C`
4. **Follow up** after merge: #2, #4, #5–#13

### 🟡 Medium

**#1 · DNS flows bypass the per-lease 1,024-flow `connlimit` in both layers**
`vesper-vesperd:323-326`, `:389-392` · Security / Reliability · Codex F1, Claude F1, Antigravity F1 · ✓ verified

Both chains accept `resolver:53` before the `connlimit` rule runs, so DNS flows are never counted. One guest can therefore fill the System VM's shared conntrack table and the masquerade ports, which breaks the ADR's resource-isolation guarantee. The judge set this to Medium: Codex rated it High and Antigravity Critical, but nobody measured the exhaustion. Codex's cited reproducer `review-artifacts/adr0008_dns_limit.py` is not in the worktree.
- **Verify:** `grep -n -e "dport 53" -e "connlimit" systemvm/overlay/usr/local/bin/vesper-vesperd`. The port-53 accepts appear above `connlimit` in both chains.
- **Fix:** Move the `connlimit` drop above the DNS accepts in `VESPERD-EGRESS` and `VESPERD-GUEST`, then bump the root-rule signature so existing installs rebuild. Correct ADR line 99 in the same change, and add a suite probe that opens more than 1,024 flows.

**#2 · A resolver or `eth0` subnet change on a new launch breaks DNS for running `nat` leases**
`vesper-vesperd:314-335`, `:408`, `:894-895` · Reliability · Antigravity F4 · ✓ verified

The shared root chain is rebuilt whenever its signature changes. Leases that are already running still DNAT to the old resolver, which sits in `DENY4`, so their DNS is dropped and `isolation` keeps reporting `egress_ok:false` for them. The fault fails closed, so it costs availability, not isolation. The judge set it to Medium; the reviewer rated it High.
- **Verify:** `sed -n '314,335p;893,898p' systemvm/overlay/usr/local/bin/vesper-vesperd`
- **Fix:** Under `$LOCK`, when the signature changes, refresh `VESPERD-GUEST`, the namespace DNAT rules and `$NETDIR/<id>` for every active lease. Or refuse the new launch until the old leases drain.

**#3 · The `root_layer_alone_after_docker_restart` step cannot fail**
`src/control/suites/egress.rs:239-258`; `vesper-vesperd:997-999`, `:1035` · Testing · Antigravity F5, Claude F4 · ✓ verified

The step restarts dockerd before `boot`. `boot` sends `LAUNCHAPI`, which reinstalls every root rule, so the step cannot see rules that dockerd wiped. `test-open-ns` flushes only `filter FORWARD`, so the namespace's `rp_filter` and MASQUERADE still act and the raw anti-spoof rule is never exercised. `root == 0` reads a global counter that only ever grows, and nothing checks that dockerd came back.
- **Verify:** `sed -n '75,79p;239,258p' src/control/suites/egress.rs`
- **Fix:** Launch the lease first, then kill dockerd. Poll `HEALTH` until `docker_ok` recovers, with no `LAUNCHAPI` in between. In `test-open-ns`, also flush the namespace's `nat POSTROUTING` and set `rp_filter=0`. Assert before/after deltas on `ns`, `root` and `raw`.

**#4 · The egress suite hard-codes this host's vmnet addresses**
`src/control/suites/egress.rs:29-33` · Design and maintainability · Claude F4 · ✓ verified

The deny probes target literal `192.168.64.2` and `192.168.64.1`. On a host where DHCP hands out other addresses, those probes report `closed` whatever the policy is, so the module breaks its own promise that `closed` means "the policy dropped it".
- **Verify:** `grep -n "192.168.64" src/control/suites/egress.rs`
- **Fix:** Have the helper print `eth0`'s address and gateway (for example on the `POLICY` line), and build the probes from those values.

### 🔵 Low

- **#5** `vesper-vesperd:317-346` · Security · Antigravity F2 · ✓ verified (conditional) — the root layer has no `INPUT` drop and no `-o eth0` restriction, so a System VM address outside `DENY4` (a non-RFC 1918 Docker network, or a secondary `eth0` address) is reachable. This needs an unusual configuration, so the judge lowered it from High → add `-A INPUT -i vpd+ -j DROP` and `-A FORWARD -i vpd+ ! -o eth0 -j DROP` to the `*mangle` block.
- **#6** `vesper-vesperd:234-275` · API and compatibility · Claude F3, Antigravity F7 · ✓ verified — `$(( ))` reads leading-zero octets and prefixes as octal while `[ -le ]` reads them as decimal, so the overlap checks test a different address from the one applied. Only a direct `LAUNCHAPI` call reaches it, and it fails closed. The judge lowered Antigravity's Medium → in `ip2int` and the prefix checks, reject `0?*`.
- **#7** `vesper-vesperd:364-368` · Correctness · Claude F2 · ✓ verified — a tap named `veth<digits>` makes the peer lookup find two links, so every `nat` launch fails, and the "only veth link" comment is wrong → match on `@up.$n`, or refuse those tap names under `nat`.
- **#8** `vesper-vesperd:215-221`, `:372-375` · Reliability · Antigravity F8 · ✓ verified — `tap*` and `up.<N>` exist before `all`/`default` `send_redirects=0` is written, so they keep `send_redirects=1`, short of ADR line 90. Under `none`, `ipv6_off` is always false. The judge lowered it from Medium → set the sysctls at the top of `netns_up`, then set each existing interface explicitly.
- **#9** `vesper-vesperd:299-336`, `:994-1000` · Observability · Antigravity F9 · ✓ verified — `nat_root_ok` never checks `DOCKER-USER`; if `VESPERD-REPLY` is flushed, every later launch fails at `:348`; and `DROPS` leaves out the REPLY and INVALID counters. The judge lowered it from Medium → widen the integrity check and the counter list.
- **#10** `tests/vesperd_systemvm.rs:484-524` · Testing · Antigravity F10 · ✓ verified — the "hermetic" test reads the runner's `resolv.conf` and `eth0`; `judge` passes the allow probes when there is no internet; and `check_structure.py` has no entry for `egress.rs`. The judge lowered it from Medium → make the inputs injectable, fail on probes it cannot judge, and register the file.
- **#11** `vesper-vesperd:783-810` · Concurrency · Antigravity F6 · ✓ verified (partial) — the global lock spans `reap_pass`, `prepare`, the kernel copy and the netns setup. The reviewer's timings and the claim that launches exceed `READY_TIMEOUT` were not measured; the judge lowered it from Medium → fast-path with `nat_root_ok` and narrow the lock.
- **#12** `docs/SECURITY.md:161`, `systemvm-bringup.md:501-505`, `vesper-agent:23`, `vesper-vesperd:38-45`, `:754` · Design and maintainability · Claude F5 · ✓ verified — these still give the `LAUNCHAPI` grammar and helper usage header from before ADR 0008 (invariant 7), and `net=none net=nat` is accepted → update them, and track whether `net=` was already given in its own flag.
- **#13** `vesper-vesperd:421-428` · Scalability and performance · Antigravity F3 · ? unverified — `nat_down` does not flush the lease's root conntrack entries. The claims about kernel behaviour (5-day entries, masquerade cleanup) are not shown; the judge lowered it from High → run `conntrack -L -s 198.18.x.2` after stopping a lease that held open flows.

### Coverage
A gap means nobody reviewed the lens, not that it is fine.

| Lens | Status | Evidence |
| --- | --- | --- |
| Security | covered | #1, #5; thin for the riskiest lens |
| Privacy | covered | Codex: resolver disclosure is accepted |
| Supply chain | covered | No dependency or lockfile changes |
| Correctness | covered | #6, #7; Claude traced the helpers |
| API and compatibility | covered | #6; old helper refuses new tokens |
| Rollout and migration | covered | Opt-in; initramfs stamp forces rebuild |
| Reliability | covered | #1, #2, #8 |
| Scalability and performance | covered | #11, #13; both unmeasured |
| Concurrency | covered | #2, #11; `pick_idx` reuse not checked |
| Observability | covered | #9 |
| Testing | covered | #3, #10 |
| Design and maintainability | covered | #4, #12 |
| Readability and idioms | covered | #7, #12 |

Not reached by any lens: runs against a live System VM, link-local IPv6 in the namespace under `none`, and the reaper's handling of an orphaned `vpd` link whose record is gone.

### Consensus
| # | Codex | Claude | Antigravity | Judge |
| --- | --- | --- | --- | --- |
| 1 | ● | ● | ● | ✓ |
| 2 | ○ | ○ | ● | ✓ |
| 3 | ○ | ● | ● | ✓ |
| 4 | ○ | ● | ○ | ✓ |
| 5 | ○ | ○ | ● | ✓ |
| 6 | ○ | ● | ● | ✓ |
| 7 | ○ | ● | ○ | ✓ |
| 8 | ○ | ○ | ● | ✓ |
| 9 | ○ | ○ | ● | ✓ |
| 10 | ○ | ○ | ● | ✓ |
| 11 | ○ | ○ | ● | ✓ |
| 12 | ○ | ● | ○ | ✓ |
| 13 | ○ | ○ | ● | ? |

All three reviewers found #1 but rated it differently: Codex High, Claude Medium, Antigravity Critical. The judge sided with Claude.
Dropped as false: none.
● flagged · ○ not flagged · ✓ verified · ? unverified · ✗ false

### Strengths
- `--network nat` is opt-in, and the `none` path adds only no-op teardown and empty reaper loops.
- Both layers share the deny list through `DENY4`, and tracing found no path to a denied destination.
- A mixed helper version fails closed with `ERR usage` rather than silently ignoring the new tokens.

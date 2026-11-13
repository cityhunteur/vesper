## Code Review Council Report

**⛔ Do not merge** · 4 blockers
🔴 0 Critical · 🟠 4 High · 🟡 11 Medium · 🔵 1 Low · ▲ 1 coverage gap
`docs/design-docs/adr/0008-vesperd-microvm-egress.md` +1 more (2 files) · Design (Linux networking, iptables/nftables, busybox sh)
Agents: Codex · Claude · Antigravity · Judge

Option 2 is still the most secure choice: an opt-in per-lease veth with NAT, with `--network none` as the default. But ADR 0008 does not yet deliver its "enforced twice" claim. Four things break it: Docker takes back the top of `FORWARD`, teardown can leak a live veth, the deny list leaves holes, and the guest has no network driver to run the planned tests. Keeping `none` as the default, keeping all per-lease state out of the root namespace, and rejecting options 3–5 are all sound.

### Next steps
1. **Fix #1** before accepting the ADR → `grep -n "need localmount vesper-packages" systemvm/overlay/etc/init.d/vesper-agent systemvm/overlay/etc/init.d/vesper-services`
2. **Fix #2** before accepting the ADR → `grep -n -C 3 "umount -l" systemvm/overlay/usr/local/bin/vesper-vesperd`
3. **Fix #3** before accepting the ADR → `grep -n "198.18\|169.254\|100.64\|224\|240" docs/design-docs/adr/0008-vesperd-microvm-egress.md`
4. **Fix #4** before accepting the ADR → `grep -n "virtio_" systemvm/overlay/usr/local/bin/vesper-microvm systemvm/overlay/usr/local/bin/vesper-microvm-init`
5. **Close the gap** ▲ Privacy → review the DNS exception for name leakage, logging and retention, then record the accepted risk for owner approval (see #5).
6. **Follow up** in the ADR revision: #5–#15. Low: #16.

### 🟠 High

**#1 · The root-namespace filter depends on a `FORWARD` position that Docker takes back**
`docs/design-docs/adr/0008-vesperd-microvm-egress.md:88` · Design and maintainability / Reliability · Claude F1, Antigravity F3 · ✓ verified

`vesper-agent` and `vesper-services` both declare only `need localmount vesper-packages`. dockerd starts in a detached subshell under supervise-daemon and can be restarted with `KILL docker`. Each time Docker starts, it puts its jumps back at the top of `FORWARD`, so Docker's `ACCEPT` rules for published ports run before `VESPERD-EGRESS`. The second layer is then gone.
- **Verify:** `grep -n "need \|supervise\|setsid" systemvm/overlay/etc/init.d/vesper-services systemvm/overlay/etc/init.d/vesper-agent`. At runtime (not run): `VESPERD KILL docker`, then `iptables -S FORWARD` and check rule 1.
- **Fix:** Put the root-side drops where Docker's ordering cannot matter: `raw PREROUTING -i <vesperd-prefix>+`, or `mangle FORWARD`. Keep only the needed `ACCEPT` in `DOCKER-USER`. Have `launch` check that the rules exist and refuse to start (fail closed) if they do not.

**#2 · Teardown relies on the namespace dying to remove the veth, and frees the /30 anyway**
`docs/design-docs/adr/0008-vesperd-microvm-egress.md:92`; `systemvm/overlay/usr/local/bin/vesper-vesperd:184,196-201` · Correctness / Concurrency · Claude F2, Antigravity F6 · ✓ verified

`netns_down` only runs `umount -l`, and its own comment says the namespace "lives on until its last user goes". A `net` or `isolation` inspection holding the namespace open, or a Firecracker that survives SIGKILL, keeps the namespace alive. The root-side veth, its forwarding and its NAT then stay live. Meanwhile `rm -f "$LEASES/$tid"` runs unconditionally and `leftovers()` reports the lease CLEAN or ABSENT. The next lease can then reuse the same /30, which gives duplicate addresses and a path between leases.
- **Verify:** `grep -n "umount -l\|lives on\|rm -f \"\$LEASES/\$tid\"" systemvm/overlay/usr/local/bin/vesper-vesperd`
- **Fix:** In `teardown` and `reap_pass`, run `ip link del <root veth>` in the root namespace before `netns_down`. Add a `veth` check to `leftovers`. Do not free a /30 while its veth still exists. Allocate from a live scan of root veths, not only from lease records.

**#3 · "Public unicast only" is never defined, and the ranges the ADR does list leave holes**
`docs/design-docs/adr/0008-vesperd-microvm-egress.md:38,62-65,83,86` · Correctness / Security · Claude F5, Antigravity F2 · ✓ verified (Antigravity said Critical; the judge ruled High)

The deny list names only RFC 1918, `100.64/10`, `169.254/16` and `192.168.64/24`. It misses the ADR's own veth pool `198.18/15`, so in the lease namespace a guest can reach another lease's veth. It also misses `0/8`, `127/8`, `224/4`, `240/4` (which includes `255.255.255.255`) and the documentation ranges. No protocol limit is stated, so GRE, ESP, SCTP and every ICMP type are allowed.
- **Verify:** `grep -n -C 2 "Blocking every private, shared and link-local" docs/design-docs/adr/0008-vesperd-microvm-egress.md`
- **Fix:** Make the full RFC 6890 special-purpose table the normative deny list in the ADR, used word for word by both layers. Allow only TCP, UDP and ICMP echo. Correct the claim that `198.18/15` "collides with no real network".

**#4 · The guest initramfs has no `virtio_net`, so the in-guest test plan cannot run**
`docs/design-docs/adr/0008-vesperd-microvm-egress.md:94`; `systemvm/overlay/usr/local/bin/vesper-microvm:65` · Testing · Antigravity F9 · ✓ verified

`vesper-microvm prepare` packs no `virtio_net`, `net_failover` or `failover` modules, and `vesper-microvm-init` never loads them. `LAUNCHAPI` copies that same `initrd.gz` into the jail, so a `nat` guest has no `eth0`. The existing suite only checks links from the host side. Only one reviewer found this.
- **Verify:** `grep -n "virtio_" systemvm/overlay/usr/local/bin/vesper-microvm systemvm/overlay/usr/local/bin/vesper-microvm-init`
- **Fix:** Pack `failover`, `net_failover` and `virtio_net` into `initrd.gz`, load `virtio_net` in init, and add a network probe mode inside the guest.

### 🟡 Medium

**#5 · The hard-coded DNS exception `192.168.64.1:53` can be shadowed by a tap and leaks names**
`docs/design-docs/adr/0008-vesperd-microvm-egress.md:68-72,79-82` · API and compatibility / Reliability · Codex F2, Claude F8, Antigravity F5 · ✓ verified (Antigravity said High; the judge ruled Medium)

eth0 gets its address by DHCP, and the repo records both `.2` and `.3` leases, so the resolver address is not a constant. `--tap tap0=192.168.64.1/30` passes every stated check, but it makes the resolver address local to the namespace, where namespace `INPUT` silently drops new DNS. The exception also lets a guest enumerate split-DNS and VPN-internal names. Reaching a service on the host is on the AGENTS.md "Ask a human first" list. Antigravity's figures for vmnet subnet selection and the DNS denial-of-service are unsourced.
- **Verify:** `grep -n "lease of 192.168.64" docs/exec-plans/completed/2026-11-06-WOR-27-systemvm-image-pipeline.md`
- **Fix:** Have guests use the tap gateway as their resolver, DNAT its `:53` to the resolver read at launch, and fail closed when there is none. Reject tap subnets that overlap eth0's subnet, docker0, `198.18/15` or the resolver address, at both the CLI and `LAUNCHAPI`. Record the name leak as an accepted risk for owner approval.

**#6 · Filtering by address does not isolate VPN routes for public prefixes**
`docs/design-docs/adr/0008-vesperd-microvm-egress.md:63` · Security · Codex F1 · ✓ verified (Codex said High; the judge ruled Medium)

A VPN can route a globally routable prefix such as `11.0.0.0/24`. Both filters pass that destination, and macOS then sends it into the VPN. So the claim that RFC 1918 and `100.64/10` close "the developer's LAN and VPN" does not hold. The judge notes that Codex's Python check only tests its own premise, not the ADR.
- **Verify:** `sed -n '62,65p' docs/design-docs/adr/0008-vesperd-microvm-egress.md`, and compare it with a host where a VPN routes a public prefix.
- **Fix:** State this limit in the ADR. For strict isolation, offer an explicit egress proxy or gateway. Otherwise, require enforcement on the host side that covers LAN and VPN routes.

**#7 · The lease namespace's own stack is filtered once, not twice**
`docs/design-docs/adr/0008-vesperd-microvm-egress.md:30-32,85-90` · Security · Claude F6, Antigravity F1 · ✓ verified (Antigravity said Critical; the judge ruled Medium)

The ADR never mentions `OUTPUT`, so sockets that a compromised Firecracker opens get only the root-side filter. Root `VESPERD-EGRESS` drops only traffic *from* vesperd veths, and namespace `INPUT` covers only the taps. A docker0 container can therefore reach the namespace's veth address, though not the guest.
- **Verify:** `grep -n "OUTPUT" docs/design-docs/adr/0008-vesperd-microvm-egress.md` (returns nothing); `grep -n -C 3 "VESPERD-EGRESS" docs/design-docs/adr/0008-vesperd-microvm-egress.md`
- **Fix:** Set the namespace `OUTPUT` policy to `DROP`. Make `INPUT` default `DROP` on both taps and the veth, accepting only `lo` and `ESTABLISHED,RELATED`. In root, also drop `-o <vesperd-prefix>+` for anything that is not a reply.

**#8 · No `--ctstate INVALID` drop, so un-NATed packets can leave on eth0**
`docs/design-docs/adr/0008-vesperd-microvm-egress.md:86-87` · Reliability · Antigravity F4 · ✓ verified

The NAT table does not apply to packets in the `INVALID` state, and accepting "ICMP errors" without conntrack lets a guest send forged ICMP into the namespace.
- **Verify:** `grep -n -C 2 "accepts only replies and ICMP errors" docs/design-docs/adr/0008-vesperd-microvm-egress.md`
- **Fix:** Start `FORWARD` and `INPUT` in both namespaces with `-m conntrack --ctstate INVALID -j DROP`. Bind outbound traffic to `-i <tap> -s <tap_cidr>`. Accept ICMP on `INPUT` only as `ESTABLISHED,RELATED`. Set `rp_filter=1` and `accept_redirects=0`.

**#9 · IPv6 is closed by one sysctl in the namespace only, and nothing tests it**
`docs/design-docs/adr/0008-vesperd-microvm-egress.md:41,94-95` · Correctness / Security · Claude F7, Antigravity F10 · ✓ verified

In the root namespace, eth0 has a ULA address and the root-side veth will get an `fe80::` address, and no `ip6tables` rules exist. Antigravity also notes that a `--tap` name can collide with the unnamed namespace veth. Claude overstates one point: `conf.all.disable_ipv6` does reach interfaces that already exist.
- **Verify:** `grep -n -i "ipv6\|ip6" docs/design-docs/adr/0008-vesperd-microvm-egress.md` (returns line 41 only)
- **Fix:** Set the `ip6tables` policy to DROP in the namespace. In root, add `ip6tables` drops for `-i`/`-o <prefix>+` and set `disable_ipv6=1` on the root veth. Give the namespace veth a name that tap names cannot match (for example, one containing `.`). Add guest tests that try the System VM's `fe80::` and ULA addresses.

**#10 · Per-lease root rules cannot be named, reaped or scaled**
`docs/design-docs/adr/0008-vesperd-microvm-egress.md:88-93` · Design and maintainability / Scalability · Claude F3, Antigravity F7 · ✓ verified

Lease IDs can be 64 characters but interface names hold only 15. The orphan-netns reap calls only `netns_down`, so it cannot find which rules or /30 belong to an orphan. Per-lease rules also grow root `FORWARD` linearly with the number of leases; the ~900-rule figure is an estimate.
- **Verify:** `grep -n "le 64\|le 15\|orphan-netns" systemvm/overlay/usr/local/bin/vesper-vesperd`
- **Fix:** Have no per-lease root rules. Use one static set keyed on `-i vpd+ -s 198.18.0.0/15`, name each veth `vpd<pool-index>`, and have the reaper delete any `vpd*` link that no live lease record claims.

**#11 · The lease record cannot carry the /30, and `netns_up` runs outside the lock**
`systemvm/overlay/usr/local/bin/vesper-vesperd:152-154,560-563,587,678` · Correctness / Concurrency · Claude F4, Antigravity F7 · ✓ verified

`write_record` writes only `holder= conn= fc=`, and the record is rewritten in three places. `unlock` (line 560) runs before `netns_up` (line 563), so two concurrent launches can pick the same /30.
- **Verify:** `grep -n "write_record\|^  unlock$\|netns_up \"\$LID\"" systemvm/overlay/usr/local/bin/vesper-vesperd`
- **Fix:** Add a `net=` field to all three record writers and to `read_record`. Allocate the /30 and create the veth while holding the lock. Say this in the ADR.

**#12 · The `LAUNCHAPI` grammar change breaks three existing parsers**
`systemvm/overlay/usr/local/bin/vesper-vesperd:99-102,435-437,516`; `src/control/launchapi.rs:40,289` · API and compatibility / Rollout · Claude F9 · ✓ verified

The token `net=nat` is parsed as the ceiling and returns `ERR bad ceiling`. `launch_stage` skips only `tap=*`. `valid_tap` and `valid_tap_name` reject `=`, `.` and `/`, so `tap=NAME=ADDR/PREFIX` is rejected. Mixing old and new versions fails closed, but with misleading errors.
- **Verify:** `grep -n "stage=\*|tap=\*|'')\|tap=\*) continue" systemvm/overlay/usr/local/bin/vesper-vesperd`
- **Fix:** Use a separate token (`tapaddr=NAME:ADDR/PREFIX`). List all three parser sites in the ADR. Bump `AGENT_PROTOCOL_VERSION`, or have vesperd refuse `--network nat` against an agent that does not advertise it.

**#13 · The test plan cannot prove the policy and depends on the public internet**
`docs/design-docs/adr/0008-vesperd-microvm-egress.md:94-95` · Testing · Claude F10, Antigravity F9 · ✓ verified

The plan has five reachability probes and none of the attack paths raised above. "A public address answers" also makes the suite depend on the internet.
- **Verify:** `sed -n '94,95p' docs/design-docs/adr/0008-vesperd-microvm-egress.md`
- **Fix:** Add guest cases that assert drop-rule counters, not timeouts:
  - IPv6
  - spoofed sources
  - other leases' `198.18.x`
  - bogons and multicast
  - non-53 ports on the gateway
  - GRE and fragments
  - published Docker ports, before and after `KILL docker`
  - no leftovers after teardown and after kill-holder
  - a `none` lease is unchanged
  - two concurrent `nat` launches get distinct /30s

**#14 · Unbounded double conntrack lets one guest exhaust the System VM's tables**
`docs/design-docs/adr/0008-vesperd-microvm-egress.md:86` · Scalability and performance · Antigravity F8 · ? unverified

The judge confirmed the core: root conntrack is shared, there is no per-lease cap, and the package lock has neither `conntrack-tools` nor `ipset`. The vmnet table size and the collision window are unsourced.
- **Verify:** `grep -n "name: iptables\|conntrack\|ipset" docs/specs/systemvm-packages.lock.yaml`. Confirming exhaustion needs a runtime flood test (not run).
- **Fix:** Add per-lease `connlimit` and new-flow rate limits, lower the conntrack timeouts inside the namespace, and allocate /30s with a rotating cursor instead of lowest-free.

**#15 · Lifecycle sensors cannot see leaked veths or rules, or firewall drops**
`systemvm/overlay/usr/local/bin/vesper-vesperd:160-168` · Observability · Antigravity F11 · ✓ verified (the judge ruled Low–Medium)

`leftovers` checks only record, jail, cgroup, netns and process. `wait_absent` therefore passes even when a veth is left behind, and no allow or drop counters are exposed.
- **Verify:** `grep -n -C 4 "leftovers()" systemvm/overlay/usr/local/bin/vesper-vesperd`
- **Fix:** Report `veth` in `leftovers` and `status`. Add `ipv6_off` and `egress_filter_ok` to `isolation`. Have `net` print the policy and `iptables -nvx` counters, without LOG rules (see TD-055).

### 🔵 Low

- **#16** `adr/0008-vesperd-microvm-egress.md:101` · Design and maintainability · Claude F11 · ✓ verified — the ADR cites the untracked `run/GH-166/...` → cite the upstream Firecracker file by URL at commit `d83d72b` instead.

### Coverage
A gap means nobody reviewed the lens, not that it is fine.

| Lens | Status | Evidence |
| --- | --- | --- |
| Privacy | ▲ gap | Codex: "not checked"; DNS leak only in #5 |
| Security | covered | #3, #6, #7, #8, #9 |
| Supply chain | covered | Docs-only diff; no new dependency |
| Correctness | covered | #2, #3, #9, #11 |
| API and compatibility | covered | #5, #12 |
| Rollout and migration | covered | #12 mixed versions, #1 dockerd restart |
| Reliability | covered | #1, #5, #8 |
| Scalability and performance | covered | #10, #14; some figures unsourced |
| Concurrency | covered | #2, #11 |
| Observability | covered | #15 |
| Testing | covered | #4, #13 |
| Design and maintainability | covered | #1, #10, #16 |
| Readability and idioms | covered | Thin; "/16 … /30" should say prefix length |

No lens reached: DNS rebinding, ARP and neighbour behaviour on the tap, or whether Firecracker's seccomp filter already limits socket creation (this affects #7).

### Consensus
| # | Codex | Claude | Antigravity | Judge |
| --- | --- | --- | --- | --- |
| 1 | ○ | ● | ● | ✓ |
| 2 | ○ | ● | ● | ✓ |
| 3 | ○ | ● | ● | ✓ |
| 4 | ○ | ○ | ● | ✓ |
| 5 | ● | ● | ● | ✓ |
| 6 | ● | ○ | ○ | ✓ |
| 7 | ○ | ● | ● | ✓ |
| 8 | ○ | ○ | ● | ✓ |
| 9 | ○ | ● | ● | ✓ |
| 10 | ○ | ● | ● | ✓ |
| 11 | ○ | ● | ● | ✓ |
| 12 | ○ | ● | ○ | ✓ |
| 13 | ○ | ● | ● | ✓ |
| 14 | ○ | ○ | ● | ? |
| 15 | ○ | ○ | ● | ✓ |
| 16 | ○ | ● | ○ | ✓ |

The reviewers disagree on how secure option 2 can be. Claude and Antigravity say it is the most secure choice once hardened. Codex says only an external egress proxy fully isolates LAN and VPN routes (#6).
Dropped as false: Codex F3 (the context file is not in the worktree, and it is not a design flaw).
● flagged · ○ not flagged · ✓ verified · ? unverified · ✗ false

### Strengths
- `--network none` stays the default, so current behaviour is unchanged unless a caller opts in.
- Options 3 (shared L2) and 5 (LAN presence, entitlement change) are rejected for the right reasons.
- Mixing old and new versions of `LAUNCHAPI` fails closed in both directions.

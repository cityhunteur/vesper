---
status: accepted
owner: cityhunteur
last_reviewed: 2026-11-13
---

# 0008. Egress for `vesperd` microVMs: opt-in NAT to public addresses only

Date: 2026-11-13. Supersedes: none. Superseded by: none.

## Context and problem

Since D-027 a `vesperd --backend systemvm` microVM can have tap devices (`--tap NAME`). Each lease has its own network
namespace in the System VM (`/run/vesper/vesperd/netns/<id>`, joined by `jailer --netns`) that holds `lo` and the
taps and nothing else, so a guest reaches no network at all (TD-058). Upstream Firecracker leaves connectivity to
the operator, who typically gives the tap an address and adds an iptables `MASQUERADE` rule on the host. A
Firecracker client of `vesperd` has no host to do that on: the System VM is Vesper's.

What a guest could reach if it had a route, measured on 2026-11-13 in the System VM:

| Destination | Address | What it is |
|---|---|---|
| System VM | `eth0` (DHCP from vmnet: `192.168.64.2/24` here), `docker0` `172.17.0.1/16`, `127.0.0.1` | dockerd's bridge, published container ports; the agent itself is on vsock only |
| macOS host | the vmnet gateway (`192.168.64.1` here) | The NAT gateway, the System VM's only DNS resolver, and the host's services on that address |
| Containers | `172.17.0.0/16` | Docker's default bridge |
| Other microVMs | their namespaces | Unreachable today: no namespace is connected to another |
| The host's LAN and VPNs | RFC 1918, CGNAT, link-local, and any prefix a VPN routes | Reached through the vmnet NAT like any other address |
| The internet | public unicast | Reached through the vmnet NAT |

The guest runs untrusted code by design. The threat is a guest that uses the network to reach Vesper's own control
surfaces, the developer's Mac, the developer's LAN or VPN, cloud metadata or another microVM, or that exhausts the
System VM's connection tracking. Firecracker inside the namespace runs as uid 20000 with no capabilities, so it
cannot change the namespace's routes or rules.

## Decision

We will keep **no uplink as the default** (`vesperd --network none`), and add one opt-in policy,
`vesperd --network nat`. Under `nat`, a lease's namespace is joined to the System VM by one point-to-point veth pair
and masqueraded out of `eth0`. A guest may reach **public unicast IPv4 addresses over TCP, UDP and ICMP echo**, plus
DNS through its tap gateway, and nothing else. Every rule set is default-deny and is enforced in two places that do
not depend on each other: the lease's namespace (where it lives and dies with the lease) and the System VM's root
namespace (in the `raw` and `mangle` tables, which Docker does not manage). IPv6 is off on both sides of the veth.
Nothing is published inbound.

Council review (Codex, Claude, Antigravity, Opus judge; 2026-11-13) chose this option over the others and found 16
flaws in the first draft; each is answered below as a rule of the design.

## The policy

**Denied destinations** (RFC 6890 special-purpose IPv4, used word for word by both layers):
`0.0.0.0/8`, `10.0.0.0/8`, `100.64.0.0/10`, `127.0.0.0/8`, `169.254.0.0/16`, `172.16.0.0/12`, `192.0.0.0/24`,
`192.0.2.0/24`, `192.88.99.0/24`, `192.168.0.0/16`, `198.18.0.0/15`, `198.51.100.0/24`, `203.0.113.0/24`,
`224.0.0.0/4`, `240.0.0.0/4` (this includes `255.255.255.255`), and the System VM's own `eth0` subnet whatever DHCP
gave it.

**Allowed** from a guest, after the deny list: new TCP and UDP flows, and ICMP echo requests; replies to them
(conntrack `ESTABLISHED,RELATED`). Everything else is dropped, including `INVALID` packets (which NAT does not
rewrite), other IP protocols (GRE, ESP, SCTP) and other ICMP types.

**DNS.** The guest's resolver is its tap gateway address. The namespace DNATs port 53 (UDP and TCP) on that address
to the System VM's IPv4 resolver, read from `/etc/resolv.conf` at launch, and that one `address:53` is the only
private destination either layer accepts. A `nat` launch fails closed when no IPv4 resolver is configured. The
macOS resolver sees the guest's queries, including names that resolve to private addresses; the address filter
still blocks those addresses. That disclosure is an accepted risk, recorded in `docs/SECURITY.md`.

**Limits per lease.** At most 1024 tracked flows from one lease (`connlimit`, in both layers), so one guest cannot
fill the System VM's connection table.

**Not covered.** Address filtering cannot see where macOS sends a public address: a VPN that routes a public prefix
(for example a corporate `11.0.0.0/8`) is reachable from a `nat` guest. Strict isolation from such routes needs
enforcement on the Mac or an egress proxy; both are out of scope here and are listed in TD-058.

## How it is built

- `vesperd --network none|nat` (default `none`) adds `net=nat` to `LAUNCHAPI`; `--tap NAME:ADDR/PREFIX` gives the tap
  its gateway address as the token `tapaddr=NAME:ADDR/PREFIX` (upstream's `ip addr add 172.16.0.1/30 dev tap0`).
  Under `nat`, only taps with an address are routed. A tap subnet is inside RFC 1918, has a prefix length of 16 to
  30, and does not overlap another tap's subnet, `eth0`'s subnet, `172.17.0.0/16`, `198.18.0.0/15` or the resolver
  address; `vesperd` and the guest helper both check it. Every token parser (the ceiling check, `launch_stage`,
  the token loop) skips tokens it does not own, and an older guest helper refuses the new tokens with an `ERR`.
- **Addresses.** Veth links come from `198.18.0.0/16` (RFC 2544 benchmarking space; it appears on no network a
  System VM routes to, and the deny list covers it), one /30 per lease: index `N` gives the root end
  `198.18.(N/64).((N%64)*4+1)` and the namespace end `+2`. The index is allocated with the helper's lock held, by a
  rotating cursor over indexes that no `net` file and no live `vpd*` link uses, and is written to the root-only file
  `/run/vesper/vesperd/net/<id>` before the link exists; the file outlives the lease record until the link is gone.
- **Names.** The root end is `vpd<N>` and the namespace end is `up.<N>`; tap names cannot contain `.` or start with
  `veth` or `vpd`, so no tap can take or shadow the uplink's names. (busybox `ip` ignores `peer name`, so the pair is made inside the fresh namespace, where the
  only `veth*` link is the peer, which is renamed `vpd<N>` and moved to the root namespace.)
- **Namespace layer** (created under the lock, before the jailer): every lease's namespace, `none` or `nat`, gets
  `rp_filter=1`, `accept_redirects=0`, `send_redirects=0` and `disable_ipv6=1` (all, default and lo) before any
  other interface exists, so the taps and `up.<N>` inherit them; under `nat` also `ip_forward=1`, `iptables`
  policies `INPUT`, `FORWARD` and `OUTPUT` `DROP`, and `ip6tables` the same. `INPUT` and `OUTPUT` accept `lo` and
  replies only, so nothing in the namespace (Firecracker included) can open a connection. `FORWARD` drops `INVALID`,
  accepts replies, and sends `-i <tap> -s <tap subnet> -o up.<N>` to a chain that drops above 1024 flows first, then
  accepts the resolver's port 53, drops the deny list, and accepts TCP, UDP and ICMP echo. `nat` `PREROUTING`
  DNATs the gateway's port 53; `POSTROUTING` masquerades out of `up.<N>`. Both tables are replaced whole, from the
  lease's record, so a rebuild is atomic.
- **Root layer** (static rules, one set for every lease, installed by the first `nat` launch and checked by every
  one; a launch fails closed when they are missing and cannot be added):
  - `raw PREROUTING -i vpd+ ! -s 198.18.0.0/16 -j DROP` and per-veth `rp_filter=1` (spoofing);
  - `mangle PREROUTING -i vpd+ -j VESPERD-EGRESS`: drop `INVALID`; return replies; drop above 1024 flows per source;
    accept the resolver's port 53; drop the deny list; accept TCP, UDP and ICMP echo; drop the rest. It runs before
    routing, so it covers traffic to the System VM's own addresses as well as forwarded traffic;
  - `mangle INPUT -i vpd+ -j DROP` (nothing from a lease is for the System VM itself) and
    `mangle FORWARD -i vpd+ ! -o eth0 -j DROP` (nothing is forwarded anywhere but `eth0`), for System VM addresses or
    networks outside the deny list;
  - `mangle POSTROUTING -o vpd+`: drop anything that is not a reply, so the System VM, containers and other leases
    cannot open a connection to a lease;
  - `nat POSTROUTING -s 198.18.0.0/16 -o eth0 -j MASQUERADE`;
  - `filter DOCKER-USER` accepts `vpd+` to `eth0` and replies back. Docker owns the order of `FORWARD`; if a Docker
    restart drops these accepts, `nat` microVMs lose connectivity (fail closed), because every drop lives in `raw`
    and `mangle`;
  - `ip6tables` `raw PREROUTING -i vpd+` and `mangle POSTROUTING -o vpd+` drop, and `disable_ipv6=1` on each `vpd<N>`
    before it comes up.
- **Teardown** deletes `vpd<N>` in the root namespace (which removes both ends) before it unmounts the namespace, and
  keeps `/run/vesper/vesperd/net/<id>` until the link is gone; `leftovers` and `status` report `veth`. The reaper
  deletes any `vpd*` link and any `net` file that no live lease claims. A lease's index is free again only when its
  link is gone.
- **Observability.** `VESPERD net <id>` prints the policy, the addresses and the per-chain packet counters of both
  layers; `isolation` adds `ipv6_off` and `egress_ok` (the root rules are in place).
- **Guest.** The microVM initramfs carries `failover`, `net_failover` and `virtio_net`; the test init can configure
  `eth0` and run TCP, UDP, spoofed-source and DNS probes, each printed on the console.
- **Resolver changes.** The root chains carry the resolver and `eth0`'s subnet in a signature comment; when DHCP
  changes either, the next `nat` launch replaces the chains atomically and rebuilds every running lease's namespace
  tables for the new values, so their DNS keeps working.
- **Tests.** `make systemvm-egress-suite` (12 steps) judges every verdict from inside a guest, against TCP and UDP
  listeners on the System VM's addresses as the helper reports them, and compares both layers' drop counters before
  and after. One microVM has its namespace filter opened after dockerd was killed and respawned while it ran (no
  launch in between), to show the root `mangle` layer holds alone and Docker keeps `DOCKER-USER`; one also loses its
  masquerade and reverse-path check, to show the root `raw` rule drops sources outside the pool. The suite fails
  when the System VM has no internet, because then the allowed probes cannot be judged. Two flood steps (added
  2026-11-13, D-029) send 1,400 UDP flows from one guest: with both layers, the lease's namespace holds exactly 1,024
  and drops the rest; with the namespace filter opened, the root `connlimit` drops the excess.
- **Measured residual.** A stopped lease's conntrack entries stay in the System VM until their timeouts (30 s for
  UDP, 5 days for established TCP): the flood step sees the same count, 1,028, before and after teardown. The owner
  chose to count them rather than add `conntrack-tools` (D-029). They matter only if the same /30 is given again
  within that time, after the cursor has gone round 16384 leases, or if many leases each leave 1,024 established
  TCP flows behind against the table's 65,536.

## Alternatives considered

1. **No uplink only.** The most secure: nothing reachable. It leaves a client that needs a package mirror, a
   registry or an API with no way to get one. Kept as the default.
2. **Per-lease veth and NAT, default-deny in two layers (chosen, opt-in).** No two microVMs share an L2 segment or a
   namespace; per-lease rules die with the namespace; the root rules are static and outside Docker's tables.
3. **One shared bridge for all microVMs, NATed.** Every microVM shares an L2 segment, so one guest can ARP-spoof,
   scan or attack another. Rejected.
4. **Userspace networking (passt/pasta, slirp4netns) per microVM.** No kernel routing for the guest, but pasta holds
   its own tap where Firecracker needs a persistent tap it can open, and neither tool is in the pinned package set
   (a new runtime dependency is an owner decision). Rejected for now.
5. **An egress proxy only (HTTP CONNECT).** The guest gets no IP route at all; the proxy resolves names and refuses
   private answers, which also defeats DNS rebinding. It is the most restrictive design that still gives HTTPS, but
   it needs a new proxy program on the System VM and breaks every non-HTTP protocol, and it still cannot see VPN
   routes for public prefixes. Recorded in TD-058 as the next step for strict isolation.
6. **Bridged vmnet per microVM on the Mac.** Needs launcher and entitlement changes, widens what is host-visible and
   puts the guest on the LAN. Rejected.

## Rationale and consequences

Two independent default-deny layers mean one mistake does not open a path: the namespace layer is per lease and has
no Docker in it; the root layer filters before routing, in tables Docker does not touch. Denying the full
special-purpose table closes the System VM, the Mac, containers, other leases' veths, the LAN, CGNAT VPNs and
metadata addresses at once. What becomes harder: a `nat` guest cannot reach a private service (a LAN registry, a
VPN-only API). A wider policy would be a separate, owner-approved decision.

## References

- D-027, D-028, CE-0010 (closed), TD-058; `docs/SECURITY.md`, "Egress for `vesperd` microVMs".
- `systemvm/overlay/usr/local/bin/vesper-vesperd` (`netns_up`, `nat_up`, `nat_root_rules`, `teardown`, `reap_pass`).
- Firecracker v1.16.0 network setup guide:
  <https://github.com/firecracker-microvm/firecracker/blob/d83d72b710361a10294480131377b1b00b163af8/docs/network-setup.md>.
- RFC 6890 (special-purpose IPv4 address registries), RFC 2544 (`198.18.0.0/15`).

---
status: proposed
owner: cityhunteur
last_reviewed: 2026-11-13
---

# 0008. Egress for `vesperd` microVMs: opt-in NAT, public addresses only

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
| System VM | `192.168.64.2` (eth0), `172.17.0.1` (docker0), `127.0.0.1` | Its own services: dockerd's bridge, containerd, the port relay; the agent itself is on vsock only |
| macOS host | `192.168.64.1` | The vmnet NAT gateway, the System VM's only DNS resolver, and the host's services on that address |
| Containers | `172.17.0.0/16` | Docker's default bridge |
| Other microVMs | their namespaces | Today unreachable: no namespace is connected to another |
| The host's LAN and VPNs | RFC 1918, CGNAT, link-local | Reached through the vmnet NAT like any other address |
| The internet | public unicast | Reached through the vmnet NAT |

The guest runs untrusted code by design. The threat is a guest that uses the network to reach Vesper's own control
surfaces, the developer's Mac, the developer's LAN or VPN, or another microVM. Firecracker inside the namespace runs
as uid 20000 with no capabilities, so it cannot change the namespace's routes or rules.

## Decision

We will keep **no uplink as the default**, and add one opt-in policy, `vesperd --network nat`, that connects the
lease's own namespace to the System VM through one point-to-point veth pair and masquerades it out of `eth0`. A
guest on a `nat` microVM may reach **public unicast IPv4 addresses only**, plus DNS on the vmnet gateway, and
nothing on the System VM, the Mac, the LAN, containers or other microVMs. The filter is a default-deny allow-list,
enforced twice: in the lease's namespace (where it lives and dies with the lease) and in the System VM's root
namespace (so one mistake does not open a path). IPv6 is disabled in the namespace. Nothing is published inbound.

## Alternatives considered

1. **No uplink only (today).** The most secure: nothing reachable. It leaves a client that needs a package mirror,
   a registry or an API with no way to get one, so users would ask for the System VM to be opened by hand, which is
   worse. Kept as the default.
2. **Per-lease veth and NAT with a default-deny allow-list (chosen, opt-in).** Each microVM has its own path; no
   two microVMs share an L2 segment or a namespace; the rules for a lease are removed with its namespace. Costs one
   veth pair and a handful of rules per lease, and an address pool for the veth links.
3. **One shared bridge for all microVMs, NATed.** What most Firecracker setups do. Every microVM shares an L2
   segment, so one guest can ARP-spoof, scan or attack another, and per-microVM filtering needs ebtables. Rejected.
4. **Userspace networking (passt/pasta, slirp4netns) per microVM, unprivileged.** No kernel routing for the guest at
   all, which is attractive, but pasta creates and holds its own tap and Firecracker needs a persistent tap it can
   open, and neither tool is in the pinned package set (a new runtime dependency is an owner decision). Rejected
   for now; worth revisiting if Firecracker can be given pasta's tap fd.
5. **Bridged vmnet per microVM on the Mac.** Gives each guest a LAN presence. Needs launcher and entitlement changes
   and widens what is host-visible (an owner decision by AGENTS.md); it also exposes the LAN directly. Rejected.

## Rationale and consequences

Default-deny with a public-only allow-list is the narrowest policy that still lets a guest fetch from the internet.
Blocking every private, shared and link-local range closes the System VM, the Mac (`192.168.64.0/24`), containers,
the developer's LAN and VPN (RFC 1918, `100.64.0.0/10`) and cloud metadata addresses (`169.254.0.0/16`) in one
rule set. A public address can still be one the developer would not want reached (for example their own public
services); the policy cannot tell, and says so.

DNS: the only resolver the System VM has is the vmnet gateway, `192.168.64.1:53`, so a guest that may use names
must reach it. Allowing UDP and TCP port 53 to that one address is the single exception to "no private
addresses". The alternative, public resolvers only, fails on networks that block outbound DNS and leaks queries
outside the developer's resolver policy. The exception exposes the macOS DNS forwarder to guest queries, including
names that resolve to private addresses, which the address filter still blocks.

What becomes harder: a guest that needs a private service (a LAN registry, a VPN-only API) cannot reach it under
`nat`. That is deliberate; a wider policy would be a separate, owner-approved decision.

## Implementation and migration impact

- `vesperd --network none|nat` (default `none`) becomes `net=nat` on `LAUNCHAPI`; `--tap NAME[=ADDR/PREFIX]` gives a
  tap the gateway address the guest routes through, as upstream's `ip addr add 172.16.0.1/30 dev tap0`. Under `nat`,
  only taps with an address have a route; tap subnets must be RFC 1918, at most /16 and at least /30, and must not
  overlap each other.
- Veth links come from `198.18.0.0/16` (reserved for benchmarking, RFC 2544, so it collides with no real network),
  one /30 per lease, allocated under the helper's lock and recorded with the lease.
- In the namespace: forwarding on, `rp_filter` strict, the tap's address, the veth's address and a default route,
  `MASQUERADE` out of the veth, and a `FORWARD` chain that accepts tap to veth only for allowed destinations and
  replies back, and drops the rest. `INPUT` from the taps accepts only replies and ICMP errors.
- In the root namespace: one chain, `VESPERD-EGRESS`, jumped from the top of `FORWARD` (Docker's chains come later
  and are not touched), that accepts the lease's veth to `eth0` for allowed destinations and replies back, and drops
  everything else from a vesperd veth; `INPUT` drops everything from a vesperd veth; `MASQUERADE` for
  `198.18.0.0/16` out of `eth0`.
- Teardown removes the veth (deleting the namespace removes its end and the root-side peer) and the lease's root
  rules; the reaper does the same for leftovers.
- Tests: the vesperd suite boots a `nat` microVM and, from inside the guest, checks that a public address answers
  and that the System VM, the gateway (other than DNS), docker0, a private address and a second microVM do not.

## References

- D-027, CE-0010 (closed), TD-058; `docs/SECURITY.md`, "Network interfaces for `vesperd` microVMs".
- `systemvm/overlay/usr/local/bin/vesper-vesperd` (`netns_up`).
- Firecracker's network setup guide: `run/GH-166/firecracker/docs/network-setup.md` at `d83d72b`.

---
status: accepted
owner: cityhunteur
last_reviewed: 2026-11-22
---

# 0005. Published container ports reach localhost through vesper-proxy and a vsock gateway in the System VM

## Context and problem

Docker Desktop users expect `docker run -p 8080:80` to answer on `localhost:8080`. In Vesper, dockerd runs inside
the System VM, so its published ports exist on the guest, not on macOS. design-v0 fixes the ownership path as
`host -> vesper-proxy -> System VM gateway -> microVM` and asks for deterministic, observable forwarding with
clean conflict handling and teardown (GH #28).

## Decision

- `vesper-proxy --publish-ports` runs a publisher next to the Docker socket proxy. It follows container events
  on the Docker API, reconciles running containers at start, and for every TCP `-p` binding listens on the host
  loopback (`127.0.0.1:<host port>`; a specific loopback host ip is honoured, anything else is reported as
  unsupported) and forwards each connection through the launcher's vsock bridge (`CONNECT 2400`) to the System
  VM gateway `vesper-port-relay`, which reads `PORT <n>` and connects to `127.0.0.1:<n>` inside the guest, where
  dockerd's own mapping reaches the container. UDP is forwarded too, on loopback only (amendment below).
- Every change writes `run/vesper/ports.json` (the path, the gateway, each mapping with `listening`, `conflict`
  or `unsupported` and an error text), and the proxy log names every bind and release.
- The launcher ignores `SIGPIPE` and sets `SO_NOSIGPIPE` on bridge clients, so a proxy that dies with a live
  stream no longer takes the VM down.

### Amendment, 2026-11-22: UDP on loopback (TD-037, D-077; the owner approved UDP on loopback in D-066)

- A UDP `-p` binding on loopback gets one UDP socket on the host loopback and one bridge stream for all its flows:
  the preface is `CONNECT 2400` then `UDP <n>`, and `vesper-port-relay-conn` hands the stream to
  `vesper-udp-relay <n>` (Go, standard library only). A binding to any other host ip stays `unsupported`, as for TCP.
- Frames in both directions are `kind (1 byte) | flow id (4, big-endian) | length (2, big-endian) | payload`;
  `kind` is 0 (data) or 1 (close); a length over 65,507, an unknown kind or a close with a payload ends the stream.
  The frame carries no address: the guest relay sends every flow to `127.0.0.1:<n>` and nowhere else.
- A flow is one host client address. The host holds at most 1,024 flows per mapping, drops the least recently used
  one past that (sending a close) and closes a flow after 90 s without a datagram; the guest relay holds at most
  1,024 flows and closes one after 120 s. Each flow is one connected UDP socket in the guest. Replies leave from the
  published socket, so a client with a connected socket accepts them.
- A datagram that finds the bridge stream busy is dropped, never queued without bound; when the stream ends its
  flows are lost and the host dials again with a backoff of 1 to 30 s.
- The container sees the source address dockerd's userland proxy gives it, as for TCP, not the Mac client's.
- The published socket's send and receive buffers are raised, because macOS refuses a datagram larger than the send
  buffer (9,216 bytes by default).
- Each UDP mapping takes one bridge connection (of 512) and one `vesper-port-relay` handler (of 256), whatever its
  number of flows. Rejected: a stream per flow, which would let a client with random source ports fill the bridge
  cap shared with the Docker socket and the `vesperd` leases (D-075).

## Alternatives considered

- **Forward to the guest's NAT address over the shared vmnet.** It worked from `curl` but not from Python or the
  proxy process (`EHOSTUNREACH`, a macOS local-network policy effect), and it would make the feature depend on
  the host's network configuration. The bridge path has no such dependency and keeps the "no host-visible
  network" story of the ingress.
- **Let dockerd bind on the guest and document the guest IP.** Breaks the `localhost` expectation the issue is
  about.
- **A static vsock port per published port.** Would need a relay per port in the guest; the single gateway with a
  `PORT` preface is one supervised process.

## Rationale and consequences

- Published ports are the first host-visible TCP listeners Vesper opens; they are loopback-only and exist only
  while the container runs, and the status file makes the set auditable (SECURITY.md host exposure rules).
- Conflicts are visible at two levels: dockerd rejects a host port already published by another container, and
  the publisher records `conflict` when the macOS port is taken by anything else, leaving the container running
  inside the guest.
- Limits (TD-037): loopback only; TCP sets up one bridge connection per client connection; UDP counters are in the
  log, not in the status file.

## Implementation and migration impact

`src/ports.rs`, `src/ports/udp.rs`, `shim/cmd/vesper-udp-relay`, `shim/internal/udpframe`, `src/bin/vesper-proxy.rs` (`--publish-ports`, `--relay-port`, `--ports-status`),
`scripts/systemvm/docker-up.sh`, guest `etc/init.d/vesper-port-relay` and `usr/local/bin/vesper-port-relay-conn`,
`launcher/Sources/vesper-vmlauncher/main.swift` (SIGPIPE), `scripts/systemvm/ports-suite.sh`, `make ports-suite`.

## References

- GH #28 / WOR-34; design-v0 "deterministic port publishing ownership path"; ADR 0003 (bridge preface).

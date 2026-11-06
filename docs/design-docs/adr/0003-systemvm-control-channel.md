---
status: accepted
owner: cityhunteur
last_reviewed: 2026-11-06
---

# 0003. Control channel: launcher-served vsock bridge and a socat-backed guest agent

Date: 2026-11-06. Supersedes: none. Superseded by: none.

## Context and problem

GH #13 needs host-to-guest communication over vsock with a handshake, keepalive, reconnect and large payloads.
`vesper-proxy` already dials `vsock://<cid>:<port>` upstreams through a host Unix socket
`<dir>/<cid>.sock` with a Firecracker-style `CONNECT <port>\n` preface (WOR-71), but nothing served that socket.
Virtualization.framework exposes guest vsock only to the process that owns the VM, and the guest ships no
Vesper code beyond the overlay.

## Decision

We will serve the bridge socket from `vesper-vmlauncher` (`vsock_bridge_path` in the launch config, from the lock's
`control` section): it accepts a client, reads the `CONNECT <port>` preface, connects to that guest port with
`VZVirtioSocketDevice` and relays bytes both ways; the socket is created with mode 0600. In the guest, OpenRC
supervises `socat VSOCK-LISTEN:1024,fork EXEC:/usr/local/bin/vesper-agent`; the agent is a POSIX shell script
speaking line protocol version 1 (`VESPER-AGENT 1 <host>` greeting; `PING`, `VERSION`, `ECHO <n>`, `HEALTH`,
`DROP`, `SHUTDOWN`, `QUIT`). On the host, `src/control.rs` implements the client, `systemvmctl control` the CLI,
and `make systemvm-up` / `systemvm-down` keep the VM running between calls.

## Alternatives considered

- **A separate bridge daemon** on the host: cannot reach the guest's vsock, which belongs to the launcher's
  process.
- **A compiled guest agent** (Rust, cross-compiled): typed and fast, but a cross toolchain and a second build
  pipeline before any Phase 1 exit criterion is met; the shell agent carries the protocol until a workload needs
  more.
- **Reusing the console** for commands: works for the probe but is single-channel, unframed and not usable while
  the login shell is in use.

## Rationale and consequences

Everything stays inside the pinned image plus the overlay, with no new host dependency. The proxy's existing
bridge convention now has a server, so the Docker socket path (GH #15) can reuse it unchanged. Harder: the bridge is
a host-visible Unix socket that reaches any guest vsock port; it is owner-only (0600) and lives under `run/`, and
`vesperd`/`vesper-proxy` remain the only intended consumers (ARCHITECTURE invariant 1). The shell agent is a
line protocol, not an RPC schema; `ECHO` streams the payload back as it arrives, so clients must read while they
write (`control::Session::echo` does).

## Implementation and migration impact

`launcher/` (socket device, bridge, `--keep-running`, SIGTERM), `systemvm/overlay/etc/init.d/vesper-agent` and
`usr/local/bin/vesper-agent`, `src/control.rs`, `systemvmctl control …`, image lock schema 5 (`control`),
`scripts/systemvm/up.sh` / `down.sh`. ExecPlan:
[2026-11-06-WOR-25-systemvm-control-channel.md](../../exec-plans/completed/2026-11-06-WOR-25-systemvm-control-channel.md).

## References

- [ADR 0001](0001-systemvm-launcher-swift-helper.md), [ADR 0002](0002-systemvm-overlay-via-initramfs.md).
- [systemvm-bringup.md](../../process/systemvm-bringup.md) "Proxy bridge convention" and "Control channel".
- GitHub #13, #15; Linear WOR-25, WOR-23.

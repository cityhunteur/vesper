# Sourced by the handlers that socat runs for a vsock connection (the agent, the port relay, the share notifier).
# ADR 0010: these endpoints serve the macOS host, which connects from CID 2. A process inside the System VM that
# managed to connect (a container without a seccomp filter can open a vsock socket) is turned away. socat sets
# SOCAT_PEERADDR for the handler; a handler run by hand, as root, has no such variable and is not a connection.
vsock_host_only() {
  [ -z "${SOCAT_PEERADDR+set}" ] && return 0
  [ "$SOCAT_PEERADDR" = 2 ] && return 0
  echo "ERR this endpoint serves the macOS host only"
  exit 1
}

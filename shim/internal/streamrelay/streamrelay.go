// Package streamrelay relays one byte stream between a client (the host side of a vsock bridge connection, given as
// separate read and write ends) and a target socket in the System VM, with both half-closes passed on (TD-093):
//
//   - The client ends its writes (for example, `printf req | nc`): the target gets a half-close (CloseWrite) and the
//     relay keeps carrying the target's reply until the target ends it, however late (GH #167).
//   - The target ends its writes (dockerd exits, or a container's server closes a connection first): the client side
//     is closed at once and the relay returns, so the host sees the end of the stream.
//
// socat cannot do the second: with an STDIO or FD address it does not shut down its write side when the other
// address reaches EOF, so with the long timeout GH #167 needs, a stream whose server closed stayed open until the
// client closed too. That hung `docker events` after a dockerd restart, and with it the port publisher.
package streamrelay

import (
	"errors"
	"io"
	"net"
	"os"
	"syscall"
)

// HalfCloser is a target the relay can half-close: *net.UnixConn and *net.TCPConn.
type HalfCloser interface {
	net.Conn
	CloseWrite() error
}

// Relay copies in to target and target to out until the target ends its writes, then closes out (shutting down
// its write side first, when it is a socket, so a duplicate of the descriptor cannot keep the stream open) and
// returns. An end of in half-closes the target and the relay goes on. It returns the error that ended the
// target-to-client copy, or nil for a clean end.
func Relay(in io.Reader, out io.WriteCloser, target HalfCloser) error {
	go func() {
		_, _ = io.Copy(target, in)
		// The client has no more to send; the target may still answer.
		_ = target.CloseWrite()
	}()
	_, err := io.Copy(out, target)
	shutdownWrite(out)
	_ = out.Close()
	_ = target.Close()
	return err
}

// shutdownWrite shuts down the write side of out when it is a socket: closing one descriptor does not end a stream
// whose socket another descriptor (stdin, say) still holds.
func shutdownWrite(out io.Writer) {
	type closeWriter interface{ CloseWrite() error }
	if cw, ok := out.(closeWriter); ok {
		_ = cw.CloseWrite()
		return
	}
	if f, ok := out.(*os.File); ok {
		// ENOTSOCK for a pipe or a file: closing it is then enough.
		_ = syscall.Shutdown(int(f.Fd()), syscall.SHUT_WR)
	}
}

// Dial connects to a target named `unix PATH` or `tcp HOST:PORT`.
func Dial(network, address string) (HalfCloser, error) {
	switch network {
	case "unix", "tcp":
	default:
		return nil, errors.New("network must be unix or tcp")
	}
	conn, err := net.Dial(network, address)
	if err != nil {
		return nil, err
	}
	hc, ok := conn.(HalfCloser)
	if !ok {
		conn.Close()
		return nil, errors.New("connection cannot be half-closed")
	}
	return hc, nil
}

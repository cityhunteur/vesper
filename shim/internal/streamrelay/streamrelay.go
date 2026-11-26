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
	"time"
)

// HalfCloseLimit is the bound the program uses for how long the relay waits for the target to end after the client has ended its writes: the
// inner `socat -t 86400` it replaces ended a connection that long after either side ended. Without it a target
// that ignores the half-close (a silent attach, a server that never answers) would hold its socket for ever.
const HalfCloseLimit = 24 * time.Hour

// HalfCloser is a target the relay can half-close: *net.UnixConn and *net.TCPConn.
type HalfCloser interface {
	net.Conn
	CloseWrite() error
}

// DrainLimit bounds how long the relay goes on reading the client after the target has ended: see Relay.
const DrainLimit = 10 * time.Second

// Relay copies in to target and target to out until the target ends its writes, then shuts down out's write side
// (when it is a socket, so a duplicate of the descriptor cannot keep the stream open): the client sees the end at
// once. An end of in half-closes the target and the relay goes on. After the client's end the target has
// halfCloseLimit to end its reply (HalfCloseLimit in the program).
//
// When the target stops taking the client's bytes (it closed, often after refusing a large request early), the rest
// of in is read and dropped (TD-103). Without that, socat's next write to the relay failed and socat could end before
// it passed the target's answer to the host. After the target ends, the relay waits up to DrainLimit for in to end,
// then closes out and the target and returns the error that ended the target-to-client copy, or nil for a clean end.
func Relay(in io.Reader, out io.WriteCloser, target HalfCloser, halfCloseLimit time.Duration) error {
	inDone := make(chan struct{})
	go func() {
		defer close(inDone)
		if _, err := io.Copy(target, in); err != nil {
			_, _ = io.Copy(io.Discard, in)
			return
		}
		// The client has no more to send; the target may still answer, within halfCloseLimit.
		_ = target.CloseWrite()
		_ = target.SetReadDeadline(time.Now().Add(halfCloseLimit))
	}()
	_, err := io.Copy(out, target)
	shutdownWrite(out)
	// A write to the closed target fails, so a client still sending is drained from here on.
	_ = target.Close()
	select {
	case <-inDone:
	case <-time.After(DrainLimit):
	}
	_ = out.Close()
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

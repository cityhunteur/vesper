package streamrelay

import (
	"bufio"
	"io"
	"net"
	"os"
	"path/filepath"
	"testing"
	"time"
)

// client gives the relay its two ends as socat EXEC does: one socketpair for stdin, another for stdout. It returns
// the relay's in and out, and the test's ends of each.
func client(t *testing.T) (in *os.File, out *os.File, toRelay *os.File, fromRelay *os.File) {
	t.Helper()
	pair := func() (*os.File, *os.File) {
		fds, err := socketpair()
		if err != nil {
			t.Fatal(err)
		}
		return os.NewFile(uintptr(fds[0]), "a"), os.NewFile(uintptr(fds[1]), "b")
	}
	in, toRelay = pair()
	out, fromRelay = pair()
	return
}

// target listens on a Unix socket and runs serve on the one connection it accepts.
func target(t *testing.T, serve func(c *net.UnixConn)) HalfCloser {
	t.Helper()
	// A short directory: a Unix socket path is at most 104 bytes on macOS, and t.TempDir's names are long.
	dir, err := os.MkdirTemp("/tmp", "sr")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { os.RemoveAll(dir) })
	path := filepath.Join(dir, "t.sock")
	l, err := net.ListenUnix("unix", &net.UnixAddr{Name: path, Net: "unix"})
	if err != nil {
		t.Fatal(err)
	}
	go func() {
		c, err := l.AcceptUnix()
		l.Close()
		if err == nil {
			serve(c)
		}
	}()
	conn, err := Dial("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	return conn
}

// readAll reads from r until EOF, or fails the test after limit.
func readAll(t *testing.T, r io.Reader, limit time.Duration) string {
	t.Helper()
	done := make(chan string, 1)
	go func() {
		b, _ := io.ReadAll(r)
		done <- string(b)
	}()
	select {
	case s := <-done:
		return s
	case <-time.After(limit):
		t.Fatalf("no end of stream within %s", limit)
		return ""
	}
}

// The server writes and closes while the client still holds its write side open: the client sees the end at once.
// With socat this waited for the client (TD-093).
func TestServerCloseEndsTheStreamWhileTheClientStillWrites(t *testing.T) {
	in, out, toRelay, fromRelay := client(t)
	defer toRelay.Close()
	tg := target(t, func(c *net.UnixConn) {
		_, _ = c.Write([]byte("hi\n"))
		c.Close()
	})
	go func() { _ = Relay(in, out, tg) }()
	start := time.Now()
	if got := readAll(t, fromRelay, 3*time.Second); got != "hi\n" {
		t.Fatalf("got %q", got)
	}
	if time.Since(start) > 2*time.Second {
		t.Fatalf("the end took %s", time.Since(start))
	}
}

// GH #167: the client sends a request and half-closes at once; a reply 1 s later still arrives in full.
func TestClientHalfCloseStillGetsALateReply(t *testing.T) {
	in, out, toRelay, fromRelay := client(t)
	tg := target(t, func(c *net.UnixConn) {
		req, _ := io.ReadAll(c) // ends when the relay half-closes the target
		time.Sleep(time.Second)
		_, _ = c.Write([]byte("late:" + string(req)))
		c.Close()
	})
	go func() { _ = Relay(in, out, tg) }()
	_, _ = toRelay.Write([]byte("req"))
	_ = toRelay.Close()
	if got := readAll(t, fromRelay, 5*time.Second); got != "late:req" {
		t.Fatalf("got %q", got)
	}
}

// Bytes pass both ways unchanged, many round trips on one connection.
func TestBothDirectionsCarryTheBytes(t *testing.T) {
	in, out, toRelay, fromRelay := client(t)
	tg := target(t, func(c *net.UnixConn) {
		_, _ = io.Copy(c, c) // echo until the client half-closes
		c.Close()
	})
	go func() { _ = Relay(in, out, tg) }()
	r := bufio.NewReader(fromRelay)
	for i := range 100 {
		line := []byte("line\n")
		if _, err := toRelay.Write(line); err != nil {
			t.Fatal(err)
		}
		got, err := r.ReadString('\n')
		if err != nil || got != "line\n" {
			t.Fatalf("round %d: %q %v", i, got, err)
		}
	}
	_ = toRelay.Close()
	if rest := readAll(t, r, 3*time.Second); rest != "" {
		t.Fatalf("unexpected %q", rest)
	}
}

// The relay's stdout may share its socket with another descriptor; the end must still reach the client, so the
// write side is shut down, not only closed.
func TestEndReachesTheClientWhenOutIsDuplicated(t *testing.T) {
	in, out, toRelay, fromRelay := client(t)
	defer toRelay.Close()
	dup, err := dupFile(out)
	if err != nil {
		t.Fatal(err)
	}
	defer dup.Close()
	tg := target(t, func(c *net.UnixConn) {
		_, _ = c.Write([]byte("x"))
		c.Close()
	})
	go func() { _ = Relay(in, out, tg) }()
	if got := readAll(t, fromRelay, 3*time.Second); got != "x" {
		t.Fatalf("got %q", got)
	}
}

func TestDialRefusesOtherNetworks(t *testing.T) {
	if _, err := Dial("udp", "127.0.0.1:1"); err == nil {
		t.Fatal("udp was accepted")
	}
}

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
	go func() { _ = Relay(in, out, tg, HalfCloseLimit) }()
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
	go func() { _ = Relay(in, out, tg, HalfCloseLimit) }()
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
	go func() { _ = Relay(in, out, tg, HalfCloseLimit) }()
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
	go func() { _ = Relay(in, out, tg, HalfCloseLimit) }()
	if got := readAll(t, fromRelay, 3*time.Second); got != "x" {
		t.Fatalf("got %q", got)
	}
}

func TestDialRefusesOtherNetworks(t *testing.T) {
	if _, err := Dial("udp", "127.0.0.1:1"); err == nil {
		t.Fatal("udp was accepted")
	}
}

// A target that ignores the client's half-close is cut off after HalfCloseLimit, as socat's -t did.
func TestATargetThatNeverEndsIsCutOffAfterTheHalfClose(t *testing.T) {
	in, out, toRelay, fromRelay := client(t)
	hold := make(chan struct{})
	defer close(hold)
	tg := target(t, func(c *net.UnixConn) {
		<-hold // reads nothing, writes nothing, never closes while the test runs
		c.Close()
	})
	done := make(chan error, 1)
	go func() { done <- Relay(in, out, tg, 300*time.Millisecond) }()
	_ = toRelay.Close()
	select {
	case <-done:
	case <-time.After(3 * time.Second):
		t.Fatal("the relay did not end after HalfCloseLimit")
	}
	if got := readAll(t, fromRelay, time.Second); got != "" {
		t.Fatalf("got %q", got)
	}
}

// TD-103: the server reads part of a large request, answers and closes while the client still sends. The relay reads
// and drops the rest, so the client's send finishes (socat's write to a relay that stopped reading failed, and socat
// could end before it passed the answer on), and the client then reads the answer.
func TestAnEarlyAnswerReachesAClientThatStillSends(t *testing.T) {
	in, out, toRelay, fromRelay := client(t)
	const answer = "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n"
	tg := target(t, func(c *net.UnixConn) {
		head := make([]byte, 64*1024)
		_, _ = io.ReadFull(c, head)
		_, _ = c.Write([]byte(answer))
		c.Close()
	})
	done := make(chan error, 1)
	go func() { done <- Relay(in, out, tg, HalfCloseLimit) }()
	sent := make(chan error, 1)
	go func() {
		_, err := toRelay.Write(make([]byte, 4*1024*1024))
		sent <- err
	}()
	select {
	case err := <-sent:
		if err != nil {
			t.Fatalf("the client's send failed: %v", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("the client's send did not finish: the relay stopped reading it")
	}
	if got := readAll(t, fromRelay, 3*time.Second); got != answer {
		t.Fatalf("got %q", got)
	}
	_ = toRelay.Close()
	select {
	case <-done:
	case <-time.After(3 * time.Second):
		t.Fatal("the relay did not end after the client closed")
	}
}

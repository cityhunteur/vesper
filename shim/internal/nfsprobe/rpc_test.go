package nfsprobe

import (
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"net"
	"testing"
	"time"
)

// fakeServer answers one MOUNT3 MNT and one NFS3 GETATTR, checking the call it receives.
func fakeServer(t *testing.T, ln net.Listener, handle func(prog, proc uint32, cred Cred, args *dec) []byte) {
	t.Helper()
	go func() {
		c, err := ln.Accept()
		if err != nil {
			return
		}
		defer c.Close()
		for {
			var h [4]byte
			if _, err := io.ReadFull(c, h[:]); err != nil {
				return
			}
			n := binary.BigEndian.Uint32(h[:]) &^ (1 << 31)
			body := make([]byte, n)
			if _, err := io.ReadFull(c, body); err != nil {
				return
			}
			d := &dec{b: body}
			xid := d.u32()
			d.u32() // CALL
			d.u32() // rpc version
			prog, _, proc := d.u32(), d.u32(), d.u32()
			d.u32() // cred flavour
			cd := &dec{b: d.opaque(400)}
			cd.u32()
			cd.opaque(255)
			cred := Cred{UID: cd.u32(), GID: cd.u32()}
			d.u32()
			d.opaque(400) // verifier
			res := handle(prog, proc, cred, d)
			var e enc
			e.u32(0)
			e.u32(xid)
			e.u32(1) // REPLY
			e.u32(0) // accepted
			e.u32(0) // verifier
			e.u32(0)
			e.u32(0) // SUCCESS
			e.b = append(e.b, res...)
			binary.BigEndian.PutUint32(e.b, uint32(len(e.b)-4)|1<<31)
			if _, err := c.Write(e.b); err != nil {
				return
			}
		}
	}()
}

func TestMountAndGetattr(t *testing.T) {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer ln.Close()
	fh := []byte{1, 2, 3, 4, 5}
	fakeServer(t, ln, func(prog, proc uint32, cred Cred, args *dec) []byte {
		var e enc
		switch {
		case prog == ProgMount && proc == 1:
			if p := string(args.opaque(1024)); p != "/s/0" || cred.UID != 1000 || cred.GID != 1001 {
				t.Errorf("MNT got path %q cred %+v", p, cred)
			}
			e.u32(0)
			e.opaque(fh)
			e.u32(0) // no auth flavours
		case prog == ProgNFS && proc == procGetattr:
			if got := args.opaque(64); string(got) != string(fh) {
				t.Errorf("GETATTR got handle %x", got)
			}
			e.u32(0)
			for _, v := range []uint32{2, 0o755, 3, 1000, 1001} {
				e.u32(v)
			}
			e.u64(4096) // size
			e.u64(4096) // used
			e.u32(0)
			e.u32(0)    // rdev
			e.u64(77)   // fsid
			e.u64(1234) // fileid
			for i := 0; i < 6; i++ {
				e.u32(0)
			}
		default:
			t.Errorf("unexpected prog %d proc %d", prog, proc)
		}
		return e.b
	})
	port := ln.Addr().(*net.TCPAddr).Port
	c, err := Dial("", "127.0.0.1", port, ProgMount, 2*time.Second)
	if err != nil {
		t.Fatal(err)
	}
	defer c.Close()
	c.Cred = Cred{UID: 1000, GID: 1001}
	got, st, err := Mount(c, "/s/0")
	if err != nil || st != 0 || string(got) != string(fh) {
		t.Fatalf("Mount = %x, %v, %v", got, st, err)
	}
	c.prog = ProgNFS // the fake serves both programs on one connection
	a, st, err := NFS{c}.Getattr(fh)
	if err != nil || st != 0 || a.Mode != 0o755 || a.UID != 1000 || a.FSID != 77 || a.FileID != 1234 || a.Size != 4096 {
		t.Fatalf("Getattr = %+v, %v, %v", a, st, err)
	}
}

func TestStatusNames(t *testing.T) {
	if Status(30).String() != "ROFS" || Status(70).String() != "STALE" || Status(99).String() != "STATUS99" {
		t.Error("status names")
	}
}

func TestDecShort(t *testing.T) {
	d := &dec{b: []byte{0, 0, 0, 9, 1}}
	d.opaque(64)
	if d.err == nil {
		t.Error("a short opaque was accepted")
	}
	d = &dec{b: []byte{0, 0, 1, 0}}
	d.opaque(64)
	if d.err == nil {
		t.Error("an opaque over its limit was accepted")
	}
}

// A held call arrives in two parts: the server has everything but the last bytes of the arguments until the
// client's wait returns, and the call is whole afterwards (TD-079).
func TestRemoveHeld(t *testing.T) {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer ln.Close()
	fh := []byte{9, 8, 7, 6}
	held := make(chan struct{}) // closed when the server has seen that the end of the call is kept back
	fail := make(chan string, 1)
	go func() {
		c, err := ln.Accept()
		if err != nil {
			fail <- "accept: " + err.Error()
			close(held)
			return
		}
		defer c.Close()
		_ = c.SetReadDeadline(time.Now().Add(5 * time.Second)) // a client that keeps back too much fails this, not hangs
		var h [4]byte
		if _, err := io.ReadFull(c, h[:]); err != nil {
			fail <- "no record mark: " + err.Error()
			close(held)
			return
		}
		n := int(binary.BigEndian.Uint32(h[:]) &^ (1 << 31))
		body := make([]byte, n)
		if _, err := io.ReadFull(c, body[:n-4]); err != nil {
			fail <- "the first part: " + err.Error()
			close(held)
			return
		}
		_ = c.SetReadDeadline(time.Now().Add(200 * time.Millisecond))
		if _, err := c.Read(body[n-4:]); err == nil {
			fail <- "the end of the call arrived before the wait returned"
			close(held)
			return
		}
		_ = c.SetReadDeadline(time.Now().Add(5 * time.Second))
		close(held)
		if _, err := io.ReadFull(c, body[n-4:]); err != nil {
			fail <- "the rest: " + err.Error()
			return
		}
		d := &dec{b: body}
		xid := d.u32()
		d.u32()
		d.u32()
		_, _, proc := d.u32(), d.u32(), d.u32()
		d.u32()
		d.opaque(400)
		d.u32()
		d.opaque(400)
		dir, name := d.opaque(64), string(d.opaque(255))
		if proc != procRemove || string(dir) != string(fh) || name != "secret~" || d.err != nil {
			fail <- fmt.Sprintf("the call as it arrived: proc %d dir %x name %q err %v", proc, dir, name, d.err)
		}
		var e enc
		e.u32(0)
		e.u32(xid)
		e.u32(1)
		e.u32(0)
		e.u32(0)
		e.u32(0)
		e.u32(0)
		e.u32(30) // ROFS
		binary.BigEndian.PutUint32(e.b, uint32(len(e.b)-4)|1<<31)
		_, _ = c.Write(e.b)
	}()
	c, err := Dial("", "127.0.0.1", ln.Addr().(*net.TCPAddr).Port, ProgNFS, 2*time.Second)
	if err != nil {
		t.Fatal(err)
	}
	defer c.Close()
	waited := false
	st, err := NFS{c}.RemoveHeld(fh, "secret~", func() error {
		select {
		case <-held:
		case <-time.After(10 * time.Second):
			return errors.New("the server never saw the call held")
		}
		waited = true
		return nil
	})
	select {
	case msg := <-fail:
		t.Fatal(msg)
	default:
	}
	if err != nil || st != 30 || !waited {
		t.Fatalf("RemoveHeld = %v, %v (waited %v)", st, err, waited)
	}
	for _, bad := range []struct {
		hold int
		wait func() error
	}{{5, func() error { return nil }}, {0, func() error { return nil }}, {2, nil}} {
		if _, err := c.CallHeld(3, procRemove, []byte{1, 2, 3, 4}, bad.hold, bad.wait); err == nil {
			t.Errorf("a held call with hold %d and wait %v was sent", bad.hold, bad.wait != nil)
		}
	}
	// A wait that fails ends the call with its error, and the connection, which has part of a request on it.
	stop := errors.New("stop")
	if _, err := c.CallHeld(3, procRemove, []byte{1, 2, 3, 4, 5, 6, 7, 8}, 4, func() error { return stop }); !errors.Is(err, stop) {
		t.Errorf("a held call whose wait failed: %v", err)
	}
	if _, err := c.Call(3, procGetattr, nil); !errors.Is(err, net.ErrClosed) {
		t.Errorf("a call on the connection of a held call that failed: %v, want the connection closed", err)
	}
}

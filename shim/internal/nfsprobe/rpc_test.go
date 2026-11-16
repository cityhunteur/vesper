package nfsprobe

import (
	"encoding/binary"
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

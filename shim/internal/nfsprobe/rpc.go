// Package nfsprobe is a small NFSv3 client for testing a share server from outside (GH #160 M3): it speaks ONC RPC
// over TCP with AUTH_SYS credentials, MOUNT3 MNT, and the NFS3 calls the escape tests need. It sends whatever file
// handle it is given, so a test can send a forged one. It is a test tool, not a file system client.
package nfsprobe

import (
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"net"
	"syscall"
	"time"
)

const (
	// ProgNFS and ProgMount are the ONC RPC program numbers of NFS and MOUNT.
	ProgNFS   = 100003
	ProgMount = 100005
	authSys   = 1
	maxRecord = 1 << 20
)

// enc builds XDR.
type enc struct{ b []byte }

func (e *enc) u32(v uint32) { e.b = binary.BigEndian.AppendUint32(e.b, v) }
func (e *enc) u64(v uint64) { e.b = binary.BigEndian.AppendUint64(e.b, v) }
func (e *enc) opaque(p []byte) {
	e.u32(uint32(len(p)))
	e.b = append(e.b, p...)
	for len(e.b)%4 != 0 {
		e.b = append(e.b, 0)
	}
}
func (e *enc) str(s string) { e.opaque([]byte(s)) }
func (e *enc) fixed(p []byte) {
	e.b = append(e.b, p...)
	for len(e.b)%4 != 0 {
		e.b = append(e.b, 0)
	}
}

// dec reads XDR; the first error sticks.
type dec struct {
	b   []byte
	err error
}

var errShort = errors.New("xdr: short reply")

func (d *dec) u32() uint32 {
	if d.err != nil || len(d.b) < 4 {
		d.err = errShort
		return 0
	}
	v := binary.BigEndian.Uint32(d.b)
	d.b = d.b[4:]
	return v
}
func (d *dec) u64() uint64 { return uint64(d.u32())<<32 | uint64(d.u32()) }
func (d *dec) bytes(n int) []byte {
	pad := (4 - n%4) % 4
	if d.err != nil || n < 0 || len(d.b) < n+pad {
		d.err = errShort
		return nil
	}
	p := append([]byte(nil), d.b[:n]...)
	d.b = d.b[n+pad:]
	return p
}
func (d *dec) opaque(max int) []byte {
	n := int(d.u32())
	if d.err == nil && n > max {
		d.err = fmt.Errorf("xdr: opaque of %d bytes, limit %d", n, max)
		return nil
	}
	return d.bytes(n)
}

// Cred is the AUTH_SYS identity a call claims.
type Cred struct {
	UID, GID uint32
	GIDs     []uint32
}

// Conn is one RPC connection to one program.
type Conn struct {
	c       net.Conn
	prog    uint32
	xid     uint32
	Cred    Cred
	Timeout time.Duration
}

// Dial connects from local (an address and a port below 1024 when the server wants a reserved port; empty for
// any) to the server's port.
func Dial(local, server string, port int, prog uint32, timeout time.Duration) (*Conn, error) {
	var la *net.TCPAddr
	if local != "" {
		var err error
		if la, err = net.ResolveTCPAddr("tcp", local); err != nil {
			return nil, err
		}
	}
	var last error
	// A reserved source port: try from 1023 down, as a kernel client does.
	for p := 1023; p >= 600; p-- {
		var addr *net.TCPAddr
		if la != nil {
			addr = &net.TCPAddr{IP: la.IP, Port: p}
		}
		d := net.Dialer{LocalAddr: addr, Timeout: timeout}
		c, err := d.Dial("tcp", net.JoinHostPort(server, fmt.Sprint(port)))
		if err == nil {
			return &Conn{c: c, prog: prog, xid: uint32(time.Now().UnixNano()), Timeout: timeout}, nil
		}
		last = err
		// Only a source port in use is worth another try; a refusal or a timeout is the server's answer.
		if la == nil || !errors.Is(err, syscall.EADDRINUSE) {
			break
		}
	}
	return nil, last
}

// Close ends the connection.
func (c *Conn) Close() error { return c.c.Close() }

// AcceptError is an RPC call the server did not accept (bad program, procedure or arguments) or denied (auth).
type AcceptError struct{ Stat, Reject uint32 }

func (e *AcceptError) Error() string {
	return fmt.Sprintf("rpc: accept_stat %d reject %d", e.Stat, e.Reject)
}

// Call sends procedure proc with XDR args and returns the XDR result.
func (c *Conn) Call(vers, proc uint32, args []byte) (*dec, error) {
	c.xid++
	var e enc
	e.u32(0) // record mark, filled below
	e.u32(c.xid)
	e.u32(0) // CALL
	e.u32(2) // RPC version
	e.u32(c.prog)
	e.u32(vers)
	e.u32(proc)
	var cred enc
	cred.u32(0)  // stamp
	cred.str("") // machine name
	cred.u32(c.Cred.UID)
	cred.u32(c.Cred.GID)
	cred.u32(uint32(len(c.Cred.GIDs)))
	for _, g := range c.Cred.GIDs {
		cred.u32(g)
	}
	e.u32(authSys)
	e.opaque(cred.b)
	e.u32(0) // verifier AUTH_NONE
	e.u32(0)
	e.b = append(e.b, args...)
	binary.BigEndian.PutUint32(e.b, uint32(len(e.b)-4)|1<<31)
	if c.Timeout > 0 {
		_ = c.c.SetDeadline(time.Now().Add(c.Timeout))
	}
	if _, err := c.c.Write(e.b); err != nil {
		return nil, err
	}
	for {
		rec, err := c.readRecord()
		if err != nil {
			return nil, err
		}
		d := &dec{b: rec}
		if d.u32() != c.xid {
			continue // a late reply to an earlier call
		}
		if d.u32() != 1 {
			return nil, errors.New("rpc: not a reply")
		}
		if stat := d.u32(); stat != 0 { // MSG_DENIED
			return nil, &AcceptError{Stat: 0xffffffff, Reject: stat}
		}
		d.u32()       // verifier flavour
		d.opaque(400) // verifier body
		if as := d.u32(); as != 0 {
			return nil, &AcceptError{Stat: as}
		}
		return d, d.err
	}
}

func (c *Conn) readRecord() ([]byte, error) {
	var out []byte
	for {
		var h [4]byte
		if _, err := io.ReadFull(c.c, h[:]); err != nil {
			return nil, err
		}
		v := binary.BigEndian.Uint32(h[:])
		n := int(v &^ (1 << 31))
		if len(out)+n > maxRecord {
			return nil, errors.New("rpc: record too large")
		}
		frag := make([]byte, n)
		if _, err := io.ReadFull(c.c, frag); err != nil {
			return nil, err
		}
		out = append(out, frag...)
		if v&(1<<31) != 0 {
			return out, nil
		}
	}
}

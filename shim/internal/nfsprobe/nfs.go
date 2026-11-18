package nfsprobe

import (
	"fmt"
	"strings"
)

// NFS3 procedures and status names used by the probe.
const (
	procGetattr  = 1
	procSetattr  = 2
	procLookup   = 3
	procReadlink = 5
	procRead     = 6
	procWrite    = 7
	procCreate   = 8
	procMkdir    = 9
	procRemove   = 12
	procReaddir  = 16
	maxFH        = 64
)

var statusNames = map[uint32]string{
	0: "OK", 1: "PERM", 2: "NOENT", 5: "IO", 6: "NXIO", 13: "ACCES", 17: "EXIST", 18: "XDEV", 19: "NODEV",
	20: "NOTDIR", 21: "ISDIR", 22: "INVAL", 27: "FBIG", 28: "NOSPC", 30: "ROFS", 31: "MLINK", 63: "NAMETOOLONG",
	66: "NOTEMPTY", 69: "DQUOT", 70: "STALE", 71: "REMOTE", 10001: "BADHANDLE", 10002: "NOT_SYNC",
	10003: "BAD_COOKIE", 10004: "NOTSUPP", 10005: "TOOSMALL", 10006: "SERVERFAULT", 10007: "BADTYPE",
	10008: "JUKEBOX",
}

// Status is an NFS3 or MOUNT3 status.
type Status uint32

func (s Status) String() string {
	if n, ok := statusNames[uint32(s)]; ok {
		return n
	}
	return fmt.Sprintf("STATUS%d", uint32(s))
}

// Attr is the part of fattr3 the tests read.
type Attr struct {
	Type, Mode, Nlink, UID, GID uint32
	Size, FSID, FileID          uint64
}

func (a Attr) String() string {
	return fmt.Sprintf("type=%d mode=%o uid=%d gid=%d size=%d fsid=%d fileid=%d", a.Type, a.Mode, a.UID, a.GID, a.Size, a.FSID, a.FileID)
}

func (d *dec) fattr() Attr {
	var a Attr
	a.Type, a.Mode, a.Nlink, a.UID, a.GID = d.u32(), d.u32(), d.u32(), d.u32(), d.u32()
	a.Size = d.u64()
	d.u64() // used
	d.u32() // rdev
	d.u32()
	a.FSID = d.u64()
	a.FileID = d.u64()
	for i := 0; i < 6; i++ { // atime, mtime, ctime
		d.u32()
	}
	return a
}

func (d *dec) postOpAttr() (Attr, bool) {
	if d.u32() == 0 {
		return Attr{}, false
	}
	return d.fattr(), true
}

// Mount asks the MOUNT3 server for the root handle of an export.
func Mount(c *Conn, path string) ([]byte, Status, error) {
	var e enc
	e.str(path)
	d, err := c.Call(3, 1, e.b)
	if err != nil {
		return nil, 0, err
	}
	st := Status(d.u32())
	if st != 0 {
		return nil, st, d.err
	}
	fh := d.opaque(maxFH)
	return fh, st, d.err
}

// NFS is an NFS3 connection.
type NFS struct{ *Conn }

// Getattr returns a handle's attributes.
func (n NFS) Getattr(fh []byte) (Attr, Status, error) {
	var e enc
	e.opaque(fh)
	d, err := n.Call(3, procGetattr, e.b)
	if err != nil {
		return Attr{}, 0, err
	}
	st := Status(d.u32())
	if st != 0 {
		return Attr{}, st, d.err
	}
	return d.fattr(), st, d.err
}

// Lookup finds name in directory dir.
func (n NFS) Lookup(dir []byte, name string) ([]byte, Attr, Status, error) {
	var e enc
	e.opaque(dir)
	e.str(name)
	d, err := n.Call(3, procLookup, e.b)
	if err != nil {
		return nil, Attr{}, 0, err
	}
	st := Status(d.u32())
	if st != 0 {
		return nil, Attr{}, st, d.err
	}
	fh := d.opaque(maxFH)
	a, _ := d.postOpAttr()
	return fh, a, st, d.err
}

// Walk looks up each component of a slash-separated path from dir ("" is dir itself).
func (n NFS) Walk(dir []byte, path string) ([]byte, Attr, Status, error) {
	fh := dir
	var a Attr
	if path == "" {
		a, st, err := n.Getattr(dir)
		return dir, a, st, err
	}
	for _, c := range strings.Split(path, "/") {
		var st Status
		var err error
		fh, a, st, err = n.Lookup(fh, c)
		if err != nil || st != 0 {
			return nil, a, st, err
		}
	}
	return fh, a, 0, nil
}

// Read reads up to count bytes at offset.
func (n NFS) Read(fh []byte, off uint64, count uint32) ([]byte, Status, error) {
	var e enc
	e.opaque(fh)
	e.u64(off)
	e.u32(count)
	d, err := n.Call(3, procRead, e.b)
	if err != nil {
		return nil, 0, err
	}
	st := Status(d.u32())
	d.postOpAttr()
	if st != 0 {
		return nil, st, d.err
	}
	d.u32() // count
	d.u32() // eof
	data := d.opaque(maxRecord)
	return data, st, d.err
}

// Write writes data at offset with FILE_SYNC.
func (n NFS) Write(fh []byte, off uint64, data []byte) (Status, error) {
	var e enc
	e.opaque(fh)
	e.u64(off)
	e.u32(uint32(len(data)))
	e.u32(2) // FILE_SYNC
	e.opaque(data)
	d, err := n.Call(3, procWrite, e.b)
	if err != nil {
		return 0, err
	}
	return Status(d.u32()), d.err
}

func sattrMode(e *enc, mode *uint32) {
	if mode != nil {
		e.u32(1)
		e.u32(*mode)
	} else {
		e.u32(0)
	}
	e.u32(0) // uid
	e.u32(0) // gid
	e.u32(0) // size
	e.u32(0) // atime: don't change
	e.u32(0) // mtime
}

// Create makes a file (UNCHECKED) in dir.
func (n NFS) Create(dir []byte, name string, mode uint32) ([]byte, Status, error) {
	var e enc
	e.opaque(dir)
	e.str(name)
	e.u32(0) // UNCHECKED
	sattrMode(&e, &mode)
	d, err := n.Call(3, procCreate, e.b)
	if err != nil {
		return nil, 0, err
	}
	st := Status(d.u32())
	if st != 0 {
		return nil, st, d.err
	}
	var fh []byte
	if d.u32() != 0 {
		fh = d.opaque(maxFH)
	}
	return fh, st, d.err
}

// Mkdir makes a directory in dir.
func (n NFS) Mkdir(dir []byte, name string, mode uint32) (Status, error) {
	var e enc
	e.opaque(dir)
	e.str(name)
	sattrMode(&e, &mode)
	d, err := n.Call(3, procMkdir, e.b)
	if err != nil {
		return 0, err
	}
	return Status(d.u32()), d.err
}

// Remove removes a file from dir.
func (n NFS) Remove(dir []byte, name string) (Status, error) {
	var e enc
	e.opaque(dir)
	e.str(name)
	d, err := n.Call(3, procRemove, e.b)
	if err != nil {
		return 0, err
	}
	return Status(d.u32()), d.err
}

// RemoveHeld is Remove with the request held before the end of the name until wait returns (Conn.CallHeld).
func (n NFS) RemoveHeld(dir []byte, name string, wait func() error) (Status, error) {
	var e enc
	e.opaque(dir)
	e.str(name)
	d, err := n.CallHeld(3, procRemove, e.b, 4, wait) // the name's last bytes
	if err != nil {
		return 0, err
	}
	return Status(d.u32()), d.err
}

// CreateHeld is Create (UNCHECKED, which opens a file that is there) with the request held before the end of its
// attributes until wait returns. The server checks a CREATE in another way than a REMOVE.
func (n NFS) CreateHeld(dir []byte, name string, mode uint32, wait func() error) (Status, error) {
	var e enc
	e.opaque(dir)
	e.str(name)
	e.u32(0) // UNCHECKED
	sattrMode(&e, &mode)
	d, err := n.CallHeld(3, procCreate, e.b, 4, wait)
	if err != nil {
		return 0, err
	}
	return Status(d.u32()), d.err
}

// Chmod sets a mode.
func (n NFS) Chmod(fh []byte, mode uint32) (Status, error) {
	var e enc
	e.opaque(fh)
	sattrMode(&e, &mode)
	e.u32(0) // no guard
	d, err := n.Call(3, procSetattr, e.b)
	if err != nil {
		return 0, err
	}
	return Status(d.u32()), d.err
}

// Readlink returns a symbolic link's target.
func (n NFS) Readlink(fh []byte) (string, Status, error) {
	var e enc
	e.opaque(fh)
	d, err := n.Call(3, procReadlink, e.b)
	if err != nil {
		return "", 0, err
	}
	st := Status(d.u32())
	d.postOpAttr()
	if st != 0 {
		return "", st, d.err
	}
	return string(d.opaque(4096)), st, d.err
}

// Readdir lists the names in a directory (one call of up to 32 KiB).
func (n NFS) Readdir(fh []byte) ([]string, Status, error) {
	var e enc
	e.opaque(fh)
	e.u64(0)                 // cookie
	e.fixed(make([]byte, 8)) // cookie verifier
	e.u32(32768)
	d, err := n.Call(3, procReaddir, e.b)
	if err != nil {
		return nil, 0, err
	}
	st := Status(d.u32())
	d.postOpAttr()
	if st != 0 {
		return nil, st, d.err
	}
	d.bytes(8) // verifier
	var names []string
	for d.err == nil && d.u32() != 0 {
		d.u64() // fileid
		names = append(names, string(d.opaque(255)))
		d.u64() // cookie
	}
	return names, st, d.err
}

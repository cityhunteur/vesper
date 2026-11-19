package nfsprobe

import (
	"bytes"
	"encoding/hex"
	"fmt"
	"io"
	"os"
	"strings"
	"time"
)

// The M3 battery runs against a server started with these sources, made by the suite in a macOS share:
//
//	/s/0  m3/proj:rw        file.txt "proj", secret600 (0600), dir700/inner.txt (0700 directory),
//	                        link-out -> /etc, link-up -> ../outside, conf/ (source 1)
//	/s/1  m3/proj/conf:ro   app.conf "conf"
//	/s/2  m3/ro:ro          r.txt "ro"
//
// and m3/outside/secret.txt, which no source holds. Every line is "PROBE <name> ok|FAIL <detail>" or
// "INFO <name> <detail>", then "PROBE-SUMMARY passed=N failed=M".
type Battery struct {
	Server   string // the server address
	Local    string // the local address to call from (the guest's)
	Cred     Cred
	Timeout  time.Duration
	Out      io.Writer
	pass     int
	fail     int
	shareFS  uint64
	exported map[[2]uint64]bool // (fsid, fileid) of every object reachable inside the exports
}

func (b *Battery) check(name string, ok bool, format string, a ...any) {
	res := "ok"
	if ok {
		b.pass++
	} else {
		res = "FAIL"
		b.fail++
	}
	fmt.Fprintf(b.Out, "PROBE %s %s %s\n", name, res, fmt.Sprintf(format, a...))
}

func (b *Battery) info(name, format string, a ...any) {
	fmt.Fprintf(b.Out, "INFO %s %s\n", name, fmt.Sprintf(format, a...))
}

func (b *Battery) dial(port int, prog uint32) (*Conn, error) {
	c, err := Dial(b.Local, b.Server, port, prog, b.Timeout)
	if err != nil {
		return nil, err
	}
	c.Cred = b.Cred
	return c, nil
}

// Held sends one request that would change the file at path (below /s/0), a REMOVE or an UNCHECKED CREATE by kind,
// and holds it, unfinished, until the file until exists; then it sends the rest and reports whether the server
// refused the request as read-only. While the request is held the server looks at nothing. A file that came to be
// what path names in that time, and that was the read-only file for a while, must still be refused: the server
// reads its list inside the request's own check (TD-078, TD-079).
//
// It prints "HELD sent KIND PATH (present|absent)" when the request is held, with what the name was just before,
// and makes the file marker then, so that the caller knows the hold began before what it does next. At the end it
// prints "HELD KIND PATH status=…" and "HELD control status=…": a file made and removed in the export's root, to
// show that the server refuses that one request and not every change.
func (b *Battery) Held(kind, path, until, marker string, limit time.Duration) bool {
	fail := func(format string, a ...any) bool {
		fmt.Fprintf(b.Out, "HELD failed: "+format+"\n", a...)
		return false
	}
	if kind != "remove" && kind != "create" {
		return fail("kind %q", kind)
	}
	m, err := b.dial(2050, ProgMount)
	if err != nil {
		return fail("mount port: %v", err)
	}
	root, st, err := Mount(m, "/s/0")
	m.Close()
	if err != nil || st != 0 {
		return fail("mount /s/0: status=%v err=%v", st, err)
	}
	c, err := b.dial(2049, ProgNFS)
	if err != nil {
		return fail("nfs port: %v", err)
	}
	defer c.Close()
	n := NFS{c}
	dir, name := root, path
	if i := strings.LastIndex(path, "/"); i >= 0 {
		if dir, _, st, err = n.Walk(root, path[:i]); err != nil || st != 0 {
			return fail("walk %s: status=%v err=%v", path[:i], st, err)
		}
		name = path[i+1:]
	}
	was := "absent"
	if _, _, lst, lerr := n.Lookup(dir, name); lerr != nil {
		return fail("lookup %s: %v", path, lerr)
	} else if lst == 0 {
		was = "present"
	}
	wait := func() error {
		fmt.Fprintf(b.Out, "HELD sent %s %s (%s)\n", kind, path, was)
		if marker != "" {
			if err := os.WriteFile(marker, nil, 0o644); err != nil {
				return fmt.Errorf("marker %s: %w", marker, err)
			}
		}
		var last error
		for end := time.Now().Add(limit); time.Now().Before(end); time.Sleep(200 * time.Millisecond) {
			if _, last = os.Stat(until); last == nil {
				return nil
			}
		}
		return fmt.Errorf("%s did not appear in %s (last: %v)", until, limit, last)
	}
	if kind == "remove" {
		st, err = n.RemoveHeld(dir, name, wait)
	} else {
		st, err = n.CreateHeld(dir, name, 0o644, wait)
	}
	if err != nil {
		return fail("%s %s: %v", kind, path, err)
	}
	fmt.Fprintf(b.Out, "HELD %s %s status=%v\n", kind, path, st)
	// The control: the same connection makes and removes a file of its own in the export's root.
	_, cst, cerr := n.Create(root, ".vesper-held-control", 0o644)
	if cerr == nil && cst == 0 {
		cst, cerr = n.Remove(root, ".vesper-held-control")
	}
	fmt.Fprintf(b.Out, "HELD control status=%v err=%v\n", cst, cerr)
	return st == 30 && cerr == nil && cst == 0 // ROFS for the held request, and the control went through
}

// RawRemove sends one REMOVE of the object at path (below /s/0) and prints "REMOVE PATH status=…". A REMOVE of a
// directory is something a kernel client never sends (its unlink of a directory fails before it asks the server),
// and unfs3 answers it with a removal of the directory when it is empty. It reports whether the call was answered.
func (b *Battery) RawRemove(path string) bool {
	fail := func(format string, a ...any) bool {
		fmt.Fprintf(b.Out, "REMOVE failed: "+format+"\n", a...)
		return false
	}
	m, err := b.dial(2050, ProgMount)
	if err != nil {
		return fail("mount port: %v", err)
	}
	root, st, err := Mount(m, "/s/0")
	m.Close()
	if err != nil || st != 0 {
		return fail("mount /s/0: status=%v err=%v", st, err)
	}
	c, err := b.dial(2049, ProgNFS)
	if err != nil {
		return fail("nfs port: %v", err)
	}
	defer c.Close()
	n := NFS{c}
	dir, name := root, path
	if i := strings.LastIndex(path, "/"); i >= 0 {
		if dir, _, st, err = n.Walk(root, path[:i]); err != nil || st != 0 {
			return fail("walk %s: status=%v err=%v", path[:i], st, err)
		}
		name = path[i+1:]
	}
	if st, err = n.Remove(dir, name); err != nil {
		return fail("remove %s: %v", path, err)
	}
	fmt.Fprintf(b.Out, "REMOVE %s status=%v\n", path, st)
	return true
}

// Run runs the battery and reports whether every check passed.
func (b *Battery) Run() bool {
	m, err := b.dial(2050, ProgMount)
	if err != nil {
		b.check("mount_reachable", false, "%v", err)
		return b.summary()
	}
	defer m.Close()
	roots := map[string][]byte{}
	for _, p := range []string{"/s/0", "/s/1", "/s/2"} {
		fh, st, err := Mount(m, p)
		b.check("mnt "+p, err == nil && st == 0 && len(fh) > 0, "status=%v err=%v", st, err)
		roots[p] = fh
	}
	for _, p := range []string{"/", "/s", "/s/3", "/usr", "/etc", "/s/0/.."} {
		_, st, err := Mount(m, p)
		b.check("mnt-refused "+p, err != nil || st != 0, "status=%v err=%v", st, err)
	}
	// A path that resolves to another export is that export, nothing more.
	if fh, st, err := Mount(m, "/s/0/../1"); err == nil && st == 0 {
		b.check("mnt /s/0/../1 is /s/1", bytes.Equal(fh, roots["/s/1"]), "handle %x, /s/1 %x", fh, roots["/s/1"])
	} else {
		b.check("mnt /s/0/../1 is /s/1", true, "refused: status=%v err=%v", st, err)
	}
	c, err := b.dial(2049, ProgNFS)
	if err != nil {
		b.check("nfs_reachable", false, "%v", err)
		return b.summary()
	}
	defer c.Close()
	n := NFS{c}
	r0, r1, r2 := roots["/s/0"], roots["/s/1"], roots["/s/2"]
	if r0 == nil || r1 == nil || r2 == nil {
		return b.summary()
	}
	a0, st, err := n.Getattr(r0)
	b.check("getattr /s/0", err == nil && st == 0, "%v status=%v err=%v", a0, st, err)
	b.shareFS = a0.FSID
	b.exported = map[[2]uint64]bool{}

	read := func(name string, dir []byte, path, want string) {
		fh, _, st, err := n.Walk(dir, path)
		if err != nil || st != 0 {
			b.check(name, false, "lookup %s: status=%v err=%v", path, st, err)
			return
		}
		data, st, err := n.Read(fh, 0, 4096)
		b.check(name, err == nil && st == 0 && string(bytes.TrimSpace(data)) == want, "read %s: %q status=%v err=%v", path, data, st, err)
	}
	read("read file", r0, "file.txt", "proj")
	read("read nested 0700 dir", r0, "dir700/inner.txt", "inner")
	read("read ro export", r2, "r.txt", "ro")
	read("read nested ro through outer", r0, "conf/app.conf", "conf")
	// A 0600 file: the question that decided the server's user (ADR 0009, Privilege model; D-037). The owner it
	// reports is printed for the suite, which checks that a caller sees its own uid.
	if fh, a, st, err := n.Walk(r0, "secret600"); err == nil && st == 0 {
		data, st, err := n.Read(fh, 0, 64)
		b.check("read 0600 file", err == nil && st == 0 && string(bytes.TrimSpace(data)) == "s", "read status=%v err=%v data=%q", st, err, bytes.TrimSpace(data))
		b.info("read-0600", "%v", a)
	} else {
		b.check("read 0600 file", false, "lookup status=%v err=%v", st, err)
	}

	// Writes: allowed in the rw export, refused in a ro export and in a ro source nested in the rw one.
	fh, st, err := n.Create(r0, "probe-new.txt", 0o644)
	b.check("create in rw", err == nil && st == 0, "status=%v err=%v", st, err)
	if fh != nil {
		st, err = n.Write(fh, 0, []byte("w\n"))
		b.check("write in rw", err == nil && st == 0, "status=%v err=%v", st, err)
		st, err = n.Remove(r0, "probe-new.txt")
		b.check("remove in rw", err == nil && st == 0, "status=%v err=%v", st, err)
	}
	// A refusal is an NFS status from the server; a dropped connection or an RPC error proves nothing.
	refused := func(name string, st Status, err error) {
		b.check(name, err == nil && st != 0, "status=%v err=%v", st, err)
	}
	_, st, err = n.Create(r2, "x", 0o644)
	refused("create in ro export", st, err)
	if fh, _, st0, _ := n.Walk(r2, "r.txt"); st0 == 0 {
		st, err = n.Write(fh, 0, []byte("x"))
		refused("write in ro export", st, err)
		st, err = n.Chmod(fh, 0o777)
		refused("chmod in ro export", st, err)
	} else {
		b.check("lookup r.txt in ro export", false, "status=%v", st0)
	}
	_, st, err = n.Create(r1, "x", 0o644)
	refused("create in ro conf export", st, err)
	conf, _, st0, _ := n.Walk(r0, "conf")
	if st0 == 0 {
		_, st, err = n.Create(conf, "x", 0o644)
		refused("create in nested ro through outer", st, err)
		st, err = n.Mkdir(conf, "d", 0o755)
		refused("mkdir in nested ro through outer", st, err)
		if fh, _, st1, _ := n.Lookup(conf, "app.conf"); st1 == 0 {
			st, err = n.Write(fh, 0, []byte("x"))
			refused("write nested ro through outer", st, err)
			st, err = n.Remove(conf, "app.conf")
			refused("remove nested ro through outer", st, err)
		} else {
			b.check("lookup app.conf through outer", false, "status=%v", st1)
		}
	} else {
		b.check("lookup conf through outer", false, "status=%v", st0)
	}

	// Nothing outside the exports: "..", symbolic links, and forged handles. First, every object the exports hold,
	// which is the whole of what a handle may name.
	for _, r := range [][]byte{r0, r1, r2} {
		b.collect(n, r, 0)
	}
	b.info("exported-objects", "%d", len(b.exported))
	for i, r := range [][]byte{r0, r1, r2} {
		up := r
		for k := 1; k <= 3; k++ {
			h, a, st, err := n.Lookup(up, "..")
			if err != nil || st != 0 {
				b.check(fmt.Sprintf("dotdot /s/%d x%d", i, k), err == nil, "status=%v err=%v", st, err)
				break
			}
			name := fmt.Sprintf("dotdot /s/%d x%d", i, k)
			if a.FSID != b.shareFS {
				// unfs3 answers ".." at an export's root with the directory above it, outside every export. Its
				// attributes come back with the lookup; the handle itself must open nothing.
				b.info(name, "outside the share: %v", a)
				b.refusedHandle(name, n, h)
				break
			}
			b.check(name, true, "%v", a)
			if names, st, _ := n.Readdir(h); st == 0 {
				b.info(fmt.Sprintf("dotdot-list /s/%d x%d", i, k), "%v", names)
				for _, nm := range names {
					if nm == "usr" || nm == "etc" || nm == "lib" || nm == "s" || nm == "outside" {
						b.check(fmt.Sprintf("dotdot-names /s/%d x%d", i, k), false, "sees %q", nm)
					}
				}
			}
			up = h
		}
	}
	for _, p := range []string{"link-out/passwd", "link-up/secret.txt"} {
		_, a, st, err := n.Walk(r0, p)
		b.check("through symlink "+p, err != nil || st != 0, "status=%v err=%v %v", st, err, a)
	}
	// unfs3 follows a symbolic link in the middle of a path, so link-dev/null reaches the sandbox's /dev/null, which
	// is the System VM's own inode. Its mode must not change (council SEC-1): the attempt sets the mode it already
	// has, so a failure here changes nothing. What else such a link can read is a recorded residual (SEC-2).
	if fh, a, st, err := n.Walk(r0, "link-dev/null"); err == nil && st == 0 {
		b.info("through symlink link-dev/null", "%v", a)
		cs, cerr := n.Chmod(fh, a.Mode&0o7777)
		b.check("setattr /dev/null through a symlink", cerr == nil && cs != 0, "status=%v err=%v", cs, cerr)
	} else {
		b.check("setattr /dev/null through a symlink", true, "not reachable: status=%v err=%v", st, err)
	}
	// TD-073: the server follows a symbolic link whose handle is used as a directory. The fixture's link-CONF points
	// at the read-only nested directory under another letter case, a name no read-only mount covers on a volume that
	// ignores case; link-FILE points at a file in it. The server must refuse by the object's number, with the link
	// resolved (council SEC-1). On a volume that keeps case the links dangle, and what cannot be resolved is refused.
	if fhL, _, st, _ := n.Lookup(r0, "link-CONF"); st == 0 {
		_, cs, cerr := n.Create(fhL, "probe-through-link", 0o644)
		b.check("create in a read-only directory through a link handle", cerr == nil && cs != 0, "status=%v err=%v", cs, cerr)
		ms, merr := n.Mkdir(fhL, "probe-dir-through-link", 0o755)
		b.check("mkdir in a read-only directory through a link handle", merr == nil && ms != 0, "status=%v err=%v", ms, merr)
		rs, rerr := n.Remove(fhL, "app.conf")
		b.check("remove in a read-only directory through a link handle", rerr == nil && rs != 0, "status=%v err=%v", rs, rerr)
		if fh, _, st2, _ := n.Lookup(fhL, "app.conf"); st2 == 0 {
			ws, werr := n.Write(fh, 0, []byte("x"))
			b.check("write in a read-only directory through a link handle", werr == nil && ws != 0, "status=%v err=%v", ws, werr)
		}
	} else {
		b.check("the fixture has link-CONF", false, "status=%v", st)
	}
	if fhF, _, st, _ := n.Lookup(r0, "link-FILE"); st == 0 {
		cs, cerr := n.Chmod(fhF, 0o600)
		b.check("setattr through a link to a file in a read-only directory", cerr == nil && cs != 0, "status=%v err=%v", cs, cerr)
	} else {
		b.check("the fixture has link-FILE", false, "status=%v", st)
	}
	// Links that reach nothing protected stay usable while a read-only source exists (council C-1): a change of
	// attributes through a link to nowhere fails as it did (no such file), not as read-only, and a create through a
	// link whose target is not there yet makes the target.
	if fhN, _, st, _ := n.Lookup(r0, "link-nowhere"); st == 0 {
		cs, cerr := n.Chmod(fhN, 0o600)
		b.check("setattr through a link to nowhere is not refused as read-only", cerr == nil && cs != 30, "status=%v err=%v", cs, cerr)
	} else {
		b.check("the fixture has link-nowhere", false, "status=%v", st)
	}
	if _, _, st, _ := n.Lookup(r0, "link-new"); st == 0 {
		_, cs, cerr := n.Create(r0, "link-new", 0o644)
		_, _, ts, _ := n.Lookup(r0, "made-through-link")
		b.check("create through a link to a new name makes the target", cerr == nil && cs == 0 && ts == 0, "status=%v err=%v target=%v", cs, cerr, ts)
		_, _ = n.Remove(r0, "made-through-link")
	} else {
		b.check("the fixture has link-new", false, "status=%v", st)
	}
	if _, _, st, _ := n.Lookup(r0, "link-FILE"); st == 0 { // an UNCHECKED create opens what the name gives
		_, cs, cerr := n.Create(r0, "link-FILE", 0o644)
		b.check("create over a link to a file in a read-only directory", cerr == nil && cs != 0, "status=%v err=%v", cs, cerr)
	}
	if _, a, st, err := n.Walk(r0, "link-out/exports"); err == nil && st == 0 {
		b.info("through symlink link-out/exports", "readable residual (SEC-2): %v", a)
	}
	if fh, _, st, _ := n.Lookup(r0, "link-out"); st == 0 {
		t, st, err := n.Readlink(fh)
		b.check("readlink link-out", err == nil && st == 0 && t == "/etc", "%q status=%v err=%v", t, st, err)
	}
	b.forged("/s/0", &n, r0)
	fh, _, st, err = n.Walk(r0, "file.txt")
	if err != nil { // the forged handles above may have ended this connection as the server restarted (TD-065)
		if nn, rerr := b.reconnect(); rerr == nil {
			n.Close()
			n = nn
			fh, _, st, err = n.Walk(r0, "file.txt")
		}
	}
	if err == nil && st == 0 {
		b.forged("/s/0/file.txt", &n, fh)
	} else {
		b.check("forged /s/0/file.txt", false, "lookup file.txt: status=%v err=%v", st, err)
	}
	return b.summary()
}

// collect records every object below a directory handle (a few levels; the fixture is small).
func (b *Battery) collect(n NFS, dir []byte, depth int) {
	if a, st, err := n.Getattr(dir); err == nil && st == 0 {
		b.exported[[2]uint64{a.FSID, a.FileID}] = true
	}
	if depth > 4 {
		return
	}
	names, st, err := n.Readdir(dir)
	if err != nil || st != 0 {
		return
	}
	for _, nm := range names {
		if nm == "." || nm == ".." {
			continue
		}
		fh, a, st, err := n.Lookup(dir, nm)
		if err != nil || st != 0 {
			continue
		}
		b.exported[[2]uint64{a.FSID, a.FileID}] = true
		if a.Type == 2 {
			b.collect(n, fh, depth+1)
		}
	}
}

// reconnect waits for the server to listen again (supervise-daemon restarts it within about a second).
func (b *Battery) reconnect() (NFS, error) {
	var last error
	for range 30 {
		c, err := b.dial(2049, ProgNFS)
		if err == nil {
			return NFS{c}, nil
		}
		last = err
		time.Sleep(500 * time.Millisecond)
	}
	return NFS{}, last
}

// refusedHandle checks that a handle of something outside the exports reads, lists and finds nothing: each call
// must come back with an NFS error status.
func (b *Battery) refusedHandle(name string, n NFS, h []byte) {
	_, st, err := n.Getattr(h)
	b.check(name+" getattr", err == nil && st != 0, "status=%v err=%v", st, err)
	names, st, err := n.Readdir(h)
	b.check(name+" readdir", err == nil && st != 0, "status=%v err=%v names=%v", st, err, names)
	for _, c := range []string{"0", "1", "2", "usr", "etc", "lib", "dev", ".."} {
		_, a, st, err := n.Lookup(h, c)
		b.check(name+" lookup "+c, err == nil && st != 0, "status=%v err=%v %v", st, err, a)
	}
	_, st, err = n.Create(h, "x", 0o644)
	b.check(name+" create", err == nil && st != 0, "status=%v err=%v", st, err)
}

// forged sends variants of a real handle: each byte flipped, eight bytes longer, all zeros (every one must be sent
// and answered), then each shorter length. A variant the server accepts must name an object seen inside the exports.
// unfs3 0.10.0 aborts on some shortened handles (TD-065): a dropped connection among the full-length variants is
// reported, the probe waits for the restart and goes on; among the shortened ones it reports and stops there.
func (b *Battery) forged(name string, n *NFS, fh []byte) {
	var full, short [][]byte
	for i := range fh {
		v := append([]byte(nil), fh...)
		v[i] ^= 0xff
		full = append(full, v)
	}
	full = append(full, append(append([]byte(nil), fh...), make([]byte, 8)...), make([]byte, len(fh)))
	for l := len(fh) - 1; l >= 0; l-- {
		short = append(short, append([]byte(nil), fh[:l]...))
	}
	accepted, outside, answered, drops, shortSent, fullSent := 0, 0, 0, 0, 0, 0
	lost := false
	try := func(v []byte) bool {
		a, st, err := n.Getattr(v)
		if err != nil {
			drops++
			b.info("forged-dropped "+name, "the server dropped the connection at handle %x: %v", v, err)
			n.Close()
			nn, rerr := b.reconnect()
			if rerr != nil {
				lost = true
				b.check("forged "+name+" server back", false, "%v", rerr)
				return false
			}
			*n = nn
			return false
		}
		if st == 0 {
			accepted++
			if a.FSID != b.shareFS || !b.exported[[2]uint64{a.FSID, a.FileID}] {
				outside++
				b.info("forged-outside "+name, "handle %x -> %v", v, a)
			}
		}
		return true
	}
	for _, v := range full {
		if lost {
			break
		}
		fullSent++
		if try(v) {
			answered++
		}
	}
	for _, v := range short {
		if lost {
			break
		}
		shortSent++
		if !try(v) {
			break
		}
	}
	b.check("forged "+name, !lost && fullSent == len(full) && outside == 0,
		"%d of %d full-length variants sent (%d answered), %d of %d shortened sent, %d accepted, %d outside the exports, %d dropped connections",
		fullSent, len(full), answered, shortSent, len(short), accepted, outside, drops)
}

func (b *Battery) summary() bool {
	fmt.Fprintf(b.Out, "PROBE-SUMMARY passed=%d failed=%d\n", b.pass, b.fail)
	return b.fail == 0
}

// SaveHandles mounts /s/0 and writes the handles of its root and of file.txt to path, one "name hex" per line.
func (b *Battery) SaveHandles(path string) error {
	m, err := b.dial(2050, ProgMount)
	if err != nil {
		return err
	}
	defer m.Close()
	r0, st, err := Mount(m, "/s/0")
	if err != nil || st != 0 {
		return fmt.Errorf("mnt /s/0: status=%v err=%v", st, err)
	}
	c, err := b.dial(2049, ProgNFS)
	if err != nil {
		return err
	}
	defer c.Close()
	f, _, st, err := NFS{c}.Lookup(r0, "file.txt")
	if err != nil || st != 0 {
		return fmt.Errorf("lookup file.txt: status=%v err=%v", st, err)
	}
	return os.WriteFile(path, fmt.Appendf(nil, "root %x\nfile %x\n", r0, f), 0o600)
}

// CheckHandles uses handles saved before a restart, without mounting again: "HANDLE <name> <status>" per handle.
func (b *Battery) CheckHandles(path string) error {
	raw, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	c, err := b.dial(2049, ProgNFS)
	if err != nil {
		return err
	}
	defer c.Close()
	n := NFS{c}
	for _, line := range strings.Split(strings.TrimSpace(string(raw)), "\n") {
		var name, hx string
		if _, err := fmt.Sscan(line, &name, &hx); err != nil {
			return err
		}
		fh, err := hex.DecodeString(hx)
		if err != nil {
			return err
		}
		_, st, err := n.Getattr(fh)
		detail := fmt.Sprintf("getattr=%v", st)
		if name == "file" && err == nil && st == 0 {
			data, rst, _ := n.Read(fh, 0, 64)
			detail += fmt.Sprintf(" read=%v %q", rst, bytes.TrimSpace(data))
		}
		if err != nil {
			detail += " err=" + err.Error()
		}
		fmt.Fprintf(b.Out, "HANDLE %s %s\n", name, detail)
	}
	return nil
}

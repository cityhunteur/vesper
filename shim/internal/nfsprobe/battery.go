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
	exported map[uint64]bool
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
	b.exported = map[uint64]bool{}

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
	// A 0600 file: the question that decides the server's user (ADR 0009, Privilege model). Reported, not judged.
	if fh, a, st, err := n.Walk(r0, "secret600"); err == nil && st == 0 {
		data, st, err := n.Read(fh, 0, 64)
		b.info("read-0600", "%v; read status=%v err=%v data=%q", a, st, err, bytes.TrimSpace(data))
	} else {
		b.info("read-0600", "lookup status=%v err=%v", st, err)
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
	refused := func(name string, st Status, err error) {
		b.check(name, err != nil || st != 0, "status=%v err=%v", st, err)
	}
	_, st, err = n.Create(r2, "x", 0o644)
	refused("create in ro export", st, err)
	if fh, _, st0, _ := n.Walk(r2, "r.txt"); st0 == 0 {
		st, err = n.Write(fh, 0, []byte("x"))
		refused("write in ro export", st, err)
		st, err = n.Chmod(fh, 0o777)
		refused("chmod in ro export", st, err)
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
		}
	} else {
		b.check("lookup conf through outer", false, "status=%v", st0)
	}

	// Nothing outside the exports: "..", symbolic links, and forged handles.
	b.exported[a0.FileID] = true
	for i, r := range [][]byte{r0, r1, r2} {
		a, _, _ := n.Getattr(r)
		b.exported[a.FileID] = true
		up := r
		for k := 1; k <= 3; k++ {
			h, a, st, err := n.Lookup(up, "..")
			if err != nil || st != 0 {
				b.check(fmt.Sprintf("dotdot /s/%d x%d", i, k), true, "status=%v err=%v", st, err)
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
	if fh, _, st, _ := n.Lookup(r0, "link-out"); st == 0 {
		t, st, err := n.Readlink(fh)
		b.check("readlink link-out", err == nil && st == 0 && t == "/etc", "%q status=%v err=%v", t, st, err)
	}
	b.forged("/s/0", n, r0)
	if fh, _, st, _ := n.Walk(r0, "file.txt"); st == 0 {
		b.forged("/s/0/file.txt", n, fh)
	}
	return b.summary()
}

// refusedHandle checks that a handle of something outside the exports reads, lists and finds nothing.
func (b *Battery) refusedHandle(name string, n NFS, h []byte) {
	_, st, err := n.Getattr(h)
	b.check(name+" getattr", err != nil || st != 0, "status=%v err=%v", st, err)
	names, st, err := n.Readdir(h)
	b.check(name+" readdir", err != nil || st != 0, "status=%v err=%v names=%v", st, err, names)
	for _, c := range []string{"0", "1", "2", "usr", "etc", "lib", "dev", ".."} {
		_, a, st, err := n.Lookup(h, c)
		b.check(name+" lookup "+c, err != nil || st != 0, "status=%v err=%v %v", st, err, a)
	}
	_, st, err = n.Create(h, "x", 0o644)
	b.check(name+" create", err != nil || st != 0, "status=%v err=%v", st, err)
}

// forged sends variants of a real handle: each byte flipped, each shorter length, longer, all zeros. A variant the
// server accepts must name something in the share (same fsid); the sandbox holds nothing else of the share than the
// exports, so that is the whole check.
func (b *Battery) forged(name string, n NFS, fh []byte) {
	var variants [][]byte
	for i := range fh {
		v := append([]byte(nil), fh...)
		v[i] ^= 0xff
		variants = append(variants, v)
	}
	variants = append(variants, append(append([]byte(nil), fh...), make([]byte, 8)...), make([]byte, len(fh)))
	// Shortened handles last: unfs3 asserts on some of them and dies (TD-065), which ends the run.
	for l := len(fh) - 1; l >= 0; l-- {
		variants = append(variants, append([]byte(nil), fh[:l]...))
	}
	accepted, outside, sent := 0, 0, 0
	for _, v := range variants {
		a, st, err := n.Getattr(v)
		sent++
		if err != nil {
			// unfs3 0.10.0 asserts on some malformed handles and the server dies (TD-065); supervise-daemon
			// starts it again. That ends this connection, so the rest of the variants are not sent.
			b.info("forged-dropped "+name, "the server dropped the connection at variant %d of %d (%x): %v", sent, len(variants), v, err)
			break
		}
		if st == 0 {
			accepted++
			if a.FSID != b.shareFS {
				outside++
				b.info("forged-outside "+name, "handle %x -> %v", v, a)
			}
		}
	}
	b.check("forged "+name, outside == 0, "%d of %d variants sent, %d accepted, %d outside the share", sent, len(variants), accepted, outside)
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

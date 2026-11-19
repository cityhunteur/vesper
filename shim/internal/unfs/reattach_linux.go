//go:build linux

package unfs

import (
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"time"

	"golang.org/x/sys/unix"
)

// A file source that macOS replaces (D-052).
//
// An editor's save, `git checkout` and `sed -i` write a new file and rename it over the old one. The export still
// holds the old file, which is gone: every read through it fails, and the guest sees a stale handle. A directory
// source does not have the fault, because a name in it is looked up again.
//
// So the supervisor looks at each file source's path every FileCheck. When the file there is another one than the
// file attached, it attaches the new one in the sandbox in place of the old one, places the read-only nests of that
// source again, and tells the guest, which binds the file again at its destinations.
//
// This opens a source path again after the server has started, which Source binding (ADR 0009) otherwise forbids,
// and a task can change what a path names: with a read-write directory source it can rename directories so that
// the path of one of its file sources names another file, one that another bind made read-only. So the new file is
// not taken from the share mount when a read-write directory source holds its path. It is taken through that
// source's export inside the sandbox, by the same walk an NFS request makes. A file the walk reaches under a
// read-only nest is cloned from that nest and stays read-only, whatever the file source's own mode. When no
// read-write directory source holds the path, the task cannot change what it names, and the file is taken from the
// share mount by the rules of the first open.

// FileCheck is how often the supervisor looks for a replaced file source.
var FileCheck = time.Second

// The guest is told after a file was attached again, and after the server started, until it has answered twice in a
// row that its binds are the export's file, or the notices run out. One notice is not enough: for a second or two
// after the new file is attached the server can still answer for the old one (unfsd holds the old file open), and a
// guest that looks then finds nothing changed, or binds the old file once more.
const (
	noticesAfterSwap  = 30
	noticesAfterStart = 5
	settledAnswers    = 2
	// swapBudget is how many times one source can be attached or nested again at a stretch; one more is allowed
	// every swapRefill. A task that renames directories in a loop gets no more mounts than this out of the server.
	swapBudget = 20
	swapRefill = 15 * time.Second
	// verifyChecks is for how many checks after an attach the attached file is read again, to see it is not a dead
	// one.
	verifyChecks = 3
	// coveredChecks is for how many checks a nest found under another read-only mount is left alone.
	coveredChecks = 30
	// maxPins is how many replaced read-only files stay held (fileWatch.pins).
	maxPins = 256
	// maxListGrowth is how many bytes the watcher adds to the server's list before it has the list written anew
	// by a start of the server: about 50,000 lines, more than a week of attaches at the rate one source allows.
	maxListGrowth = 1 << 20
)

var (
	// errNotTheFileOpened: the file reached through the export is another one than the file opened at the path.
	errNotTheFileOpened = errors.New("the file reached through the export is not the one that was opened at the path")
	// errNotTold: the server's list could not be told of a file.
	errNotTold = errors.New("the server's list was not told")
)

// moveMountBeneath is MOVE_MOUNT_BENEATH (Linux 6.5): attach under the mount that is there, so that removing that
// one shows the new mount with no moment in between.
const moveMountBeneath = 0x200

// fileID tells one file from the one that replaced it under the same name.
type fileID struct{ dev, ino uint64 }

func idOf(fd int) (fileID, error) {
	var st unix.Stat_t
	if err := unix.Fstat(fd, &st); err != nil {
		return fileID{}, err
	}
	return fileID{uint64(st.Dev), st.Ino}, nil
}

// notice is what is still to tell the guest about one source.
type notice struct {
	left int  // tries left
	same int  // answers in a row that the binds are the export's file
	swap bool // after a file was attached again, not after a start: a guest that never confirms is reported
	gen  int  // which attach this is about: an answer to an older one does not count
}

// fileWatch is the supervisor's state for the file sources of one server.
type fileWatch struct {
	srcs       []Source
	nests      []Nest
	mfds       []int  // the share mounts, by Source.Mount
	procfd     int    // /proc/PID of the sandbox's first process, held so the number cannot come to name another
	nsfd       int    // its mount namespace, which holds the exports
	server     string // the name of the server binary: the sandbox is complete when the process has become it
	ready      bool
	notify     func(int) (string, error) // tells the guest to look at export N's binds; nil when there is no guest
	record     string                    // Config.ProtectFile
	unrecorded map[int]bool              // Config.unrecorded
	logf       func(string, ...any)

	ids []fileID // what is attached, by source; unused for a directory
	att []int    // a descriptor of the attached file, or -1
	// pins are descriptors of files that were attached as a read-only source and then replaced. The server knows
	// such a file by its inode number, and the System VM gives a file another number once nothing holds it (measured:
	// run/TD-073/notes.txt 8), so each is held while this server runs, the newest maxPins of them.
	// The file a source had at the start is not among them: Run holds its tree until the server ends (council J-8).
	// The recorded ones (the newest MaxProtectSwaps) also have a mount that outlives this process (hold).
	pins []int
	// anchors are the directories on the way to the read-only sources (Config.anchors); checkChains keeps the
	// server told of them.
	anchors *anchorState
	// The server's list (Config.list). The server is told of a file there before the file is put at a protected
	// place. grown counts the bytes added; end ends the server, which then starts again with a list written anew.
	list   *os.File
	grown  int
	end    func()
	verify []int // checks left that read the attached file
	budget []int
	refill time.Time
	said   map[string]bool
	// covered counts down, for each nest (by its place in nests) that was found under another read-only mount, the
	// checks until it is looked at again. Such a nest is not in the sandbox's mount list and needs no mount.
	covered map[int]int
	waited  int // checks made before the sandbox was complete

	mu   sync.Mutex
	tell []notice
}

// newFileWatch holds the sandbox's process and mount namespace, opens the share mounts again and takes the identity
// of each file source from its tree, which is the file the sandbox attaches. It returns nil when no source is a
// file and no read-only source can lie in a read-write one (anchorState.watched).
func newFileWatch(cfg Config, trees []*os.File, pid int) (*fileWatch, error) {
	n := len(cfg.Sources)
	w := &fileWatch{srcs: cfg.Sources, nests: Nests(cfg.Sources), procfd: -1, nsfd: -1, server: filepath.Base(cfg.Unfsd),
		notify: cfg.NotifyGuest, record: cfg.ProtectFile, unrecorded: cfg.unrecorded, said: map[string]bool{}, covered: map[int]int{}, ids: make([]fileID, n), att: make([]int, n),
		verify: make([]int, n), budget: make([]int, n), tell: make([]notice, n), refill: time.Now(), list: cfg.list, anchors: cfg.anchors}
	w.logf = func(format string, a ...any) { fmt.Fprintf(os.Stderr, "vesper-unfs run: "+format+"\n", a...) }
	if real, err := filepath.EvalSymlinks(cfg.Unfsd); err == nil { // the process's exe link names the file itself
		w.server = filepath.Base(real)
	}
	if w.server == "." || w.server == "" {
		w.server = "unfsd"
	}
	// A server whose read-only sources can lie in a read-write one is watched too, though none of its sources is a
	// file: macOS can replace a directory on the way to a read-only directory source.
	any := cfg.anchors.watched(cfg.Sources)
	for i := range w.att {
		w.att[i], w.budget[i] = -1, swapBudget
	}
	for i, s := range cfg.Sources {
		if !s.File {
			continue
		}
		any = true
		id, err := idOf(int(trees[i].Fd()))
		if err != nil {
			return nil, fmt.Errorf("source %q: %w", s.Rel, err)
		}
		// A server that has come back may hold another file than the one the guest bound: let the guest look.
		w.ids[i], w.tell[i] = id, notice{left: noticesAfterStart}
	}
	if !any {
		return nil, nil
	}
	var err error
	if w.procfd, err = unix.Open(fmt.Sprintf("/proc/%d", pid), unix.O_PATH|unix.O_DIRECTORY|unix.O_CLOEXEC, 0); err != nil {
		return nil, fmt.Errorf("the sandbox's process: %w", err)
	}
	if w.nsfd, err = unix.Openat(w.procfd, "ns/mnt", unix.O_RDONLY|unix.O_CLOEXEC, 0); err != nil {
		w.close()
		return nil, fmt.Errorf("the sandbox's mount namespace: %w", err)
	}
	for _, m := range cfg.Mounts {
		mfd, err := unix.Open(m, unix.O_PATH|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
		if err != nil {
			w.close()
			return nil, fmt.Errorf("share mount %s: %w", m, err)
		}
		w.mfds = append(w.mfds, mfd)
	}
	return w, nil
}

func (w *fileWatch) close() {
	for _, fd := range append(append([]int{w.procfd, w.nsfd}, w.mfds...), w.att...) {
		if fd >= 0 {
			unix.Close(fd)
		}
	}
}

// run checks every FileCheck until stop closes. The notices to the guest go from a second loop, so a guest that
// answers slowly, or not at all, does not hold up the checks.
func (w *fileWatch) run(stop <-chan struct{}) {
	defer w.close()
	if w.notify != nil {
		go w.tellLoop(stop)
	}
	t := time.NewTicker(FileCheck)
	defer t.Stop()
	for {
		select {
		case <-stop:
			return
		case <-t.C:
			w.check()
		}
	}
}

// once reports a thing the first time it is seen, not every second.
func (w *fileWatch) once(key, format string, a ...any) {
	if !w.said[key] {
		w.said[key] = true
		w.logf(format, a...)
	}
}

// isReady reports whether the sandbox is complete: its first process has become the server. Before that the
// namespace still holds the System VM's own tree, where no path of the sandbox means anything.
func (w *fileWatch) isReady() bool {
	if !w.ready {
		buf := make([]byte, 256)
		n, err := unix.Readlinkat(w.procfd, "exe", buf)
		w.ready = err == nil && filepath.Base(string(buf[:n])) == w.server
	}
	return w.ready
}

// mountPoints lists where mounts sit in the sandbox's namespace, from its mountinfo (the fifth field, with its
// octal escapes undone).
func (w *fileWatch) mountPoints() (map[string]bool, error) {
	fd, err := unix.Openat(w.procfd, "mountinfo", unix.O_RDONLY|unix.O_CLOEXEC, 0)
	if err != nil {
		return nil, err
	}
	f := os.NewFile(uintptr(fd), "mountinfo")
	defer f.Close()
	raw, err := io.ReadAll(io.LimitReader(f, 4<<20))
	if err != nil {
		return nil, err
	}
	return parseMountPoints(string(raw)), nil
}

func parseMountPoints(mountinfo string) map[string]bool {
	out := map[string]bool{}
	for _, line := range strings.Split(mountinfo, "\n") {
		f := strings.Fields(line)
		if len(f) < 5 {
			continue
		}
		out[unescapeMount(f[4])] = true
	}
	return out
}

// unescapeMount undoes mountinfo's \ooo escapes (space, tab, newline and backslash).
func unescapeMount(s string) string {
	if !strings.Contains(s, `\`) {
		return s
	}
	var b strings.Builder
	for i := 0; i < len(s); i++ {
		if s[i] == '\\' && i+3 < len(s) {
			if v, err := strconv.ParseUint(s[i+1:i+4], 8, 8); err == nil {
				b.WriteByte(byte(v))
				i += 3
				continue
			}
		}
		b.WriteByte(s[i])
	}
	return b.String()
}

func (w *fileWatch) check() {
	if !w.isReady() {
		if w.waited++; w.waited == 30 {
			w.logf("the sandbox's first process has not become %s after %d checks; replaced files are not attached until it has", w.server, w.waited)
		}
		return
	}
	if time.Since(w.refill) >= swapRefill {
		w.refill = time.Now()
		for i := range w.budget {
			if w.budget[i] < swapBudget {
				w.budget[i]++
			}
		}
	}
	w.checkChains()
	files := false
	for _, s := range w.srcs {
		files = files || s.File
	}
	if !files {
		return // only the chains are watched
	}
	mounted, err := w.mountPoints()
	if err != nil {
		w.once("mountinfo", "cannot read the sandbox's mounts: %v", err)
		return
	}
	for i, s := range w.srcs {
		if s.File {
			w.checkOne(i, s, mounted)
		}
	}
}

// reads reports whether the file fd refers to (an O_PATH descriptor) can be read. A file that macOS replaced answers
// a read with ESTALE, though it may still open and show the new file's attributes.
func reads(fd int) bool {
	r, err := unix.Open(fmt.Sprintf("/proc/self/fd/%d", fd), unix.O_RDONLY|unix.O_NONBLOCK|unix.O_CLOEXEC, 0)
	if err == nil {
		var b [1]byte
		_, err = unix.Pread(r, b[:], 0)
		unix.Close(r)
	}
	return !(errors.Is(err, unix.ESTALE) || errors.Is(err, unix.ENOENT))
}

func (w *fileWatch) checkOne(i int, s Source, mounted map[string]bool) {
	// One try: the System VM can still answer for the old file for a moment after macOS replaced it, and the next
	// check is the next try.
	fd, err := openLive(w.mfds[s.Mount], s.Rel, true, time.Now())
	if err != nil {
		// No regular file there now: what is attached stays until one is.
		w.once(fmt.Sprintf("absent %d", i), "source %q: no regular file is there now (%v); the file attached stays until one is", s.Rel, err)
		return
	}
	defer unix.Close(fd)
	delete(w.said, fmt.Sprintf("absent %d", i))
	id, err := idOf(fd)
	if err != nil {
		return
	}
	replaced := id != w.ids[i]
	if !replaced && w.verify[i] > 0 {
		w.verify[i]--
		if w.att[i] >= 0 && !reads(w.att[i]) {
			w.logf("source %q: the file attached does not read; attaching the file at the path again", s.Rel)
			replaced = true
		}
	}
	// Every nest of this source goes again with a new file. Without one, only a nest that is gone: the kernel
	// drops a nest when the name it sits on goes, which a file moved away and back does without a new identity.
	var nests []Nest
	var which []int
	for k, n := range w.nests {
		if n.Inner != i {
			continue
		}
		if replaced {
			delete(w.covered, k)
		} else if mounted[ExportPath(n.Outer)+"/"+n.Sub] {
			continue
		} else if w.covered[k] > 0 {
			w.covered[k]--
			continue
		}
		nests, which = append(nests, n), append(which, k)
	}
	if !replaced && len(nests) == 0 {
		return
	}
	if w.budget[i] == 0 {
		w.once(fmt.Sprintf("budget %d", i), "source %q changes too often: not attached again for now", s.Rel)
		return
	}
	if replaced && !reads(fd) {
		return // the System VM still answers for the old file: the next check looks again
	}
	set := uint64(unix.MOUNT_ATTR_NOSUID | unix.MOUNT_ATTR_NODEV | unix.MOUNT_ATTR_NOEXEC)
	own := set
	if s.ReadOnly {
		own |= unix.MOUNT_ATTR_RDONLY
	}
	// A read-write file source whose path now names a recorded read-only object, or lies under one: an earlier start
	// of this server knew it as read-only, and this start has no nest on it (council S-3). It is attached read-only.
	guarded := false
	if replaced && !s.ReadOnly && w.record != "" {
		for _, k := range MovedUnder(w.srcs) {
			if k != i {
				continue
			}
			prior, err := ReadProtectIDs(w.record)
			under := false
			if err == nil && len(prior) > 0 {
				under, err = underKnown(w.mfds[s.Mount], s.Rel, knownIDs(prior))
			}
			if err != nil || under { // what could not be compared is attached read-only too, until the next attach
				guarded = true
				own |= unix.MOUNT_ATTR_RDONLY
			}
		}
	}
	var trees []int // the source's own tree when it was replaced, then one for each nest to place
	defer func() {
		for _, t := range trees {
			unix.Close(t)
		}
	}()
	if replaced {
		t, err := detach(fd, own)
		if err != nil {
			w.once(fmt.Sprintf("clone %d", i), "source %q was replaced; cloning the new file: %v", s.Rel, err)
			return
		}
		trees = append(trees, t)
	}
	for range nests {
		t, err := detach(fd, set|unix.MOUNT_ATTR_RDONLY)
		if err != nil {
			w.once(fmt.Sprintf("clone nest %d", i), "source %q: cloning its read-only nest: %v", s.Rel, err)
			return
		}
		trees = append(trees, t)
	}
	// A list that has grown past maxListGrowth is written anew: the server is ended and starts again, with the
	// list of a start. Each attach adds a line, and the list is on the System VM's shared /run.
	tell := replaced && hasPlace(w.srcs, s)
	if tell && w.grown > maxListGrowth {
		w.logf("the server's list has grown by %d bytes; the server is ended and starts again with a list written anew", w.grown)
		w.end()
		return
	}
	holds := Holders(w.srcs, i)
	var beneath, viaExport, keptRO bool
	failed, placed := 0, 0
	err = inMountNamespace(w.nsfd, func() error {
		rest := trees
		if replaced {
			tree := trees[0]
			if len(holds) > 0 { // the task can change what the path names: take the file as the task reaches it
				t, ro, err := takeThroughExport(holds, own)
				if err != nil {
					return err
				}
				defer unix.Close(t)
				// The file found through the export must be the file that was opened at the path: that is
				// the one the server is told of, and the one that is held and recorded. Another one was put
				// there in between (a second save), and the next check takes it (council D-1).
				got, err := idOf(t)
				if err != nil {
					return fmt.Errorf("the file reached through the export: %w", err)
				}
				if got != id {
					return fmt.Errorf("%w: %d:%d at the path, %d:%d through the export", errNotTheFileOpened, id.dev, id.ino, got.dev, got.ino)
				}
				tree, viaExport, keptRO = t, true, ro && !s.ReadOnly
			}
			// The server is told of the file just before the file is put at its protected place, with a line
			// in its list that it reads before it judges its next request. It then knows the file while it
			// runs, whether or not it looked while the file was there: a task can keep its server from looking
			// by keeping it in one request (TD-078). The line is added at every attach: the server forgets an
			// object after 768 newer ones, and one that is attached again must be told again.
			if tell {
				line := ProtectID{Dev: id.dev, Ino: id.ino}
				if err := AppendSeen(w.list, line); err != nil {
					return fmt.Errorf("%w: %w", errNotTold, err)
				}
				w.grown += len(seenLine(line))
			}
			var err error
			if beneath, err = swapFile(i, tree); err != nil {
				return err
			}
			rest = trees[1:]
		}
		for k, n := range nests {
			did, err := placeNest(n, rest[k], id)
			switch {
			case err != nil:
				failed++ // tried again at the next check; the swap above stands
				w.once(fmt.Sprintf("nest %d in %d", n.Inner, n.Outer), "source %q: its read-only nest in source %d is not in place: %v", s.Rel, n.Outer, err)
			case did:
				placed++
			default: // under another read-only mount: nothing to mount, and no need to look every second
				w.covered[which[k]] = coveredChecks
			}
		}
		return nil
	})
	// A look that found the place covered costs the source nothing, nor does one that found another file through
	// the export: nothing was mounted, and the source's nests must still be placed at later checks.
	if (replaced || placed > 0 || failed > 0) && !errors.Is(err, errNotTheFileOpened) {
		w.budget[i]--
	}
	if errors.Is(err, errNotTold) {
		if errors.Is(err, os.ErrClosed) {
			return // the server has ended and Run closed the list
		}
		// The file is not attached, and the old file's nest went with its name: the new file would stay
		// writable through the outer export for as long as the server cannot be told. So the server is ended.
		// The next start opens the sources by path, writes the list whole, or does not serve (council SEC-1).
		w.logf("source %q was replaced, and the server could not be told of the new file (%v); the server is ended and starts again with a list written anew", s.Rel, err)
		w.end()
		return
	}
	if err != nil {
		w.once(fmt.Sprintf("swap %d", i), "source %q was replaced; attaching the new file: %v", s.Rel, err)
		return
	}
	if replaced {
		how := "in place of the old one"
		if !beneath {
			how = "over the old one (the kernel refused to attach beneath)"
		}
		if viaExport {
			how += ", taken through the export that holds its path"
		}
		if keptRO {
			how += "; it lies under a read-only bind and stays read-only"
		}
		if guarded {
			how += "; it is, or lies under, an object that was bound read-only, and is attached read-only"
		}
		w.logf("source %q: another file is at its path; it is attached %s", s.Rel, how)
		if w.att[i] >= 0 {
			if s.ReadOnly {
				w.pins = append(w.pins, w.att[i])
				if len(w.pins) > maxPins {
					unix.Close(w.pins[0])
					w.pins = w.pins[1:]
				}
			} else {
				unix.Close(w.att[i])
			}
		}
		if w.att[i], err = unix.Dup(fd); err != nil {
			w.att[i] = -1
		} else {
			unix.CloseOnExec(w.att[i])
		}
		w.ids[i], w.verify[i] = id, verifyChecks
		// The server protects it by its place; a later start of the server needs its number, and the number is
		// the file's only while something holds the file: a mount that outlives this process (TD-076). The hold
		// comes first; when it fails, that is logged and the number is recorded all the same, and is then right
		// only while the System VM does not forget the file. The record's limit is the holds' limit. A source
		// that the start does not record (Config.unrecorded) is left out here too (council H-6, K-4).
		if s.ReadOnly && !w.unrecorded[i] {
			holds := HoldDir(w.record)
			if err := hold(holds, fd, true); err != nil {
				w.logf("source %q: its new file is not held across restarts: %v", s.Rel, err)
			}
			all, err := RecordProtectIDs(w.record, ProtectID{Dev: id.dev, Ino: id.ino, Swapped: true})
			if err != nil {
				w.logf("source %q: its new file is not in the record of read-only objects: %v", s.Rel, err)
			} else if _, left, err := ReleaseHolds(holds, all); left > 0 {
				w.logf("%d holds that the record no longer names are still there: %v", left, err)
			}
		}
		w.said = map[string]bool{} // a later failure is news again
		w.mu.Lock()
		w.tell[i] = notice{left: noticesAfterSwap, swap: true, gen: w.tell[i].gen + 1}
		w.mu.Unlock()
	} else if placed > 0 && failed == 0 {
		w.logf("source %q: its read-only nest was gone and is in place again", s.Rel)
	}
}

// takeThroughExport opens a file source's path beneath each read-write directory source that holds it, in the
// sandbox, and clones the file found. Mounts are crossed as an NFS request crosses them, so a file under a
// read-only nest is found on that nest's mount, and its clone is read-only; such a clone is preferred. ro reports
// that. set is added to the clone's attributes.
func takeThroughExport(holds []Nest, set uint64) (tree int, ro bool, err error) {
	tree = -1
	for _, h := range holds {
		at, e := openBeneathExport(h.Outer, h.Sub, unix.O_PATH|unix.O_CLOEXEC)
		if e != nil {
			err = fmt.Errorf("beneath source %d: %w", h.Outer, e)
			continue
		}
		var st unix.Stat_t
		var sfs unix.Statfs_t
		if e = unix.Fstat(at, &st); e == nil && st.Mode&unix.S_IFMT != unix.S_IFREG {
			e = errors.New("not a regular file")
		}
		if e == nil {
			e = unix.Fstatfs(at, &sfs)
		}
		var t int
		if e == nil {
			t, e = detach(at, set)
		}
		unix.Close(at)
		if e != nil {
			err = fmt.Errorf("beneath source %d: %w", h.Outer, e)
			continue
		}
		readOnly := sfs.Flags&unix.ST_RDONLY != 0
		if tree >= 0 && !readOnly {
			unix.Close(t)
			continue
		}
		if tree >= 0 {
			unix.Close(tree)
		}
		tree, ro = t, readOnly
		if ro {
			break
		}
	}
	if tree < 0 {
		return -1, false, err
	}
	return tree, ro, nil
}

// openBeneathExport opens sub beneath source outer's export in the sandbox, with no symbolic link on the way.
func openBeneathExport(outer int, sub string, flags uint64) (int, error) {
	dir, err := unix.Open(ExportPath(outer), unix.O_PATH|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return -1, err
	}
	defer unix.Close(dir)
	return unix.Openat2(dir, sub, &unix.OpenHow{
		Flags:   flags,
		Resolve: unix.RESOLVE_BENEATH | unix.RESOLVE_NO_SYMLINKS | unix.RESOLVE_NO_MAGICLINKS,
	})
}

// swapFile attaches tree at source i's export file, in the sandbox's namespace. It attaches beneath the old file's
// mount and then removes that one, so a lookup finds the old file or the new one and never the empty file under
// both. beneath is false when the kernel refused that and the tree went on top instead, where the old mount stays
// under it.
func swapFile(i int, tree int) (beneath bool, err error) {
	dst := ExportPath(i) + "/" + FileName // in the server's read-only root: nothing a guest can rename
	to, err := unix.Open(dst, unix.O_PATH|unix.O_NOFOLLOW|unix.O_CLOEXEC, 0)
	if err != nil {
		return false, err
	}
	defer unix.Close(to)
	const flags = unix.MOVE_MOUNT_F_EMPTY_PATH | unix.MOVE_MOUNT_T_EMPTY_PATH
	if err := unix.MoveMount(tree, "", to, "", flags|moveMountBeneath); err != nil {
		if err2 := unix.MoveMount(tree, "", to, "", flags); err2 != nil {
			return false, fmt.Errorf("move_mount beneath: %v; on top: %w", err, err2)
		}
		return false, nil
	}
	if err := unix.Unmount(dst, unix.MNT_DETACH|unix.UMOUNT_NOFOLLOW); err != nil {
		return true, fmt.Errorf("remove the old file's mount: %w", err)
	}
	return true, nil
}

// placeNest binds a read-only source again at its place inside a read-write one. The place is found beneath the
// outer export with no symbolic link, crossing mounts as an NFS request does. When it lies on a read-only mount it
// is covered already, by a nest that is still there, and nothing is done: a nest on top of an old one would go with
// it when the kernel drops the old one, and the check of the sandbox's mounts would then place it again. Otherwise
// the name must be the very file the tree was cloned from, as at the first placing. From the moment the kernel
// dropped the old nest until this runs, the file is writable through the outer export.
//
// placed is false with no error when the place was covered and nothing was mounted.
func placeNest(n Nest, tree int, want fileID) (placed bool, err error) {
	at, err := openBeneathExport(n.Outer, n.Sub, unix.O_PATH|unix.O_NOFOLLOW|unix.O_CLOEXEC)
	if err != nil {
		return false, err
	}
	defer unix.Close(at)
	var sfs unix.Statfs_t
	if err := unix.Fstatfs(at, &sfs); err != nil {
		return false, err
	}
	if sfs.Flags&unix.ST_RDONLY != 0 {
		return false, nil
	}
	got, err := idOf(at)
	if err != nil {
		return false, err
	}
	if got != want {
		return false, errors.New("the file there is not the one that was opened (it was replaced again, or the task put another there)")
	}
	return true, moveTree(tree, at)
}

// inMountNamespace runs fn on a thread that has entered the mount namespace ns. The thread is not used again: a
// goroutine that ends while locked to its thread takes the thread with it.
func inMountNamespace(ns int, fn func() error) error {
	done := make(chan error, 1)
	go func() {
		runtime.LockOSThread()
		// A thread that shares its root and working directory with others cannot change mount namespace.
		if err := unix.Unshare(unix.CLONE_FS); err != nil {
			done <- fmt.Errorf("unshare the thread's filesystem state: %w", err)
			return
		}
		if err := unix.Setns(ns, unix.CLONE_NEWNS); err != nil {
			done <- fmt.Errorf("enter the sandbox's mount namespace: %w", err)
			return
		}
		done <- fn()
	}()
	return <-done
}

// tellLoop sends the notices. A source is told until the guest has answered settledAnswers times in a row that its
// binds are the export's file ("same"), or that it has none ("none"), or its tries run out.
func (w *fileWatch) tellLoop(stop <-chan struct{}) {
	t := time.NewTicker(FileCheck)
	defer t.Stop()
	gaveUp := map[int]bool{}
	for {
		select {
		case <-stop:
			return
		case <-t.C:
		}
		for i := range w.srcs {
			w.mu.Lock()
			n := w.tell[i]
			w.mu.Unlock()
			if n.left <= 0 {
				continue
			}
			answer, err := w.notify(i)
			w.mu.Lock()
			if w.tell[i].gen != n.gen { // attached again meanwhile: the answer was about the file before
				w.mu.Unlock()
				delete(gaveUp, i)
				continue
			}
			n.left--
			if err == nil && (answer == "same" || answer == "none") {
				n.same++
			} else {
				n.same = 0
			}
			if n.same >= settledAnswers {
				n.left = 0
			}
			w.tell[i] = n
			w.mu.Unlock()
			if n.left == 0 && n.same < settledAnswers && n.swap && !gaveUp[i] {
				gaveUp[i] = true
				fmt.Fprintf(os.Stderr, "vesper-unfs run: source %q: the guest did not confirm that it bound the file again (last answer %q, %v)\n", w.srcs[i].Rel, answer, err)
			}
		}
	}
}

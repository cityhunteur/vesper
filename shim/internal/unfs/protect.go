package unfs

import (
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"
)

// Objects the server keeps read-only whatever name reaches them (TD-073, unfs3 patch 0006).
//
// A read-only source that lies inside a read-write one used to be kept read-only by a mount on its name in the outer
// export alone (a nest). A task reaches the object under other names: in another letter case on a macOS volume that
// ignores case, through a hard link, or after it moved a directory above it into another read-write export. So the
// server is told which objects are read-only, and refuses a request that changes one, or anything under one, by its
// device and inode number.
//
// The server reads "<exports file>.protect". A "p PATH" line names a place in the sandbox's own root, which a task
// cannot change: the root of each read-only source. The server looks at it before every request that changes
// something, so a file that replaced a read-only file source (D-052) is protected from the first such request after
// it is attached, and it remembers every object it found there, so that file stays protected after a task moved it
// aside. An "i DEV INO" line names an
// object by number: what a read-only source was at an earlier start of this server. A task that renamed directories
// can make a source's path name another object by the time the server starts again; the object the user bound
// read-only stays in the list.
//
// A number is an object's only while something holds the object: the System VM gives a file another number when it
// has forgotten the file and looks it up again. What a running server serves is held (the sandbox's mounts, and
// fileWatch.pins for replaced files). Every recorded object also has a mount of its own that outlives the
// supervisor (hold_linux.go), so its "i" line stays right after a task moved it away and the server started again
// (TD-076).

// ProtectID is an object's device and inode number. Swapped marks one recorded when a file replaced a read-only file
// source (D-052), as against what a source was at a start of the server.
type ProtectID struct {
	Dev, Ino uint64
	Swapped  bool `json:"swapped,omitempty"`
}

func (p ProtectID) same(o ProtectID) bool { return p.Dev == o.Dev && p.Ino == o.Ino }

// The server holds 256 numbers. The record keeps the first MaxProtectStarts it saw at starts, because the first are
// what the user bound and a task can add more (after it renamed directories, a later start sees other objects at the
// paths), and the newest MaxProtectSwaps from replaced files.
const (
	MaxProtectStarts = 150
	MaxProtectSwaps  = 50
)

// ProtectPaths are the places in the sandbox whose objects are read-only: the root of each read-only source.
func ProtectPaths(srcs []Source) []string {
	var out []string
	for i, s := range srcs {
		if !s.ReadOnly || alsoReadWrite(srcs, s) {
			continue
		}
		p := ExportPath(i)
		if s.File {
			p += "/" + FileName
		}
		out = append(out, p)
	}
	return out
}

// alsoReadWrite reports whether the object of read-only source s is a read-write source too. The shim's planner makes
// such a bind one read-write source; given as two, the read-write one must stay writable (council COR-8).
func alsoReadWrite(srcs []Source, s Source) bool {
	for _, o := range srcs {
		if !o.ReadOnly && o.Mount == s.Mount && o.Rel == s.Rel && o.File == s.File {
			return true
		}
	}
	return false
}

// ProtectList is the server's protect file.
func ProtectList(srcs []Source, ids []ProtectID) string {
	var b strings.Builder
	for _, p := range ProtectPaths(srcs) {
		b.WriteString("p " + p + "\n")
	}
	for _, id := range ids {
		fmt.Fprintf(&b, "i %d %d\n", id.Dev, id.Ino)
	}
	return b.String()
}

// ParseProtectIDs reads the supervisor's record, one "DEV INO" a line, with " r" after a replaced file's; a line it
// cannot read is skipped.
func ParseProtectIDs(raw string) []ProtectID {
	var out []ProtectID
	for _, line := range strings.Split(raw, "\n") {
		f := strings.Fields(line)
		if len(f) != 2 && !(len(f) == 3 && f[2] == "r") {
			continue
		}
		dev, err1 := strconv.ParseUint(f[0], 10, 64)
		ino, err2 := strconv.ParseUint(f[1], 10, 64)
		if err1 == nil && err2 == nil {
			out = append(out, ProtectID{Dev: dev, Ino: ino, Swapped: len(f) == 3})
		}
	}
	return out
}

// MergeProtectIDs adds ids to old, each object once, within the two bounds: a start's number is dropped when
// MaxProtectStarts are held, and the oldest replaced file's when MaxProtectSwaps are.
func MergeProtectIDs(old []ProtectID, ids ...ProtectID) []ProtectID {
	var starts, swaps []ProtectID
	has := func(list []ProtectID, id ProtectID) bool {
		for _, o := range list {
			if o.same(id) {
				return true
			}
		}
		return false
	}
	for _, id := range append(append([]ProtectID(nil), old...), ids...) {
		switch {
		case has(starts, id):
		case !id.Swapped && has(swaps, id) && len(starts) < MaxProtectStarts:
			// A start found, as a source, a file once recorded as a replacement: it is kept as a source now.
			kept := swaps[:0]
			for _, o := range swaps {
				if !o.same(id) {
					kept = append(kept, o)
				}
			}
			swaps = kept
			starts = append(starts, id)
		case has(swaps, id):
		case id.Swapped:
			swaps = append(swaps, id)
		case len(starts) < MaxProtectStarts:
			starts = append(starts, id)
		}
	}
	if len(swaps) > MaxProtectSwaps {
		swaps = swaps[len(swaps)-MaxProtectSwaps:]
	}
	return append(starts, swaps...)
}

// ReadProtectIDs reads the record at file; a missing or unnamed file is an empty record.
func ReadProtectIDs(file string) ([]ProtectID, error) {
	if file == "" {
		return nil, nil
	}
	raw, err := os.ReadFile(file)
	if err != nil && !os.IsNotExist(err) {
		return nil, err
	}
	return ParseProtectIDs(string(raw)), nil
}

// RecordProtectIDs adds ids to the record at file (made 0600 beside the server's arguments) and returns all of it.
// An empty file name keeps no record.
func RecordProtectIDs(file string, ids ...ProtectID) ([]ProtectID, error) {
	old, err := ReadProtectIDs(file)
	if err != nil {
		return nil, err
	}
	all := MergeProtectIDs(old, ids...)
	if file == "" || reflect.DeepEqual(all, old) || (len(all) == 0 && len(old) == 0) {
		return all, nil // nothing new: no file is written
	}
	var b strings.Builder
	for _, id := range all {
		fmt.Fprintf(&b, "%d %d", id.Dev, id.Ino)
		if id.Swapped {
			b.WriteString(" r")
		}
		b.WriteString("\n")
	}
	tmp := filepath.Join(filepath.Dir(file), "."+filepath.Base(file)+".tmp")
	if err := os.WriteFile(tmp, []byte(b.String()), 0o600); err != nil {
		return nil, err
	}
	return all, os.Rename(tmp, file)
}

// MovedUnder lists the read-write sources that the configured paths do not put inside a read-only source of the
// same share, for the caller to compare their place with the record (GuardSources): a read-write bind that the user
// put inside a read-only one stays writable through its own export.
func MovedUnder(srcs []Source) []int {
	var out []int
	for i, s := range srcs {
		if s.ReadOnly {
			continue
		}
		inside := false
		for _, r := range srcs {
			if r.ReadOnly && !r.File && r.Mount == s.Mount {
				if _, ok := within(r.Rel, s.Rel); ok {
					inside = true
				}
			}
		}
		if !inside {
			out = append(out, i)
		}
	}
	return out
}

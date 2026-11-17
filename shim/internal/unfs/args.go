package unfs

import (
	"errors"
	"fmt"
	"strings"
)

// ArgsHeader opens a version 2 server record (GH #207). vesper-microvm writes the record, `vesper-unfs run --args`
// and the selfcheck start from it, and the fanout reads it, so all three parse it here:
//
//	#vesper-unfs-args 2
//	squash 0|1
//	mount /share/one
//	src REL:ro|rw          (a source of the mount above it)
//	mount /share/two
//	src REL:ro|rw
//
// A version 1 record, written before GH #207, is the mount, the squash flag, then one REL:mode per line.
const ArgsHeader = "#vesper-unfs-args 2"

// MaxMounts bounds the share mounts of one server.
const MaxMounts = 16

// Args is what a server is started with.
type Args struct {
	Mounts  []string
	Squash  bool
	Sources []Source
}

// ParseArgs reads a record. Any line it cannot read refuses the whole record: a source skipped would shift every
// later source to another export (council COR-9).
func ParseArgs(raw string) (Args, error) {
	lines := strings.Split(strings.TrimRight(raw, "\n"), "\n")
	if len(lines) == 0 || lines[0] != ArgsHeader {
		return parseArgsV1(lines)
	}
	var a Args
	squashSeen := false
	for n, l := range lines[1:] {
		bad := func(why string) (Args, error) { return Args{}, fmt.Errorf("record line %d: %s", n+2, why) }
		switch {
		case l == "squash 0" || l == "squash 1":
			if squashSeen {
				return bad("a second squash line")
			}
			squashSeen, a.Squash = true, l == "squash 1"
		case strings.HasPrefix(l, "mount "):
			if err := CheckMount(l[len("mount "):]); err != nil {
				return bad(err.Error())
			}
			a.Mounts = append(a.Mounts, l[len("mount "):])
		case strings.HasPrefix(l, "src "):
			if len(a.Mounts) == 0 {
				return bad("a source before any mount")
			}
			s, err := ParseSource(l[len("src "):])
			if err != nil {
				return bad(err.Error())
			}
			s.Mount = len(a.Mounts) - 1
			a.Sources = append(a.Sources, s)
		default:
			return bad("unknown line")
		}
	}
	if !squashSeen {
		return Args{}, errors.New("record: no squash line")
	}
	return a, a.Check()
}

func parseArgsV1(lines []string) (Args, error) {
	if len(lines) < 3 {
		return Args{}, errors.New("record: too short")
	}
	a := Args{Mounts: []string{lines[0]}}
	switch lines[1] {
	case "0":
	case "1":
		a.Squash = true
	default:
		return Args{}, fmt.Errorf("record: squash %q", lines[1])
	}
	if err := CheckMount(lines[0]); err != nil {
		return Args{}, fmt.Errorf("record: %w", err)
	}
	for n, l := range lines[2:] {
		s, err := ParseSource(l)
		if err != nil {
			return Args{}, fmt.Errorf("record line %d: %w", n+3, err)
		}
		a.Sources = append(a.Sources, s)
	}
	return a, a.Check()
}

// Format writes a version 2 record. Sources are grouped under their mounts in mount order, so a record keeps
// source order only when sources are already grouped; Check asks for that.
func (a Args) Format() (string, error) {
	if err := a.Check(); err != nil {
		return "", err
	}
	var b strings.Builder
	b.WriteString(ArgsHeader + "\n")
	sq := "0"
	if a.Squash {
		sq = "1"
	}
	b.WriteString("squash " + sq + "\n")
	for m, mount := range a.Mounts {
		b.WriteString("mount " + mount + "\n")
		for _, s := range a.Sources {
			if s.Mount == m {
				b.WriteString("src " + s.Arg() + "\n")
			}
		}
	}
	return b.String(), nil
}

// Check validates mounts and sources together: every mount absolute, distinct, with no newline; at most MaxMounts;
// every source on a mount that exists, with each mount's sources together and in mount order (so /s/N numbering is
// the order a record lists them); and every mount used.
func (a Args) Check() error {
	if len(a.Mounts) == 0 {
		return errors.New("no share mount")
	}
	if len(a.Mounts) > MaxMounts {
		return fmt.Errorf("%d share mounts; at most %d", len(a.Mounts), MaxMounts)
	}
	seen := map[string]bool{}
	for _, m := range a.Mounts {
		if err := CheckMount(m); err != nil {
			return err
		}
		if seen[m] {
			return fmt.Errorf("share mount %s given twice", m)
		}
		seen[m] = true
	}
	if err := Check(a.Sources); err != nil {
		return err
	}
	used := make([]bool, len(a.Mounts))
	last := 0
	for _, s := range a.Sources {
		if s.Mount < 0 || s.Mount >= len(a.Mounts) {
			return fmt.Errorf("source %q: no mount %d", s.Rel, s.Mount)
		}
		if s.Mount < last {
			return fmt.Errorf("source %q: sources must follow their mounts' order", s.Rel)
		}
		last, used[s.Mount] = s.Mount, true
	}
	for m, u := range used {
		if !u {
			return fmt.Errorf("share mount %s has no source", a.Mounts[m])
		}
	}
	return nil
}

// CheckMount accepts an absolute path with no newline or NUL.
func CheckMount(m string) error {
	switch {
	case !strings.HasPrefix(m, "/"):
		return fmt.Errorf("share mount %q: not absolute", m)
	case strings.ContainsAny(m, "\n\x00"):
		return fmt.Errorf("share mount %q: a newline or NUL", m)
	}
	return nil
}

// Arg is a source's REL:ro or REL:rw form.
func (s Source) Arg() string {
	if s.ReadOnly {
		return s.Rel + ":ro"
	}
	return s.Rel + ":rw"
}

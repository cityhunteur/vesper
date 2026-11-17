// Package unfs builds the sandbox of one unfs3 server for one shim microVM (ADR 0009, GH #160 M3).
//
// This file is the part that needs no system calls: which sources a server exports, where each read-only source is
// bound again inside a read-write one, and the exports file. The sandbox itself is in sandbox_linux.go.
package unfs

import (
	"errors"
	"fmt"
	"sort"
	"strings"
)

const (
	// ServerAddr and GuestAddr are the fixed link-local pair of every share namespace (ADR 0009, Network).
	ServerAddr = "169.254.77.1"
	GuestAddr  = "169.254.77.2"
	// NFSPort and MountPort are unfsd's ports; vesper-microvm's INPUT rule admits them from the tap only.
	NFSPort   = 2049
	MountPort = 2050
	// SquashID is the uid and gid of a server that does not run as root (ADR 0009, Privilege model).
	SquashID = 65534
	// MaxSources bounds the exports of one server.
	MaxSources = 64
	// maxRel bounds a source path relative to its share mount.
	maxRel = 4096
)

// Source is one directory a server exports, as a path relative to one of its trusted share mounts (Mount is that
// mount's index; GH #207).
type Source struct {
	Mount    int    `json:"mount,omitempty"`
	Rel      string `json:"rel"`
	ReadOnly bool   `json:"ro"`
}

// Nest says that read-only source Inner is bound again, read-only, at Sub inside read-write source Outer, so the
// outer export cannot write what the task mounted read-only (ADR 0009, Sandbox).
type Nest struct {
	Outer int    `json:"outer"`
	Inner int    `json:"inner"`
	Sub   string `json:"sub"`
}

// ParseSource reads REL:ro or REL:rw. REL is "." for the whole share, or a relative path of plain components.
func ParseSource(arg string) (Source, error) {
	i := strings.LastIndexByte(arg, ':')
	if i < 0 {
		return Source{}, fmt.Errorf("source %q: want REL:ro or REL:rw", arg)
	}
	rel, mode := arg[:i], arg[i+1:]
	var ro bool
	switch mode {
	case "ro":
		ro = true
	case "rw":
	default:
		return Source{}, fmt.Errorf("source %q: mode must be ro or rw", arg)
	}
	if err := CheckRel(rel); err != nil {
		return Source{}, err
	}
	return Source{Rel: rel, ReadOnly: ro}, nil
}

// CheckRel accepts "." or a relative path whose components are neither empty, ".", nor "..".
func CheckRel(rel string) error {
	switch {
	case rel == ".":
		return nil
	case rel == "":
		return errors.New("empty source path")
	case len(rel) > maxRel:
		return fmt.Errorf("source path longer than %d bytes", maxRel)
	case strings.ContainsAny(rel, "\x00\n"):
		return errors.New("source path holds a NUL byte or a newline")
	case strings.HasPrefix(rel, "/"):
		return fmt.Errorf("source %q: must be relative to the share mount", rel)
	}
	for _, c := range strings.Split(rel, "/") {
		if c == "" || c == "." || c == ".." {
			return fmt.Errorf("source %q: empty, . or .. component", rel)
		}
	}
	return nil
}

// within reports where inner lies below outer, as a relative path; ok is false when it does not, or when the two
// are the same directory (a task that mounts one directory both ways keeps its read-write mount writable).
func within(outer, inner string) (sub string, ok bool) {
	if outer == inner {
		return "", false
	}
	if outer == "." {
		return inner, inner != "."
	}
	if strings.HasPrefix(inner, outer+"/") {
		return inner[len(outer)+1:], true
	}
	return "", false
}

// Nests lists every read-only source inside a read-write one on the same share mount (sources on two mounts never
// nest: council ARC-8), shallowest first, so a deeper bind lands on top of
// a shallower one.
func Nests(srcs []Source) []Nest {
	var out []Nest
	for i, o := range srcs {
		if o.ReadOnly {
			continue
		}
		for j, in := range srcs {
			if !in.ReadOnly || in.Mount != o.Mount {
				continue
			}
			if sub, ok := within(o.Rel, in.Rel); ok {
				out = append(out, Nest{Outer: i, Inner: j, Sub: sub})
			}
		}
	}
	sort.SliceStable(out, func(a, b int) bool {
		da, db := strings.Count(out[a].Sub, "/"), strings.Count(out[b].Sub, "/")
		if da != db {
			return da < db
		}
		return out[a].Outer < out[b].Outer
	})
	return out
}

// ExportPath is where source i appears in the server's root, and the path the guest mounts.
func ExportPath(i int) string { return fmt.Sprintf("/s/%d", i) }

// Exports is the server's exports file: generated paths only, so no source path reaches it, and only the guest's
// address. A root server keeps the caller's uid (no_root_squash); a squashing one acts as SquashID for everyone.
func Exports(srcs []Source, squash bool) string {
	var b strings.Builder
	for i, s := range srcs {
		mode := "rw"
		if s.ReadOnly {
			mode = "ro"
		}
		opts := mode + ",no_root_squash"
		if squash {
			opts = fmt.Sprintf("%s,all_squash,anonuid=%d,anongid=%d", mode, SquashID, SquashID)
		}
		fmt.Fprintf(&b, "%s %s(%s)\n", ExportPath(i), GuestAddr, opts)
	}
	return b.String()
}

// Check validates a whole source list.
func Check(srcs []Source) error {
	if len(srcs) == 0 {
		return errors.New("no sources")
	}
	if len(srcs) > MaxSources {
		return fmt.Errorf("%d sources; at most %d", len(srcs), MaxSources)
	}
	for _, s := range srcs {
		if err := CheckRel(s.Rel); err != nil {
			return err
		}
	}
	return nil
}

// UnfsdArgs is the server's command line (ADR 0009, Server): foreground, no portmap, TCP only, the server address
// alone.
func UnfsdArgs(exports string) []string {
	return []string{"-d", "-p", "-t", "-l", ServerAddr, "-n", fmt.Sprint(NFSPort), "-m", fmt.Sprint(MountPort), "-e", exports}
}

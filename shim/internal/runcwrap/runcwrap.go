// Package runcwrap holds the logic of vesper-runc, the runtime dockerd uses in the System VM (TD-062, ADR 0010).
//
// A macOS share (Apple's virtio-fs) reports every file as owned by whoever asks, and the System VM caches that answer
// for about a second for every uid. dockerd and runc look up each bind source as root while they create a container,
// so for that second a remapped container sees its bind mounts owned by uid 0, which it shows as "nobody": chmod and
// touch fail, and so can git. After runc creates the container, and before its process starts, vesper-runc asks the
// share again as the container's uid, so the cache holds the owner the container expects.
package runcwrap

import (
	"path/filepath"
	"strconv"
	"strings"

	specs "github.com/opencontainers/runtime-spec/specs-go"
)

// Call is what vesper-runc needs to know about one runc command line.
type Call struct {
	Command     string // the runc subcommand, "" when there is none
	Bundle      string // create: the bundle directory, "" for runc's default (the working directory)
	PreserveFDs int    // create: --preserve-fds, the descriptors after stdio that runc passes to the container
}

// runc's global flags that take a value; the others are booleans. urfave/cli accepts one or two dashes.
var globalValue = map[string]bool{"root": true, "log": true, "log-format": true, "criu": true, "rootless": true}

// create's flags that take a value.
var createValue = map[string]bool{"bundle": true, "b": true, "console-socket": true, "pid-file": true, "preserve-fds": true, "pidfd-socket": true}

func flagName(arg string) (name, value string, inline bool) {
	name = strings.TrimLeft(arg, "-")
	if i := strings.IndexByte(name, '='); i >= 0 {
		return name[:i], name[i+1:], true
	}
	return name, "", false
}

// Parse reads a runc command line (without the program name). It never fails: a line it does not understand is
// passed to runc unchanged, and a create it cannot read gets no refresh.
func Parse(args []string) Call {
	var c Call
	i := 0
	for ; i < len(args); i++ {
		a := args[i]
		if a == "--" || !strings.HasPrefix(a, "-") || a == "-" {
			break
		}
		if name, _, inline := flagName(a); globalValue[name] && !inline {
			i++
		}
	}
	if i >= len(args) || args[i] == "--" {
		return c
	}
	c.Command = args[i]
	if c.Command != "create" {
		return c
	}
	for j := i + 1; j < len(args); j++ {
		a := args[j]
		if a == "--" {
			break
		}
		if !strings.HasPrefix(a, "-") || a == "-" {
			continue // the container id
		}
		name, value, inline := flagName(a)
		if !createValue[name] {
			continue
		}
		if !inline {
			if j+1 >= len(args) {
				break
			}
			j++
			value = args[j]
		}
		switch name {
		case "bundle", "b":
			c.Bundle = value
		case "preserve-fds":
			if n, err := strconv.Atoi(value); err == nil && n > 0 {
				c.PreserveFDs = n
			}
		}
	}
	return c
}

// Result counts what Refresh did.
type Result struct {
	Refreshed, Skipped int
	Errors             []error
}

// Target is what to ask the share again, and as whom.
type Target struct {
	Paths    []string // bind sources, absolute, each once
	UID, GID uint32   // the container process's uid and gid in the System VM
}

// hostID maps an id in the container to the System VM through the spec's mappings; no mappings means the same id.
func hostID(id uint32, maps []specs.LinuxIDMapping) (uint32, bool) {
	if len(maps) == 0 {
		return id, true
	}
	for _, m := range maps {
		if id >= m.ContainerID && uint64(id) < uint64(m.ContainerID)+uint64(m.Size) {
			return m.HostID + (id - m.ContainerID), true
		}
	}
	return 0, false
}

func isBind(m specs.Mount) bool {
	if m.Type == "bind" {
		return true
	}
	for _, o := range m.Options {
		if o == "bind" || o == "rbind" {
			return true
		}
	}
	return false
}

// TargetOf returns the bind sources of a container and the uid and gid its process has in the System VM. ok is false
// when nothing needs asking again: the process runs as root in the System VM (root's answer is already the cached
// one), its ids are outside the mappings, or it has no bind mounts.
func TargetOf(s *specs.Spec) (t Target, ok bool) {
	if s == nil || s.Process == nil {
		return t, false
	}
	var uidMaps, gidMaps []specs.LinuxIDMapping
	if s.Linux != nil {
		uidMaps, gidMaps = s.Linux.UIDMappings, s.Linux.GIDMappings
	}
	uid, uok := hostID(s.Process.User.UID, uidMaps)
	gid, gok := hostID(s.Process.User.GID, gidMaps)
	if !uok || !gok || uid == 0 {
		return t, false
	}
	t.UID, t.GID = uid, gid
	seen := map[string]bool{}
	for _, m := range s.Mounts {
		if !isBind(m) || !filepath.IsAbs(m.Source) {
			continue
		}
		p := filepath.Clean(m.Source)
		if !seen[p] {
			seen[p] = true
			t.Paths = append(t.Paths, p)
		}
	}
	return t, len(t.Paths) > 0
}

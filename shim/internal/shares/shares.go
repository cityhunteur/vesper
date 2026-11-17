// Package shares maps a task's OCI bind mounts from macOS shares onto the NFS exports of its microVM's share server
// (ADR 0009, GH #160 M4 and M5), and carries the resulting list to the guest.
//
// The shim plans the list (Plan): every bind mount whose resolved source lies under a virtio-fs share mount becomes
// one export of the microVM's unfsd and one NFS mount in the guest. The list travels inside the session's RUN
// request, as an annotation of the guest's OCI config, and vesper-console mounts it under the task's root before
// the workload starts.
package shares

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strings"

	specs "github.com/opencontainers/runtime-spec/specs-go"

	"github.com/cityhunteur/vesper/shim/internal/unfs"
)

// Annotation is the key of the share list in the guest's OCI config.
const Annotation = "io.vesper.shares"

// Share is one NFS mount in the guest: the export Export of the microVM's server at Dest under the task's root.
type Share struct {
	Dest     string `json:"dest"`
	Export   string `json:"export"`
	ReadOnly bool   `json:"ro"`
}

// MountOptions are the NFS options of every share (ADR 0009, Guest), without "ro".
func MountOptions() string {
	return fmt.Sprintf("addr=%s,vers=3,proto=tcp,port=%d,mountport=%d,mountproto=tcp,nolock,hard,actimeo=1,lookupcache=positive",
		unfs.ServerAddr, unfs.NFSPort, unfs.MountPort)
}

// CheckDest accepts an absolute, clean path other than "/" with no NUL.
func CheckDest(dest string) error {
	switch {
	case !strings.HasPrefix(dest, "/"):
		return fmt.Errorf("share destination %q is not absolute", dest)
	case dest == "/":
		return errors.New("a share cannot be mounted on the task's root")
	case strings.ContainsAny(dest, "\x00\n"):
		return errors.New("share destination holds a NUL or newline")
	case path.Clean(dest) != dest:
		return fmt.Errorf("share destination %q is not clean", dest)
	case len(dest) > 4096:
		return errors.New("share destination longer than 4096 bytes")
	}
	return nil
}

func checkExport(e string, n int) error {
	for i := 0; i < n; i++ {
		if e == unfs.ExportPath(i) {
			return nil
		}
	}
	return fmt.Errorf("share export %q is not one of /s/0../s/%d", e, n-1)
}

// Check validates a list as the guest receives it.
func Check(list []Share) error {
	if len(list) > unfs.MaxSources {
		return fmt.Errorf("%d shares; at most %d", len(list), unfs.MaxSources)
	}
	for _, s := range list {
		if err := CheckDest(s.Dest); err != nil {
			return err
		}
		if err := checkExport(s.Export, unfs.MaxSources); err != nil {
			return err
		}
	}
	return nil
}

// Sorted returns the list shallowest destination first, so a mount inside another lands on top of it.
func Sorted(list []Share) []Share {
	out := append([]Share(nil), list...)
	sort.SliceStable(out, func(a, b int) bool {
		da, db := strings.Count(out[a].Dest, "/"), strings.Count(out[b].Dest, "/")
		if da != db {
			return da < db
		}
		return out[a].Dest < out[b].Dest
	})
	return out
}

// FromAnnotations reads the list from an OCI config's annotations; no annotation is an empty list.
func FromAnnotations(a map[string]string) ([]Share, error) {
	raw, ok := a[Annotation]
	if !ok || raw == "" {
		return nil, nil
	}
	var list []Share
	if err := json.Unmarshal([]byte(raw), &list); err != nil {
		return nil, fmt.Errorf("share list: %w", err)
	}
	return list, Check(list)
}

// Encode is the annotation value of a list.
func Encode(list []Share) (string, error) {
	if err := Check(list); err != nil {
		return "", err
	}
	b, err := json.Marshal(list)
	return string(b), err
}

// Plan is what the shim asks for: the share mounts and sources for vesper-microvm launch --unfs-*, and the guest's
// list. An empty plan (no Sources) means the task has no share-backed bind mounts. Sources are grouped by mount, in
// Mounts order (unfs.Args.Check).
type Plan struct {
	Mounts  []string
	Sources []unfs.Source
	Shares  []Share
}

func isBind(m specs.Mount) bool {
	if m.Type == "bind" || m.Type == "rbind" {
		return true
	}
	for _, o := range m.Options {
		if o == "bind" || o == "rbind" {
			return true
		}
	}
	return false
}

func readOnly(m specs.Mount) bool {
	ro := false
	for _, o := range m.Options {
		switch o {
		case "ro":
			ro = true
		case "rw":
			ro = false
		}
	}
	return ro
}

// shareOf returns the share mount that holds p (the longest match), or "".
func shareOf(p string, shareMounts []string) string {
	best := ""
	for _, sm := range shareMounts {
		if (p == sm || strings.HasPrefix(p, sm+"/")) && len(sm) > len(best) {
			best = sm
		}
	}
	return best
}

// Resolver resolves a source path as the System VM sees it (symbolic links followed) and says whether it is a
// directory. The shim passes filepath.EvalSymlinks and os.Stat; tests pass fakes.
type Resolver struct {
	Resolve func(string) (string, error)
	IsDir   func(string) (bool, error)
}

// HostResolver resolves on the System VM's file system.
var HostResolver = Resolver{
	Resolve: filepath.EvalSymlinks,
	IsDir: func(p string) (bool, error) {
		st, err := os.Stat(p)
		if err != nil {
			return false, err
		}
		return st.IsDir(), nil
	},
}

// MakePlan maps a spec's mounts. A bind mount whose resolved source lies under one of shareMounts (the virtio-fs
// mount points) becomes a share; a file source is refused; other mounts are left as they were (ADR 0009, Shim). The
// sources may come from several share mounts, one server serving them all (GH #207). A source that holds another
// share mount is refused: it is cloned without its submounts, so the guest would see, and could write, the outer
// share's view of that directory, even when the inner share is read-only (council JDG-1, ARC-9).
func MakePlan(mounts []specs.Mount, shareMounts []string, r Resolver) (Plan, error) {
	var p Plan
	index := map[unfs.Source]int{}
	mountIndex := map[string]int{}
	for _, m := range mounts {
		if !isBind(m) || !filepath.IsAbs(m.Source) {
			continue
		}
		src, err := r.Resolve(m.Source)
		if err != nil {
			// A source that does not resolve is not a share; the mount keeps today's behaviour.
			continue
		}
		sm := shareOf(src, shareMounts)
		if sm == "" {
			continue
		}
		dir, err := r.IsDir(src)
		if err != nil {
			return Plan{}, fmt.Errorf("mount %s: %w", m.Destination, err)
		}
		if !dir {
			return Plan{}, fmt.Errorf("mount %s: the source %s is a file; only directories from a share can be mounted in a microVM", m.Destination, m.Source)
		}
		for _, other := range shareMounts {
			if other != sm && (other == src || strings.HasPrefix(other, src+"/")) {
				return Plan{}, fmt.Errorf("mount %s: the source %s holds another share, %s; bind a directory inside one share", m.Destination, m.Source, other)
			}
		}
		if err := CheckDest(path.Clean(m.Destination)); err != nil {
			return Plan{}, fmt.Errorf("mount %s: %w", m.Destination, err)
		}
		rel := "."
		if src != sm {
			rel = src[len(sm)+1:]
		}
		if err := unfs.CheckRel(rel); err != nil {
			return Plan{}, fmt.Errorf("mount %s: %w", m.Destination, err)
		}
		mi, ok := mountIndex[sm]
		if !ok {
			mi = len(p.Mounts)
			mountIndex[sm] = mi
			p.Mounts = append(p.Mounts, sm)
		}
		// The key holds the mount: the same relative path in two shares is two sources (council SEC-4).
		s := unfs.Source{Mount: mi, Rel: rel, ReadOnly: readOnly(m)}
		i, ok := index[s]
		if !ok {
			i = len(p.Sources)
			index[s] = i
			p.Sources = append(p.Sources, s)
		}
		p.Shares = append(p.Shares, Share{Dest: path.Clean(m.Destination), Export: unfs.ExportPath(i), ReadOnly: s.ReadOnly})
	}
	if len(p.Sources) > unfs.MaxSources {
		return Plan{}, fmt.Errorf("%d share sources; at most %d", len(p.Sources), unfs.MaxSources)
	}
	if len(p.Mounts) > unfs.MaxMounts {
		return Plan{}, fmt.Errorf("binds from %d shares; at most %d", len(p.Mounts), unfs.MaxMounts)
	}
	p.group()
	return p, nil
}

// group orders the sources by mount, keeping their order within a mount, and renumbers the guest's exports to match.
func (p *Plan) group() {
	order := make([]int, len(p.Sources))
	for i := range order {
		order[i] = i
	}
	sort.SliceStable(order, func(a, b int) bool { return p.Sources[order[a]].Mount < p.Sources[order[b]].Mount })
	renum := map[string]string{}
	srcs := make([]unfs.Source, len(p.Sources))
	for n, old := range order {
		srcs[n] = p.Sources[old]
		renum[unfs.ExportPath(old)] = unfs.ExportPath(n)
	}
	p.Sources = srcs
	for i := range p.Shares {
		p.Shares[i].Export = renum[p.Shares[i].Export]
	}
}

// LaunchArgs are the vesper-microvm launch arguments for a plan (none when it is empty): each --unfs-src belongs to
// the --unfs-mount before it.
func (p Plan) LaunchArgs() []string {
	if len(p.Sources) == 0 {
		return nil
	}
	args := []string{"--share-net"}
	for m, mount := range p.Mounts {
		args = append(args, "--unfs-mount", mount)
		for _, s := range p.Sources {
			if s.Mount == m {
				args = append(args, "--unfs-src", s.Arg())
			}
		}
	}
	return args
}

// ShareMounts lists the virtio-fs mount points in a /proc/mounts text (octal escapes decoded).
func ShareMounts(procMounts string) []string {
	var out []string
	for _, line := range strings.Split(procMounts, "\n") {
		f := strings.Fields(line)
		if len(f) >= 3 && f[2] == "virtiofs" {
			out = append(out, unescape(f[1]))
		}
	}
	return out
}

func unescape(s string) string {
	var b strings.Builder
	for i := 0; i < len(s); i++ {
		if s[i] == '\\' && i+3 < len(s) && isOct(s[i+1]) && isOct(s[i+2]) && isOct(s[i+3]) {
			b.WriteByte((s[i+1]-'0')<<6 | (s[i+2]-'0')<<3 | (s[i+3] - '0'))
			i += 3
			continue
		}
		b.WriteByte(s[i])
	}
	return b.String()
}

func isOct(c byte) bool { return c >= '0' && c <= '7' }

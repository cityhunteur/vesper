// Package bundle validates OCI bundles handed to the shim and derives the block-first rootfs cache key from
// the snapshot mounts, so every task from the same image chain shares one ext4 build (GH #24).
package bundle

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/containerd/containerd/v2/core/mount"
	specs "github.com/opencontainers/runtime-spec/specs-go"
)

// MetaName is the per-task rootfs record written next to config.json for recovery and audit.
const MetaName = "vesper-rootfs.json"

// Layers describes where the rootfs content comes from.
type Layers struct {
	// Kind is "overlay" (lower chain + upper), "mounts" (other mount types keyed by their sources) or
	// "directory" (no mounts: the bundle rootfs is already populated; no cache, the documented fallback).
	Kind  string   `json:"kind"`
	Lower []string `json:"lower,omitempty"`
	Upper string   `json:"upper,omitempty"`
	// Key is the cache key: sha256 of the ordered lower chain (overlay), of the mount identities (mounts),
	// or empty for the directory fallback.
	Key string `json:"key,omitempty"`
}

// Meta is the record the backend writes as <bundle>/vesper-rootfs.json.
type Meta struct {
	Task      string `json:"task"`
	Bundle    string `json:"bundle"`
	Image     string `json:"image"`
	Cache     string `json:"cache,omitempty"`
	Cached    bool   `json:"cached"`
	Sha256    string `json:"sha256"`
	Bytes     int64  `json:"bytes"`
	BuiltAt   string `json:"built_at"`
	BuildMS   int64  `json:"build_ms"`
	Strategy  string `json:"strategy"`
	Layers    Layers `json:"layers"`
	Fallback  bool   `json:"fallback"`
	Diagnosis string `json:"diagnosis,omitempty"`
}

// ValidationError carries an actionable diagnosis for a malformed bundle.
type ValidationError struct {
	Bundle string
	Reason string
	Hint   string
}

func (e *ValidationError) Error() string {
	return fmt.Sprintf("bundle %s: %s (%s)", e.Bundle, e.Reason, e.Hint)
}

// Validate checks the bundle directory, config.json and the rootfs inputs before any mount happens.
func Validate(bundle string, mounts []mount.Mount) (*specs.Spec, error) {
	fail := func(reason, hint string) (*specs.Spec, error) {
		return nil, &ValidationError{Bundle: bundle, Reason: reason, Hint: hint}
	}
	if !filepath.IsAbs(bundle) {
		return fail("path is not absolute", "containerd passes absolute bundle paths; refuse relative ones")
	}
	st, err := os.Lstat(bundle)
	if err != nil {
		return fail("directory missing: "+err.Error(), "containerd creates the bundle before Create; check the containerd state root")
	}
	if st.Mode()&os.ModeSymlink != 0 {
		return fail("bundle path is a symlink", "a symlinked bundle could redirect rootfs writes; use the real directory")
	}
	if !st.IsDir() {
		return fail("path is not a directory", "the bundle must be the directory holding config.json")
	}
	raw, err := os.ReadFile(filepath.Join(bundle, "config.json"))
	if err != nil {
		return fail("config.json unreadable: "+err.Error(), "the OCI spec was not written; check the client's container spec")
	}
	var spec specs.Spec
	if err := json.Unmarshal(raw, &spec); err != nil {
		return fail("config.json is not valid JSON: "+err.Error(), "regenerate the spec with containerd's oci.GenerateSpec")
	}
	if spec.Version == "" {
		return fail("config.json lacks ociVersion", "the spec must declare an OCI runtime-spec version")
	}
	if spec.Process == nil || len(spec.Process.Args) == 0 {
		return fail("process.args is empty", "set a command: ctr run IMAGE ID CMD… or an image with an entrypoint")
	}
	if spec.Process.Cwd != "" && !filepath.IsAbs(spec.Process.Cwd) {
		return fail("process.cwd is not absolute: "+spec.Process.Cwd, "OCI requires an absolute working directory")
	}
	if spec.Root != nil && spec.Root.Path != "" && spec.Root.Path != "rootfs" && !filepath.IsAbs(spec.Root.Path) {
		return fail("root.path is unexpected: "+spec.Root.Path, "the shim mounts the snapshot at <bundle>/rootfs")
	}
	if len(mounts) == 0 {
		entries, err := os.ReadDir(filepath.Join(bundle, "rootfs"))
		if err != nil || len(entries) == 0 {
			return fail("no rootfs mounts and <bundle>/rootfs is empty", "the snapshotter returned no mounts; check the image was unpacked (ctr images ls shows unpacked)")
		}
	}
	for i, m := range mounts {
		if m.Type == "" || m.Source == "" {
			return fail(fmt.Sprintf("rootfs mount %d lacks type or source", i), "containerd's snapshotter must return complete mounts")
		}
	}
	return &spec, nil
}

// Inspect derives the layer description and cache key from the snapshot mounts.
func Inspect(mounts []mount.Mount) Layers {
	if len(mounts) == 0 {
		return Layers{Kind: "directory"}
	}
	if len(mounts) == 1 && mounts[0].Type == "overlay" {
		l := Layers{Kind: "overlay"}
		for _, opt := range mounts[0].Options {
			switch {
			case strings.HasPrefix(opt, "lowerdir="):
				l.Lower = strings.Split(strings.TrimPrefix(opt, "lowerdir="), ":")
			case strings.HasPrefix(opt, "upperdir="):
				l.Upper = strings.TrimPrefix(opt, "upperdir=")
			}
		}
		if len(l.Lower) > 0 {
			l.Key = digest(l.Lower)
			return l
		}
	}
	ids := make([]string, 0, len(mounts))
	for _, m := range mounts {
		opts := append([]string(nil), m.Options...)
		sort.Strings(opts)
		ids = append(ids, m.Type+"|"+m.Source+"|"+strings.Join(opts, ","))
	}
	return Layers{Kind: "mounts", Lower: ids, Key: digest(ids)}
}

func digest(parts []string) string {
	h := sha256.New()
	for _, p := range parts {
		h.Write([]byte(p))
		h.Write([]byte{0})
	}
	return hex.EncodeToString(h.Sum(nil))
}

// WriteMeta persists the record atomically.
func WriteMeta(bundle string, m Meta) error {
	m.Bundle = bundle
	if m.BuiltAt == "" {
		m.BuiltAt = time.Now().UTC().Format(time.RFC3339)
	}
	raw, err := json.Marshal(m) // one line, so guest tooling can read it with plain shell
	if err != nil {
		return err
	}
	tmp := filepath.Join(bundle, MetaName+".tmp")
	if err := os.WriteFile(tmp, raw, 0o644); err != nil {
		return err
	}
	return os.Rename(tmp, filepath.Join(bundle, MetaName))
}

// ReadMeta loads the record; a missing file yields os.ErrNotExist.
func ReadMeta(bundle string) (Meta, error) {
	var m Meta
	raw, err := os.ReadFile(filepath.Join(bundle, MetaName))
	if err != nil {
		return m, err
	}
	if err := json.Unmarshal(raw, &m); err != nil {
		return m, fmt.Errorf("%s: %w", MetaName, err)
	}
	return m, nil
}

var ErrNoCacheKey = errors.New("no cache key for a directory rootfs")

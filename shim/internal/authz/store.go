package authz

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// The vesper runtime (ADR 0011). dockerd's name for it, as daemon.json registers it.
const VesperRuntime = "vesper"

// StoreRoots are dockerd's active container store, read to learn a container's runtime (ADR 0011 §1). Under
// userns-remap it is /var/lib/docker/<uid>.<gid>/containers, and a legacy store beside it is not dockerd's: a name in
// it could otherwise shadow a prefix dockerd resolves (council COR-5). Tests point this elsewhere.
var StoreRoots = func() []string {
	if roots, _ := filepath.Glob("/var/lib/docker/[0-9]*.[0-9]*/containers"); len(roots) > 0 {
		return roots
	}
	return []string{"/var/lib/docker/containers"}
}

type storedContainer struct {
	id, name, runtime string
	// started: dockerd has started it at least once (State.StartedAt set); a vesper container's drive is built from
	// dockerd's tree at its first start, so before that the tree is the container's own.
	started bool
}

// readStore lists dockerd's containers. An unreadable store is an error: a caller that guards a boundary refuses.
// errNoStore: none of StoreRoots exists, so dockerd has made no container yet.
var errNoStore = errors.New("no dockerd container store")

func readStore() ([]storedContainer, error) {
	var out []storedContainer
	found := false
	for _, root := range StoreRoots() {
		entries, err := os.ReadDir(root)
		if err != nil {
			if os.IsNotExist(err) {
				continue
			}
			return nil, err
		}
		found = true
		for _, e := range entries {
			if !e.IsDir() {
				continue
			}
			dir := filepath.Join(root, e.Name())
			var cfg struct {
				ID, Name string
				State    struct{ StartedAt string }
			}
			raw, err := os.ReadFile(filepath.Join(dir, "config.v2.json"))
			if errors.Is(err, os.ErrNotExist) {
				// dockerd makes the directory before its files, and removes them before the directory: a container
				// being created or removed has no name or runtime to match yet (review of D-090).
				continue
			}
			if err != nil {
				return nil, fmt.Errorf("read %s: %w", dir, err)
			}
			if err := json.Unmarshal(raw, &cfg); err != nil {
				return nil, fmt.Errorf("parse %s/config.v2.json: %w", dir, err)
			}
			// A container whose runtime cannot be read could be a vesper one: refuse rather than guess (council COR-2).
			var hc struct{ Runtime string }
			raw, err = os.ReadFile(filepath.Join(dir, "hostconfig.json"))
			if errors.Is(err, os.ErrNotExist) {
				continue
			}
			if err != nil {
				return nil, fmt.Errorf("read %s/hostconfig.json: %w", dir, err)
			}
			if err := json.Unmarshal(raw, &hc); err != nil {
				return nil, fmt.Errorf("parse %s/hostconfig.json: %w", dir, err)
			}
			started := cfg.State.StartedAt != "" && !strings.HasPrefix(cfg.State.StartedAt, "0001-01-01")
			out = append(out, storedContainer{id: cfg.ID, name: strings.TrimPrefix(cfg.Name, "/"), runtime: hc.Runtime, started: started})
		}
	}
	if !found {
		return nil, errNoStore
	}
	return out, nil
}

// resolveContainer finds the container a reference names, in dockerd's order: a full id, then an exact name, then a
// unique id prefix. ok is false when nothing matches or a prefix is ambiguous.
func resolveContainer(ref string, all []storedContainer) (storedContainer, bool) {
	ref = strings.TrimPrefix(ref, "/")
	for _, c := range all {
		if c.id == ref {
			return c, true
		}
	}
	for _, c := range all {
		if c.name == ref {
			return c, true
		}
	}
	var hit *storedContainer
	for i := range all {
		if ref != "" && strings.HasPrefix(all[i].id, ref) {
			if hit != nil {
				return storedContainer{}, false
			}
			hit = &all[i]
		}
	}
	if hit == nil {
		return storedContainer{}, false
	}
	return *hit, true
}

// joinsVesper is the reason a `container:<x>` mode must be refused, or "" when it may stand: x is a vesper
// container, or cannot be shown not to be one (ADR 0011 §1).
func joinsVesper(option, mode string) string {
	ref, ok := strings.CutPrefix(mode, "container:")
	if !ok {
		return ""
	}
	all, err := readStore()
	if err != nil {
		return fmt.Sprintf("%s=%s is refused: dockerd's container store could not be read (%v)", option, mode, err)
	}
	c, found := resolveContainer(ref, all)
	if !found {
		return fmt.Sprintf("%s=%s is refused: %q names no single container", option, mode, ref)
	}
	if c.runtime == VesperRuntime {
		return fmt.Sprintf("%s=%s is refused: a container cannot join the namespaces of a vesper container", option, mode)
	}
	return ""
}

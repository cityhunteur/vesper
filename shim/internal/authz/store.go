package authz

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// The vesper runtime (ADR 0011). dockerd's name for it, as daemon.json registers it.
const VesperRuntime = "vesper"

// StoreRoots are dockerd's container stores, read to learn a container's runtime (ADR 0011 §1). Under userns-remap
// the store is /var/lib/docker/<uid>.<gid>/containers; tests point this elsewhere.
var StoreRoots = func() []string {
	roots, _ := filepath.Glob("/var/lib/docker/*/containers")
	return append(roots, "/var/lib/docker/containers")
}

type storedContainer struct {
	id, name, runtime string
}

// readStore lists dockerd's containers. An unreadable store is an error: a caller that guards a boundary refuses.
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
			var cfg struct{ ID, Name string }
			raw, err := os.ReadFile(filepath.Join(dir, "config.v2.json"))
			if err != nil {
				return nil, fmt.Errorf("read %s: %w", dir, err)
			}
			if err := json.Unmarshal(raw, &cfg); err != nil {
				return nil, fmt.Errorf("parse %s/config.v2.json: %w", dir, err)
			}
			var hc struct{ Runtime string }
			if raw, err := os.ReadFile(filepath.Join(dir, "hostconfig.json")); err == nil {
				_ = json.Unmarshal(raw, &hc)
			}
			out = append(out, storedContainer{id: cfg.ID, name: strings.TrimPrefix(cfg.Name, "/"), runtime: hc.Runtime})
		}
	}
	if !found {
		return nil, fmt.Errorf("no dockerd container store")
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

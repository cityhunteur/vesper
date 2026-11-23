package authz

import (
	"encoding/json"
	"fmt"
	"os"
)

// DaemonConfig is dockerd's configuration file, which vesper-containers writes before dockerd starts.
var DaemonConfig = "/etc/docker/daemon.json"

// dockerdDefaultRuntime is dockerd's own default when its configuration names none.
const dockerdDefaultRuntime = "runc"

// DefaultRuntime is the runtime dockerd gives a container whose create names none: daemon.json's
// "default-runtime", read at each create, as dockerd reads it at its start.
func DefaultRuntime() (string, error) {
	raw, err := os.ReadFile(DaemonConfig)
	if err != nil {
		return "", err
	}
	var cfg struct {
		DefaultRuntime *string `json:"default-runtime"`
	}
	if err := json.Unmarshal(raw, &cfg); err != nil {
		return "", fmt.Errorf("parse %s: %w", DaemonConfig, err)
	}
	if cfg.DefaultRuntime == nil || *cfg.DefaultRuntime == "" {
		return dockerdDefaultRuntime, nil
	}
	return *cfg.DefaultRuntime, nil
}

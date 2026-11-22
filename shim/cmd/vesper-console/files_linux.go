package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/cityhunteur/vesper/shim/internal/bundle"
)

// dockerFileNames are the only files a Docker task's config may carry (ADR 0011 §4).
var dockerFileNames = map[string]bool{"resolv.conf": true, "hosts": true, "hostname": true}

const maxDockerFile = 64 << 10

// writeDockerFiles writes the files the session's config carries in bundle.FilesAnnotation into dir/files.
func writeDockerFiles(dir string) error {
	raw, err := os.ReadFile(filepath.Join(dir, "config.json"))
	if err != nil {
		return fmt.Errorf("files: read config: %w", err)
	}
	var spec struct{ Annotations map[string]string }
	if err := json.Unmarshal(raw, &spec); err != nil {
		return fmt.Errorf("files: parse config: %w", err)
	}
	if err := setNetSysctls(spec.Annotations[bundle.NetSysctlsAnnotation]); err != nil {
		return err
	}
	enc, ok := spec.Annotations[bundle.FilesAnnotation]
	if !ok {
		return nil
	}
	var files map[string][]byte
	if err := json.Unmarshal([]byte(enc), &files); err != nil {
		return fmt.Errorf("files: parse %s: %w", bundle.FilesAnnotation, err)
	}
	out := filepath.Join(dir, "files")
	if err := os.Mkdir(out, 0o755); err != nil && !os.IsExist(err) {
		return fmt.Errorf("files: %w", err)
	}
	for name, data := range files {
		if !dockerFileNames[name] || len(data) > maxDockerFile {
			return fmt.Errorf("files: %q is not a file a Docker task carries", name)
		}
		if err := os.WriteFile(filepath.Join(out, name), data, 0o644); err != nil {
			return fmt.Errorf("files: write %s: %w", name, err)
		}
	}
	return nil
}

var sysctlName = regexp.MustCompile(`^net\.[a-z0-9_.-]+$`)

// setNetSysctls sets a Docker task's net.* sysctls in the guest's network namespace, which the task shares.
func setNetSysctls(enc string) error {
	if enc == "" {
		return nil
	}
	var kv map[string]string
	if err := json.Unmarshal([]byte(enc), &kv); err != nil {
		return fmt.Errorf("sysctls: parse: %w", err)
	}
	for k, v := range kv {
		if !sysctlName.MatchString(k) || strings.Contains(k, "..") {
			return fmt.Errorf("sysctls: %q is not a net sysctl", k)
		}
		if err := os.WriteFile(filepath.Join("/proc/sys", strings.ReplaceAll(k, ".", "/")), []byte(v), 0o644); err != nil {
			return fmt.Errorf("sysctls: %s: %w", k, err)
		}
	}
	return nil
}

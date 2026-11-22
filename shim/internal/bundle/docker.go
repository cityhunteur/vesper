package bundle

import (
	"encoding/json"
	"fmt"
	"path/filepath"
	"regexp"
	"strings"

	specs "github.com/opencontainers/runtime-spec/specs-go"
)

// FilesAnnotation carries the files dockerd binds into a container (resolv.conf, hosts, hostname) to the guest,
// which writes them under the session's files/ directory before runc starts (ADR 0011 §4).
const FilesAnnotation = "io.vesper.docker-files"

// GuestInit is the guest's tini, which stands in for dockerd's docker-init bind (--init).
const GuestInit = "/sbin/tini-static"

// NetSysctlsAnnotation carries a Docker task's net.* sysctls, which the guest sets itself: the task shares the
// guest's own network namespace, where runc refuses them (ADR 0011 §4).
const NetSysctlsAnnotation = "io.vesper.net-sysctls"

// The bind sources dockerd uses: the container's own record and its volumes, with or without userns-remap.
var (
	dockerFileRe   = regexp.MustCompile(`^/var/lib/docker/(?:[0-9]+\.[0-9]+/)?containers/([0-9a-f]{64})/(resolv\.conf|hosts|hostname)$`)
	dockerVolumeRe = regexp.MustCompile(`^/var/lib/docker/(?:[0-9]+\.[0-9]+/)?volumes/[^/]+/_data$`)
	dockerRootRe   = regexp.MustCompile(`^/var/lib/docker/(?:[0-9]+\.[0-9]+/)?(?:overlay2|vfs|fuse-overlayfs)/[^/]+/merged$`)
)

// DockerRoot is the rootfs dockerd mounted itself, when the spec is dockerd's (ADR 0011 gap 3).
func DockerRoot(spec *specs.Spec) (string, bool) {
	if spec == nil || spec.Root == nil || !dockerRootRe.MatchString(spec.Root.Path) {
		return "", false
	}
	return spec.Root.Path, true
}

// DockerFile is one file the guest must write: its destination in the container and where dockerd keeps it.
type DockerFile struct {
	Dest   string
	Source string
}

// CheckDocker applies ADR 0011 §4 to dockerd's spec for task id and returns the files to read for the guest.
func CheckDocker(spec *specs.Spec, id string) ([]DockerFile, error) {
	if spec.Root != nil && spec.Root.Readonly {
		return nil, fmt.Errorf("a read-only root filesystem is refused for the vesper runtime (ADR 0011)")
	}
	if spec.Hooks != nil && (len(spec.Hooks.Prestart) > 0 || len(spec.Hooks.CreateRuntime) > 0 || len(spec.Hooks.CreateContainer) > 0 ||
		len(spec.Hooks.StartContainer) > 0 || len(spec.Hooks.Poststart) > 0 || len(spec.Hooks.Poststop) > 0) {
		return nil, fmt.Errorf("OCI hooks are refused for the vesper runtime (ADR 0011)")
	}
	if spec.Linux != nil {
		if len(spec.Linux.Devices) > 0 {
			return nil, fmt.Errorf("devices are refused for the vesper runtime (ADR 0011)")
		}
		for _, ns := range spec.Linux.Namespaces {
			if ns.Path != "" {
				return nil, fmt.Errorf("a %s namespace path (%s) is refused for the vesper runtime (ADR 0011)", ns.Type, ns.Path)
			}
		}
	}
	var files []DockerFile
	for _, m := range spec.Mounts {
		if m.Type != "bind" && !hasOpt(m.Options, "bind") && !hasOpt(m.Options, "rbind") {
			continue
		}
		switch {
		case dockerFileRe.MatchString(m.Source):
			if got := dockerFileRe.FindStringSubmatch(m.Source)[1]; got != id {
				return nil, fmt.Errorf("bind %s is another container's file (ADR 0011)", m.Source)
			}
			files = append(files, DockerFile{Dest: m.Destination, Source: m.Source})
		case dockerVolumeRe.MatchString(m.Source):
			// An anonymous volume of the image: its directory in the container's drive stands in (ADR 0011 §4).
		case m.Destination == "/sbin/docker-init":
		default:
			return nil, fmt.Errorf("bind %s:%s is refused for the vesper runtime until shares reach its microVMs (ADR 0011 slice 4)", m.Source, m.Destination)
		}
	}
	return files, nil
}

func hasOpt(opts []string, want string) bool {
	for _, o := range opts {
		if o == want {
			return true
		}
	}
	return false
}

// GuestDockerConfig is the guest's runc spec for a Docker task: dockerd's process, hostname, kernel mounts,
// namespaces (without the user namespace; the network is the guest's own), masked and read-only paths and sysctls;
// its three files bound from sessionDir/files; resources as the guest applies them (ADR 0011 §4).
func GuestDockerConfig(spec *specs.Spec, files map[string][]byte, resources *specs.LinuxResources, sessionDir string) ([]byte, error) {
	process, err := GuestProcess(spec.Process)
	if err != nil {
		return nil, err
	}
	out := &specs.Spec{Version: specs.Version, Process: process, Root: &specs.Root{Path: "/newroot"}, Hostname: spec.Hostname}
	for _, m := range spec.Mounts {
		switch {
		case m.Type == "bind" || hasOpt(m.Options, "bind") || hasOpt(m.Options, "rbind"):
			base := filepath.Base(m.Source)
			if dockerFileRe.MatchString(m.Source) {
				opts := []string{"rbind", "rprivate"}
				if hasOpt(m.Options, "ro") {
					opts = append(opts, "ro")
				}
				out.Mounts = append(out.Mounts, specs.Mount{Destination: m.Destination, Type: "bind", Source: filepath.Join(sessionDir, "files", base), Options: opts})
			} else if m.Destination == "/sbin/docker-init" {
				out.Mounts = append(out.Mounts, specs.Mount{Destination: m.Destination, Type: "bind", Source: GuestInit, Options: []string{"rbind", "ro"}})
			}
		case m.Type == "sysfs":
			// sysfs belongs to a network namespace, and the task shares the guest's, where a second, read-only
			// sysfs is refused (EBUSY): the guest's /sys is bound read-only instead, as for a host-network container.
			out.Mounts = append(out.Mounts, specs.Mount{Destination: m.Destination, Type: "bind", Source: "/sys", Options: []string{"rbind", "ro", "nosuid", "nodev", "noexec"}})
		default: // proc, cgroup, devpts, mqueue, tmpfs (/dev, /dev/shm, --tmpfs): made by the guest's runc
			out.Mounts = append(out.Mounts, m)
		}
	}
	linux := &specs.Linux{Resources: resources}
	var netSysctls map[string]string
	if spec.Linux != nil {
		linux.MaskedPaths, linux.ReadonlyPaths = spec.Linux.MaskedPaths, spec.Linux.ReadonlyPaths
		for k, v := range spec.Linux.Sysctl {
			if strings.HasPrefix(k, "net.") {
				if netSysctls == nil {
					netSysctls = map[string]string{}
				}
				netSysctls[k] = v
				continue
			}
			if linux.Sysctl == nil {
				linux.Sysctl = map[string]string{}
			}
			linux.Sysctl[k] = v
		}
		for _, ns := range spec.Linux.Namespaces {
			switch ns.Type {
			case specs.UserNamespace:
			case specs.NetworkNamespace:
				linux.Namespaces = append(linux.Namespaces, specs.LinuxNamespace{Type: specs.NetworkNamespace, Path: "/proc/1/ns/net"})
			default:
				linux.Namespaces = append(linux.Namespaces, specs.LinuxNamespace{Type: ns.Type})
			}
		}
	}
	out.Linux = linux
	if len(files) > 0 || len(netSysctls) > 0 {
		out.Annotations = map[string]string{}
	}
	if len(files) > 0 {
		enc, err := json.Marshal(files)
		if err != nil {
			return nil, err
		}
		out.Annotations[FilesAnnotation] = string(enc)
	}
	if len(netSysctls) > 0 {
		enc, err := json.Marshal(netSysctls)
		if err != nil {
			return nil, err
		}
		out.Annotations[NetSysctlsAnnotation] = string(enc)
	}
	if strings.ContainsRune(out.Hostname, 0) {
		return nil, fmt.Errorf("hostname contains NUL")
	}
	return json.Marshal(out)
}

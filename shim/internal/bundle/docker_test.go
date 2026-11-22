package bundle

import (
	"encoding/json"
	"strings"
	"testing"

	specs "github.com/opencontainers/runtime-spec/specs-go"
)

const cid = "1915acf2fb8f4b5c4a82357ce17b202ef7c58af7b19db4dee80c61a7b679a710"

// dockerSpec is shaped like the spec dockerd gave containerd in the B1 spike (run/B1/spec.txt).
func dockerSpec() *specs.Spec {
	rec := "/var/lib/docker/100000.100000/containers/" + cid + "/"
	return &specs.Spec{
		Version:  "1.2.1",
		Hostname: "1915acf2fb8f",
		Root:     &specs.Root{Path: "/var/lib/docker/100000.100000/overlay2/9bf742b076ce/merged"},
		Process:  &specs.Process{Args: []string{"sleep", "3600"}, Cwd: "/", User: specs.User{UID: 0, GID: 0, AdditionalGids: []uint32{0, 1, 2}}, OOMScoreAdj: new(int)},
		Mounts: []specs.Mount{
			{Destination: "/proc", Type: "proc", Source: "proc"},
			{Destination: "/dev", Type: "tmpfs", Source: "tmpfs", Options: []string{"nosuid", "mode=755", "size=65536k"}},
			{Destination: "/dev/shm", Type: "tmpfs", Source: "shm", Options: []string{"size=67108864"}},
			{Destination: "/sys", Type: "sysfs", Source: "sysfs", Options: []string{"ro"}},
			{Destination: "/etc/resolv.conf", Type: "bind", Source: rec + "resolv.conf", Options: []string{"rbind", "rprivate"}},
			{Destination: "/etc/hostname", Type: "bind", Source: rec + "hostname", Options: []string{"rbind", "rprivate"}},
			{Destination: "/etc/hosts", Type: "bind", Source: rec + "hosts", Options: []string{"rbind", "rprivate"}},
		},
		Linux: &specs.Linux{
			Namespaces:    []specs.LinuxNamespace{{Type: "mount"}, {Type: "network"}, {Type: "uts"}, {Type: "pid"}, {Type: "ipc"}, {Type: "user"}, {Type: "cgroup"}},
			UIDMappings:   []specs.LinuxIDMapping{{ContainerID: 0, HostID: 100000, Size: 65536}},
			Sysctl:        map[string]string{"net.ipv4.ip_unprivileged_port_start": "0"},
			MaskedPaths:   []string{"/proc/kcore"},
			ReadonlyPaths: []string{"/proc/sys"},
			Resources:     &specs.LinuxResources{Devices: []specs.LinuxDeviceCgroup{{Allow: false, Access: "rwm"}}},
		},
	}
}

func TestDockerRootAndFiles(t *testing.T) {
	s := dockerSpec()
	if root, ok := DockerRoot(s); !ok || !strings.HasSuffix(root, "/merged") {
		t.Fatalf("DockerRoot = %q %v", root, ok)
	}
	if _, ok := DockerRoot(&specs.Spec{Root: &specs.Root{Path: "rootfs"}}); ok {
		t.Error("a ctr bundle was taken for dockerd's")
	}
	files, err := CheckDocker(s, cid)
	if err != nil {
		t.Fatal(err)
	}
	if len(files) != 3 {
		t.Fatalf("files = %v", files)
	}
}

func TestCheckDockerRefusals(t *testing.T) {
	cases := map[string]func(*specs.Spec){
		"read-only root":   func(s *specs.Spec) { s.Root.Readonly = true },
		"a hook":           func(s *specs.Spec) { s.Hooks = &specs.Hooks{Prestart: []specs.Hook{{Path: "/x"}}} },
		"a device":         func(s *specs.Spec) { s.Linux.Devices = []specs.LinuxDevice{{Path: "/dev/kvm"}} },
		"a namespace path": func(s *specs.Spec) { s.Linux.Namespaces[1].Path = "/proc/1/ns/net" },
		"a host bind": func(s *specs.Spec) {
			s.Mounts = append(s.Mounts, specs.Mount{Destination: "/x", Type: "bind", Source: "/etc"})
		},
		"another container's file": func(s *specs.Spec) {
			s.Mounts[4].Source = strings.Replace(s.Mounts[4].Source, cid, strings.Repeat("a", 64), 1)
		},
	}
	for name, change := range cases {
		s := dockerSpec()
		change(s)
		if _, err := CheckDocker(s, cid); err == nil {
			t.Errorf("%s allowed", name)
		} else if !strings.Contains(err.Error(), "ADR 0011") {
			t.Errorf("%s: %v does not name ADR 0011", name, err)
		}
	}
	s := dockerSpec()
	s.Mounts = append(s.Mounts,
		specs.Mount{Destination: "/data", Type: "bind", Source: "/var/lib/docker/100000.100000/volumes/abc/_data"},
		specs.Mount{Destination: "/sbin/docker-init", Type: "bind", Source: "/usr/bin/docker-init"})
	if _, err := CheckDocker(s, cid); err != nil {
		t.Errorf("an anonymous volume and --init refused: %v", err)
	}
}

func TestGuestDockerConfig(t *testing.T) {
	s := dockerSpec()
	s.Mounts = append(s.Mounts, specs.Mount{Destination: "/sbin/docker-init", Type: "bind", Source: "/usr/bin/docker-init"},
		specs.Mount{Destination: "/data", Type: "bind", Source: "/var/lib/docker/100000.100000/volumes/abc/_data"})
	raw, err := GuestDockerConfig(s, map[string][]byte{"hosts": []byte("127.0.0.1 localhost\n")}, nil, "/run/sessions/p1028")
	if err != nil {
		t.Fatal(err)
	}
	var g specs.Spec
	if err := json.Unmarshal(raw, &g); err != nil {
		t.Fatal(err)
	}
	if g.Root.Path != "/newroot" || g.Hostname != "1915acf2fb8f" {
		t.Errorf("root %q hostname %q", g.Root.Path, g.Hostname)
	}
	for _, ns := range g.Linux.Namespaces {
		if ns.Type == specs.UserNamespace {
			t.Error("the guest spec keeps a user namespace")
		}
		if ns.Type == specs.NetworkNamespace && ns.Path != "/proc/1/ns/net" {
			t.Errorf("network namespace %q", ns.Path)
		}
	}
	if len(g.Linux.UIDMappings) != 0 || g.Linux.Sysctl["net.ipv4.ip_unprivileged_port_start"] != "" || len(g.Linux.MaskedPaths) != 1 ||
		!strings.Contains(g.Annotations[NetSysctlsAnnotation], "ip_unprivileged_port_start") {
		t.Errorf("linux = %+v", g.Linux)
	}
	binds := map[string]string{}
	for _, m := range g.Mounts {
		if m.Type == "bind" {
			binds[m.Destination] = m.Source
		}
	}
	want := map[string]string{"/etc/resolv.conf": "/run/sessions/p1028/files/resolv.conf", "/etc/hostname": "/run/sessions/p1028/files/hostname",
		"/etc/hosts": "/run/sessions/p1028/files/hosts", "/sbin/docker-init": GuestInit}
	for d, src := range want {
		if binds[d] != src {
			t.Errorf("bind %s = %q, want %q", d, binds[d], src)
		}
	}
	if binds["/sys"] != "/sys" {
		t.Errorf("/sys = %q, want a bind of the guest's /sys", binds["/sys"])
	}
	if _, ok := binds["/data"]; ok {
		t.Error("a volume bind reached the guest")
	}
	if !strings.Contains(g.Annotations[FilesAnnotation], `"hosts"`) {
		t.Errorf("annotation = %q", g.Annotations[FilesAnnotation])
	}
}

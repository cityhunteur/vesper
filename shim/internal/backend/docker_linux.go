//go:build linux

package backend

import (
	"bufio"
	"context"
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"

	specs "github.com/opencontainers/runtime-spec/specs-go"
	"golang.org/x/sys/unix"

	"github.com/cityhunteur/vesper/shim/internal/bundle"
)

const (
	// StateDir is the System VM's data disk; a Docker task's drive and jail live on it (ADR 0011 §5).
	StateDir = "/var/lib/vesper/state"
	// DriveDir holds one drive per Docker container, kept across its starts and swept when the container is gone.
	DriveDir = StateDir + "/microvm/drives"
	// DriveAllowanceKiB is the writable room each drive gets beyond its image (sparse; a recorded residual).
	DriveAllowanceKiB = 4 << 20
	maxDockerFile     = 64 << 10
)

// DataDiskInUse reports whether StateDir is the data disk's ext4 mount, as vesper-vesperd's data_ok checks.
func DataDiskInUse() bool {
	f, err := os.Open("/proc/mounts")
	if err != nil {
		return false
	}
	defer f.Close()
	sc := bufio.NewScanner(f)
	for sc.Scan() {
		fields := strings.Fields(sc.Text())
		if len(fields) >= 3 && fields[1] == StateDir && fields[2] == "ext4" {
			return true
		}
	}
	return false
}

// DrivePath is container id's drive.
func DrivePath(id string) string { return filepath.Join(DriveDir, id+".ext4") }

// ReadDockerFiles reads the files dockerd binds into a Docker task, each by its exact path, refusing a symbolic
// link anywhere on the way and a file over 64 KiB (ADR 0011 §4). Keys are base names.
func ReadDockerFiles(files []bundle.DockerFile) (map[string][]byte, error) {
	out := map[string][]byte{}
	for _, f := range files {
		fd, err := unix.Openat2(unix.AT_FDCWD, f.Source, &unix.OpenHow{
			Flags: unix.O_RDONLY | unix.O_CLOEXEC | unix.O_NOFOLLOW, Resolve: unix.RESOLVE_NO_SYMLINKS | unix.RESOLVE_NO_MAGICLINKS,
		})
		if err != nil {
			return nil, fmt.Errorf("read %s: %w", f.Source, err)
		}
		file := os.NewFile(uintptr(fd), f.Source)
		var st unix.Stat_t
		if err := unix.Fstat(fd, &st); err != nil || st.Mode&unix.S_IFMT != unix.S_IFREG {
			file.Close()
			return nil, fmt.Errorf("%s is not a regular file", f.Source)
		}
		data, err := io.ReadAll(io.LimitReader(file, maxDockerFile+1))
		file.Close()
		if err != nil {
			return nil, fmt.Errorf("read %s: %w", f.Source, err)
		}
		if len(data) > maxDockerFile {
			return nil, fmt.Errorf("%s is over 64 KiB", f.Source)
		}
		out[filepath.Base(f.Source)] = data
	}
	return out, nil
}

// PrepareDocker returns container id's drive, building it from dockerd's rootfs on its first start. mke2fs runs in
// a user namespace with the spec's own mapping, so the guest sees the image's owners as the container sees them;
// the shim opens the rootfs and the output file and passes them as descriptors, so the namespace's process never
// walks a root-only directory and DriveDir stays root-only (ADR 0011 §5). built is false for a reused drive.
func (m *MicroVM) PrepareDocker(ctx context.Context, id, root string, uids, gids []specs.LinuxIDMapping) (image string, built bool, err error) {
	image = DrivePath(id)
	if _, err := os.Stat(image); err == nil {
		return image, false, nil
	}
	if !DataDiskInUse() {
		return "", false, diagnose("place the drive", fmt.Errorf("the vesper runtime needs the System VM's data disk"), "start the System VM with its data disk (docs/process/systemvm-bringup.md)")
	}
	// microvm/ and drives/ can be passed through but not listed; drives are root-only files. The build, which runs
	// as the mapping's root, writes under a random name in drives/.build, which only it owns (ADR 0011 §5).
	if err := os.MkdirAll(DriveDir, 0o711); err != nil {
		return "", false, diagnose("create the drive directory", err, "")
	}
	_ = os.Chmod(filepath.Dir(DriveDir), 0o711)
	_ = os.Chmod(DriveDir, 0o711)
	used, err := diskUsageKiB(root)
	if err != nil {
		return "", false, diagnose("size the rootfs", err, "")
	}
	size := used + used/5 + 4096 + DriveAllowanceKiB
	if err := admitDrive(size); err != nil {
		return "", false, err
	}
	host := func(maps []specs.LinuxIDMapping) (uint32, uint32, bool) {
		for _, mp := range maps {
			if mp.ContainerID == 0 {
				return mp.HostID, mp.Size, true
			}
		}
		return 0, 0, false
	}
	hu, su, okU := host(uids)
	hg, sg, okG := host(gids)
	build := filepath.Join(DriveDir, ".build")
	if err := os.Mkdir(build, 0o700); err != nil && !os.IsExist(err) {
		return "", false, diagnose("create the build directory", err, "")
	}
	var rnd [16]byte
	if _, err := rand.Read(rnd[:]); err != nil {
		return "", false, diagnose("name the build", err, "")
	}
	tmp := filepath.Join(build, hex.EncodeToString(rnd[:]))
	out, err := os.OpenFile(tmp, os.O_CREATE|os.O_EXCL|os.O_RDWR|unix.O_CLOEXEC, 0o600)
	if err != nil {
		return "", false, diagnose("create the drive", err, "")
	}
	defer func() {
		out.Close()
		if err != nil {
			_ = os.Remove(tmp)
		}
	}()
	cmd := exec.CommandContext(ctx, "mke2fs", "-q", "-F", "-t", "ext4", "-d", root, "-L", "vesper-task",
		"-E", "root_owner=0:0", tmp, strconv.FormatInt(size, 10)+"K")
	if okU && okG {
		// The namespace's root is the mapping's host id: it owns the build directory and the output file, and
		// reaches dockerd's rootfs as dockerd's remapped root does (group traversal of /var/lib/docker).
		if err := os.Chown(build, int(hu), int(hg)); err != nil {
			return "", false, diagnose("give the build directory to the build", err, "")
		}
		if err := out.Chown(int(hu), int(hg)); err != nil {
			return "", false, diagnose("give the drive to the build", err, "")
		}
		cmd.SysProcAttr = &syscall.SysProcAttr{
			Cloneflags:  syscall.CLONE_NEWUSER,
			UidMappings: []syscall.SysProcIDMap{{ContainerID: 0, HostID: int(hu), Size: int(su)}},
			GidMappings: []syscall.SysProcIDMap{{ContainerID: 0, HostID: int(hg), Size: int(sg)}},
			// Become the namespace's root before exec: the creator keeps its host uid, which is unmapped there, and
			// an unmapped uid loses every capability at exec.
			Credential: &syscall.Credential{Uid: 0, Gid: 0, NoSetGroups: true},
		}
	}
	if b, e := cmd.CombinedOutput(); e != nil {
		err = diagnose("build the drive", fmt.Errorf("mke2fs: %w: %s", e, strings.TrimSpace(string(b))), "an owner outside the container's mapping cannot be shifted")
		return "", false, err
	}
	if err = out.Chown(0, 0); err != nil {
		return "", false, diagnose("give the drive back to root", err, "")
	}
	if err = os.Rename(tmp, image); err != nil {
		return "", false, diagnose("publish the drive", err, "")
	}
	return image, true, nil
}

// admitDrive refuses a drive that would leave less than the reserve free on the data disk: the System VM's memory
// size, as vesperd's store does (ADR 0011 §5).
func admitDrive(sizeKiB int64) error {
	var st unix.Statfs_t
	if err := unix.Statfs(StateDir, &st); err != nil {
		return diagnose("check the data disk's free space", err, "")
	}
	free := int64(st.Bavail) * int64(st.Bsize)
	reserve := memTotalBytes()
	if free-sizeKiB*1024 < reserve {
		return diagnose("admit the drive", fmt.Errorf("the data disk has %d MiB free and the drive needs %d MiB with a reserve of %d MiB", free>>20, sizeKiB>>10, reserve>>20),
			"remove unused containers (docker container prune) or enlarge the data disk")
	}
	return nil
}

func memTotalBytes() int64 {
	raw, err := os.ReadFile("/proc/meminfo")
	if err != nil {
		return 0
	}
	for _, line := range strings.Split(string(raw), "\n") {
		if f := strings.Fields(line); len(f) >= 2 && f[0] == "MemTotal:" {
			kib, _ := strconv.ParseInt(f[1], 10, 64)
			return kib * 1024
		}
	}
	return 0
}

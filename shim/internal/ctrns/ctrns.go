//go:build linux

// Package ctrns gives a Docker task its own network namespace before dockerd wires it (ADR 0011 §2).
//
// At Create the shim starts a holder: the System VM's sleep in new network, pid, ipc, uts and cgroup namespaces,
// which only waits, with PR_SET_PDEATHSIG. The shim binds the holder's network namespace at Dir/<id> and
// asks vesper-microvm to make it default-deny; Create then reports the holder's pid, and dockerd moves the
// container's veth into that namespace. At Start, vesper-microvm launches Firecracker in it. At Delete (or after a
// shim crash, from containerd's shim delete) vesper-microvm ctrns-down stops the holder and unbinds the namespace.
package ctrns

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"time"

	"golang.org/x/sys/unix"
)

// Dir holds the bound namespaces; ADR 0009's share namespaces are in /run/vesper/microvm/netns, apart.
const Dir = "/run/vesper/microvm/ctrns"

// HoldArg is the shim's hidden mode for the holder. New holders are holdProgram; the mode stays so that a shim
// started before an upgrade, which re-executes the binary at its old path, still gets a holder.
const HoldArg = "__vesper_ctrns_hold"

// holdProgram and holdArgs make the holder: the System VM's busybox sleep, for about 68 years. A re-exec of the shim
// held about 4.5 MiB of private memory for each Docker task, against about 0.1 MiB for sleep (TD-100 G2).
const holdProgram = "/bin/sleep"

var holdArgs = []string{"2147483647"}

const microvmBin = "/usr/local/bin/vesper-microvm"

// Hold is the holder's body: it waits until it is killed. The shim's main calls it when started with HoldArg.
func Hold() {
	// The parent's death is delivered as SIGKILL (Pdeathsig below). A sleep, not `select {}`: with no other
	// goroutine the runtime takes an empty select for a deadlock and exits.
	for {
		time.Sleep(time.Hour)
	}
}

// Path is where task id's namespace is bound.
func Path(id string) string { return filepath.Join(Dir, id) }

// Start makes the namespace for task id and returns the holder's pid. onExit runs when the holder exits, for
// whatever reason; the task's pid is the holder's, so a task whose holder is gone must stop (ADR 0011 §2). On error
// nothing is left behind.
func Start(ctx context.Context, id string, onExit func()) (int, error) {
	if err := os.MkdirAll(Dir, 0o700); err != nil {
		return 0, fmt.Errorf("namespace directory: %w", err)
	}
	started := make(chan error, 1)
	pidC := make(chan int, 1)
	// The kernel sends Pdeathsig when the thread that forked the child exits, not the process. So the fork happens
	// on a thread locked for the holder's life: the goroutine blocks in Wait and never unlocks it (ADR 0011 §2).
	go func() {
		runtime.LockOSThread()
		cmd := exec.Command(holdProgram, holdArgs...)
		cmd.SysProcAttr = &syscall.SysProcAttr{
			// No mount namespace: the holder needs none, and a copy of every mount would pin other tasks' namespace
			// binds while it lives (council COR-6).
			Cloneflags: unix.CLONE_NEWNET | unix.CLONE_NEWPID | unix.CLONE_NEWIPC | unix.CLONE_NEWUTS | unix.CLONE_NEWCGROUP,
			Pdeathsig:  syscall.SIGKILL,
		}
		if err := cmd.Start(); err != nil {
			started <- err
			return
		}
		pidC <- cmd.Process.Pid
		started <- nil
		_ = cmd.Wait()
		if onExit != nil {
			onExit()
		}
	}()
	if err := <-started; err != nil {
		return 0, fmt.Errorf("start the namespace holder: %w", err)
	}
	pid := <-pidC
	undo := func() { _ = syscall.Kill(pid, syscall.SIGKILL) }
	target := Path(id)
	f, err := os.OpenFile(target, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if err != nil {
		undo()
		return 0, fmt.Errorf("namespace file %s: %w", target, err)
	}
	f.Close()
	if err := unix.Mount(fmt.Sprintf("/proc/%d/ns/net", pid), target, "", unix.MS_BIND, ""); err != nil {
		undo()
		_ = os.Remove(target)
		return 0, fmt.Errorf("bind the namespace: %w", err)
	}
	if out, err := exec.CommandContext(ctx, microvmBin, "ctrns-up", id, strconv.Itoa(pid)).CombinedOutput(); err != nil {
		_ = Release(context.Background(), id)
		undo()
		return 0, fmt.Errorf("vesper-microvm ctrns-up %s: %v: %s", id, err, strings.TrimSpace(string(out)))
	}
	return pid, nil
}

// Release stops the holder and unbinds the namespace; safe to call again and after a shim crash.
func Release(ctx context.Context, id string) error {
	out, err := exec.CommandContext(ctx, microvmBin, "ctrns-down", id).CombinedOutput()
	if err != nil {
		return fmt.Errorf("vesper-microvm ctrns-down %s: %v: %s", id, err, strings.TrimSpace(string(out)))
	}
	return nil
}

// Exists reports whether task id has a bound namespace.
func Exists(id string) bool {
	_, err := os.Stat(Path(id))
	return err == nil
}

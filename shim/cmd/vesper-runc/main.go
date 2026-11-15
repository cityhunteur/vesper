//go:build linux

// vesper-runc is the runtime dockerd uses in the System VM (TD-062, ADR 0010): runc, plus one step after create.
//
// Every runc command but create is runc itself: vesper-runc replaces itself with /usr/bin/runc. For create it runs
// runc, and when the container is created it asks each bind source on a macOS share for its attributes again as the
// container's uid, before the process starts. Then the System VM's attribute cache holds the owner that uid sees,
// not uid 0 from dockerd's and runc's own lookups. The step never fails a create; its last result is in
// /run/vesper/runc-refresh.
//
// usage: as runc
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"strings"
	"syscall"
	"time"

	specs "github.com/opencontainers/runtime-spec/specs-go"

	"github.com/cityhunteur/vesper/shim/internal/runcwrap"
)

const (
	realRunc = "/usr/bin/runc"
	status   = "/run/vesper/runc-refresh"
	// The share answers in well under a millisecond; this bounds a share that has stopped answering.
	timeout = 2 * time.Second
)

func main() {
	args := os.Args[1:]
	call := runcwrap.Parse(args)
	if call.Command != "create" {
		err := syscall.Exec(realRunc, append([]string{realRunc}, args...), os.Environ())
		fmt.Fprintln(os.Stderr, "vesper-runc:", err)
		os.Exit(127)
	}
	code := create(args, call.PreserveFDs)
	if code == 0 {
		refresh(call.Bundle)
	}
	os.Exit(code)
}

// create runs runc as a child with the same descriptors and forwards the signals a caller may send.
func create(args []string, preserve int) int {
	cmd := exec.Command(realRunc, args...)
	cmd.Stdin, cmd.Stdout, cmd.Stderr = os.Stdin, os.Stdout, os.Stderr
	for i := 0; i < preserve; i++ {
		cmd.ExtraFiles = append(cmd.ExtraFiles, os.NewFile(uintptr(3+i), fmt.Sprintf("fd%d", 3+i)))
	}
	cmd.SysProcAttr = &syscall.SysProcAttr{Pdeathsig: syscall.SIGKILL}
	sigs := make(chan os.Signal, 8)
	signal.Notify(sigs, syscall.SIGTERM, syscall.SIGINT, syscall.SIGHUP, syscall.SIGQUIT)
	if err := cmd.Start(); err != nil {
		fmt.Fprintln(os.Stderr, "vesper-runc:", err)
		return 127
	}
	go func() {
		for s := range sigs {
			_ = cmd.Process.Signal(s)
		}
	}()
	err := cmd.Wait()
	signal.Stop(sigs)
	if err == nil {
		return 0
	}
	if ee, ok := err.(*exec.ExitError); ok {
		if ws, ok := ee.Sys().(syscall.WaitStatus); ok && ws.Signaled() {
			return 128 + int(ws.Signal())
		}
		return ee.ExitCode()
	}
	fmt.Fprintln(os.Stderr, "vesper-runc:", err)
	return 1
}

func refresh(bundle string) {
	if bundle == "" {
		bundle = "."
	}
	line := func() string {
		raw, err := os.ReadFile(filepath.Join(bundle, "config.json"))
		if err != nil {
			return "REFRESH error=no-config"
		}
		var s specs.Spec
		if err := json.Unmarshal(raw, &s); err != nil {
			return "REFRESH error=bad-config"
		}
		t, ok := runcwrap.TargetOf(&s)
		if !ok {
			return "REFRESH none"
		}
		r := runcwrap.Refresh(t, timeout)
		out := fmt.Sprintf("REFRESH uid=%d gid=%d refreshed=%d skipped=%d failed=%d", t.UID, t.GID, r.Refreshed, r.Skipped, len(r.Errors))
		if len(r.Errors) > 0 {
			out += " first=" + strings.ReplaceAll(r.Errors[0].Error(), " ", "_")
		}
		return out
	}()
	// One line, replaced by every create; it tells an operator why a container saw "nobody", and never grows.
	tmp := fmt.Sprintf("%s.%d", status, os.Getpid())
	if os.WriteFile(tmp, []byte(line+"\n"), 0o600) == nil {
		_ = os.Rename(tmp, status)
	}
}

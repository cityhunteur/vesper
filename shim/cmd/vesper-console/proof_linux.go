package main

import (
	"fmt"
	"io"
	"os"
	"os/exec"
	"strconv"
	"syscall"
	"time"

	"github.com/containerd/console"
)

// proofPTY gives ctr a real terminal in the hardware suite, then sends a window change while it runs.
func proofPTY(args []string) (int, error) {
	if len(args) < 3 {
		return 0, fmt.Errorf("proof-pty ROWS COLS COMMAND...")
	}
	rows, err := strconv.ParseUint(args[0], 10, 16)
	if err != nil || rows == 0 {
		return 0, fmt.Errorf("invalid rows")
	}
	cols, err := strconv.ParseUint(args[1], 10, 16)
	if err != nil || cols == 0 {
		return 0, fmt.Errorf("invalid columns")
	}
	master, path, err := console.NewPty()
	if err != nil {
		return 0, err
	}
	defer master.Close()
	if err := master.Resize(console.WinSize{Height: 24, Width: 80}); err != nil {
		return 0, err
	}
	slave, err := os.OpenFile(path, os.O_RDWR, 0)
	if err != nil {
		return 0, err
	}
	defer slave.Close()
	cmd := exec.Command(args[2], args[3:]...)
	cmd.Stdin, cmd.Stdout, cmd.Stderr = slave, slave, slave
	cmd.SysProcAttr = &syscall.SysProcAttr{Setsid: true, Setctty: true, Ctty: 0}
	if err := cmd.Start(); err != nil {
		return 0, err
	}
	slave.Close()
	done := make(chan struct{})
	defer close(done)
	go func() {
		select {
		case <-done:
			return
		case <-time.After(3 * time.Second):
			_ = master.Resize(console.WinSize{Height: uint16(rows), Width: uint16(cols)})
			_ = cmd.Process.Signal(syscall.SIGWINCH)
		}
	}()
	go func() { _, _ = io.Copy(master, os.Stdin) }()
	output := make(chan struct{})
	go func() { _, _ = io.Copy(os.Stdout, master); close(output) }()
	err = cmd.Wait()
	<-output
	if err != nil {
		if e, ok := err.(*exec.ExitError); ok {
			return e.ExitCode(), nil
		}
		return 0, err
	}
	return 0, nil
}

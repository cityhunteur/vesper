// vesper-console receives runc's terminal master; it has no vsock listener and never executes workload argv.
package main

import (
	"context"
	"fmt"
	"io"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"time"

	"github.com/containerd/console"
	runc "github.com/containerd/go-runc"
	"golang.org/x/sys/unix"
)

func main() {
	if len(os.Args) > 1 && os.Args[1] == "proof-pty" {
		code, err := proofPTY(os.Args[2:])
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(255)
		}
		os.Exit(code)
	}
	if len(os.Args) == 5 && os.Args[1] == "resize" {
		if err := resize(os.Args[2], os.Args[3], os.Args[4]); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		return
	}
	if len(os.Args) != 3 || (os.Args[1] != "RUN" && os.Args[1] != "EXEC") {
		fmt.Fprintln(os.Stderr, "usage: vesper-console RUN|EXEC SESSION-DIR | resize SESSION-DIR ROWS COLS")
		os.Exit(1)
	}
	code, err := run(os.Args[1], os.Args[2])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(255)
	}
	os.Exit(code)
}

func resize(dir, rows, cols string) error {
	c, err := net.DialTimeout("unix", filepath.Join(dir, "resize.sock"), time.Second)
	if err != nil {
		return fmt.Errorf("connect resize: %w", err)
	}
	defer c.Close()
	_ = c.SetDeadline(time.Now().Add(time.Second))
	if _, err = fmt.Fprintf(c, "%s %s\n", rows, cols); err != nil {
		return fmt.Errorf("send resize: %w", err)
	}
	var reply string
	if _, err = fmt.Fscan(c, &reply); err != nil {
		return fmt.Errorf("read resize: %w", err)
	}
	if reply != "OK" {
		return fmt.Errorf("resize failed: %s", reply)
	}
	return nil
}

func run(op, dir string) (int, error) {
	if err := unix.Prctl(unix.PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0); err != nil {
		return 0, fmt.Errorf("subreaper: %w", err)
	}
	socket, err := runc.NewConsoleSocket(filepath.Join(dir, "console.sock"))
	if err != nil {
		return 0, fmt.Errorf("console socket: %w", err)
	}
	defer socket.Close()
	ctl, err := net.Listen("unix", filepath.Join(dir, "resize.sock"))
	if err != nil {
		return 0, fmt.Errorf("resize socket: %w", err)
	}
	defer ctl.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	args := []string{"--root", "/run/runc", "--log", filepath.Join(dir, "runc.log"), "--log-format", "json"}
	action := []string{"create", "--bundle", dir}
	if op == "EXEC" {
		action = []string{"exec", "--detach", "--process", filepath.Join(dir, "config.json")}
	}
	action = append(action, "--console-socket", socket.Path(), "--pid-file", filepath.Join(dir, "child.pid"), "workload")
	cmd := exec.CommandContext(ctx, "/usr/bin/runc", append(args, action...)...)
	cmd.Stderr = os.Stderr
	if err := cmd.Start(); err != nil {
		return 0, fmt.Errorf("start runc: %w", err)
	}
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()
	type received struct {
		master console.Console
		err    error
	}
	masterC := make(chan received, 1)
	go func() { m, e := socket.ReceiveMaster(); masterC <- received{m, e} }()
	var master console.Console
	select {
	case result := <-masterC:
		if result.err != nil {
			<-done
			return 0, fmt.Errorf("receive console: %w", result.err)
		}
		master = result.master
		if err := <-done; err != nil {
			master.Close()
			return 0, fmt.Errorf("runc terminal: %w", err)
		}
	case err := <-done:
		if err != nil {
			return 0, fmt.Errorf("runc terminal: %w", err)
		}
		select {
		case result := <-masterC:
			if result.err != nil {
				return 0, result.err
			}
			master = result.master
		case <-ctx.Done():
			return 0, fmt.Errorf("receive console: %w", ctx.Err())
		}
	case <-ctx.Done():
		<-done
		return 0, fmt.Errorf("runc terminal timeout: %w", ctx.Err())
	}
	defer master.Close()
	epoller, err := console.NewEpoller()
	if err != nil {
		return 0, fmt.Errorf("console poller: %w", err)
	}
	defer epoller.Close()
	terminal, err := epoller.Add(master)
	if err != nil {
		return 0, fmt.Errorf("poll console: %w", err)
	}
	go func() { _ = epoller.Wait() }()
	go serveResize(ctl, master)
	output := make(chan struct{})
	go func() { _, _ = io.Copy(os.Stdout, terminal); close(output) }()
	go func() { _, _ = io.Copy(terminal, os.Stdin) }()
	if op == "RUN" {
		start := exec.CommandContext(ctx, "/usr/bin/runc", append(args, "start", "workload")...)
		start.Stderr = os.Stderr
		if err := start.Run(); err != nil {
			return 0, fmt.Errorf("runc start: %w", err)
		}
	}
	raw, err := os.ReadFile(filepath.Join(dir, "child.pid"))
	if err != nil {
		return 0, fmt.Errorf("read guest pid: %w", err)
	}
	pid, err := strconv.Atoi(string(raw))
	if err != nil || pid <= 0 {
		return 0, fmt.Errorf("invalid guest pid %q", raw)
	}
	if err := os.Rename(filepath.Join(dir, "child.pid"), filepath.Join(dir, "pid")); err != nil {
		return 0, fmt.Errorf("publish pid: %w", err)
	}
	var status unix.WaitStatus
	for {
		reaped, e := unix.Wait4(-1, &status, 0, nil)
		if e == unix.EINTR {
			continue
		}
		if e != nil {
			err = e
			break
		}
		if reaped == pid {
			err = nil
			break
		}
	}
	if err != nil {
		return 0, fmt.Errorf("wait guest pid %d: %w", pid, err)
	}
	if op == "RUN" {
		cleanup := exec.Command("/usr/bin/runc", append(args, "delete", "--force", "workload")...)
		cleanup.Stderr = os.Stderr
		if err := cleanup.Run(); err != nil {
			return 0, fmt.Errorf("destroy guest container: %w", err)
		}
	}
	_ = terminal.Shutdown(epoller.CloseConsole)
	<-output
	return exitCode(status), nil
}

func exitCode(status unix.WaitStatus) int {
	if status.Signaled() {
		return 128 + int(status.Signal())
	}
	return status.ExitStatus()
}

func serveResize(listener net.Listener, master console.Console) {
	for {
		c, err := listener.Accept()
		if err != nil {
			return
		}
		_ = c.SetDeadline(time.Now().Add(time.Second))
		var rows, cols uint16
		_, err = fmt.Fscan(io.LimitReader(c, 64), &rows, &cols)
		if err == nil && rows > 0 && cols > 0 {
			err = master.Resize(console.WinSize{Height: rows, Width: cols})
		} else {
			err = fmt.Errorf("invalid size")
		}
		if err == nil {
			_, _ = fmt.Fprintln(c, "OK")
		} else {
			_, _ = fmt.Fprintln(c, "ERROR")
		}
		c.Close()
	}
}

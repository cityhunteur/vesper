package shares

import (
	"context"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

// JailSocket is the host side of a shim microVM's vsock device. A Docker task, which has a bound network namespace,
// has its jail on the data disk (ADR 0011 §5); every other one in tmpfs. vesper-microvm's jail_base is the same rule.
func JailSocket(id string) string {
	root := "/srv/jailer"
	if _, err := os.Stat("/run/vesper/microvm/ctrns/" + id); err == nil {
		root = "/var/lib/vesper/state/microvm/jailer"
	}
	return root + "/firecracker/" + id + "/root/v.sock"
}

// DialUnix connects to a Unix socket. A path longer than sun_path allows (a Docker task's jail on the data disk,
// ADR 0011 §5) is reached through its directory: /proc/self/fd/N/<name>.
func DialUnix(ctx context.Context, path string) (net.Conn, error) {
	if len(path) < 100 {
		return (&net.Dialer{}).DialContext(ctx, "unix", path)
	}
	dir, err := os.Open(filepath.Dir(path))
	if err != nil {
		return nil, err
	}
	defer dir.Close()
	return (&net.Dialer{}).DialContext(ctx, "unix", fmt.Sprintf("/proc/self/fd/%d/%s", dir.Fd(), filepath.Base(path)))
}

// DialGuest connects to a guest vsock port through Firecracker's Unix socket ("CONNECT <port>", then "OK <n>").
func DialGuest(sock string, port int, timeout time.Duration) (net.Conn, error) {
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()
	c, err := DialUnix(ctx, sock)
	if err != nil {
		return nil, err
	}
	_ = c.SetDeadline(time.Now().Add(timeout))
	if _, err := fmt.Fprintf(c, "CONNECT %d\n", port); err != nil {
		c.Close()
		return nil, err
	}
	line := make([]byte, 0, 80)
	var b [1]byte
	for len(line) < 80 { // one byte at a time, so no stream byte is taken
		if _, err := io.ReadFull(c, b[:]); err != nil {
			c.Close()
			return nil, err
		}
		line = append(line, b[0])
		if b[0] == '\n' {
			break
		}
	}
	if !strings.HasPrefix(string(line), "OK ") {
		c.Close()
		return nil, fmt.Errorf("vsock bridge answered %q", line)
	}
	_ = c.SetDeadline(time.Time{})
	return c, nil
}

// NotifyBind tells the guest behind sock to look again at the binds of file export n: an event with an empty rest
// (D-052). The share server's supervisor sends it after it attached the file that replaced the export's file. It
// returns the guest's answer (Replay's "BIND n ..." line): "same", "again", "failed" or "none", or "" when the guest
// sent no such line in time. A guest writes the line, so nothing else of it is kept.
func NotifyBind(sock string, n int) (string, error) {
	c, err := DialGuest(sock, EventPort, 2*time.Second)
	if err != nil {
		return "", err
	}
	defer c.Close()
	_ = c.SetDeadline(time.Now().Add(3 * time.Second))
	if _, err := io.WriteString(c, Event{Export: n}.Line()); err != nil {
		return "", err
	}
	want := "BIND " + strconv.Itoa(n) + " "
	line := make([]byte, 0, 64)
	var b [1]byte
	for len(line) < 64 {
		if _, err := io.ReadFull(c, b[:]); err != nil || b[0] == '\n' {
			break
		}
		line = append(line, b[0])
	}
	switch answer := strings.TrimPrefix(string(line), want); answer {
	case "same", "again", "failed", "none":
		if strings.HasPrefix(string(line), want) {
			return answer, nil
		}
	}
	return "", nil
}

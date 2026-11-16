package shares

import (
	"fmt"
	"io"
	"net"
	"strings"
	"time"
)

// JailSocket is the host side of a shim microVM's vsock device.
func JailSocket(id string) string { return "/srv/jailer/firecracker/" + id + "/root/v.sock" }

// DialGuest connects to a guest vsock port through Firecracker's Unix socket ("CONNECT <port>", then "OK <n>").
func DialGuest(sock string, port int, timeout time.Duration) (net.Conn, error) {
	c, err := net.DialTimeout("unix", sock, timeout)
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

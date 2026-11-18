package shares

import (
	"fmt"
	"io"
	"net"
	"strconv"
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

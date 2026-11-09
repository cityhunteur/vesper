package backend

import (
	"bufio"
	"context"
	"fmt"
	"io"
	"net"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"time"

	"github.com/containerd/fifo"
)

// IO names the containerd FIFOs for a session.
type IO struct {
	Stdin, Stdout, Stderr string
	Terminal              bool
}

// Session owns the control connection and unframed stdio connections of one guest process.
type Session struct {
	control *net.UnixConn
	reader  *bufio.Reader
	streams []*net.UnixConn
	input   io.ReadWriteCloser
	mu      sync.Mutex
	outputs sync.WaitGroup
	fifos   []io.ReadWriteCloser
	cancel  context.CancelFunc
}

func (m *MicroVM) socket(id string) string { return "/srv/jailer/firecracker/" + id + "/root/v.sock" }

func dialPort(ctx context.Context, path string, port uint32) (*net.UnixConn, error) {
	var last error
	for {
		c, err := (&net.Dialer{}).DialContext(ctx, "unix", path)
		if err == nil {
			u := c.(*net.UnixConn)
			_ = u.SetDeadline(time.Now().Add(2 * time.Second))
			if _, err = fmt.Fprintf(u, "CONNECT %d\n", port); err == nil {
				// Read one byte at a time so the handshake cannot consume stream bytes.
				line := make([]byte, 0, 80)
				var b [1]byte
				for len(line) < 80 {
					if _, err = io.ReadFull(u, b[:]); err != nil {
						break
					}
					line = append(line, b[0])
					if b[0] == '\n' {
						break
					}
				}
				if err == nil && strings.HasPrefix(string(line), "OK ") {
					_ = u.SetDeadline(time.Time{})
					return u, nil
				}
				if err == nil {
					err = fmt.Errorf("bridge response %q", line)
				}
			}
			u.Close()
		}
		last = err
		select {
		case <-ctx.Done():
			return nil, fmt.Errorf("connect vsock port %d: %v: %w", port, last, ctx.Err())
		case <-time.After(20 * time.Millisecond):
		}
	}
}

// OpenSession connects the control channel, then the three stdio streams. The guest launches runc at
// READY (before these stream dials), so the stdio bridges buffer early output; a process that writes
// more than the pipe buffer and exits before the host connects can block the guest writer until then.
func (m *MicroVM) OpenSession(ctx context.Context, id string, port uint32, config []byte, isExec bool, paths IO) (_ *Session, err error) {
	if len(config) == 0 || len(config) > 1048576 {
		return nil, fmt.Errorf("guest config must be 1..1048576 bytes")
	}
	startCtx, cancelStart := context.WithTimeout(ctx, 30*time.Second)
	defer cancelStart()
	c, err := dialPort(startCtx, m.socket(id), 1025)
	if err != nil {
		return nil, err
	}
	life, cancel := context.WithCancel(context.Background())
	s := &Session{control: c, reader: bufio.NewReader(c), cancel: cancel}
	defer func() {
		if err != nil {
			s.Close()
		}
	}()
	_ = c.SetDeadline(time.Now().Add(30 * time.Second))
	op := "RUN"
	if isExec {
		op = "EXEC"
	}
	tty := 0
	if paths.Terminal {
		tty = 1
	}
	if _, err = fmt.Fprintf(c, "%s %d %d %d\n", op, port, len(config), tty); err != nil {
		return nil, fmt.Errorf("send start: %w", err)
	}
	if _, err = c.Write(config); err != nil {
		return nil, fmt.Errorf("send config: %w", err)
	}
	line, err := s.line()
	if err != nil {
		return nil, err
	}
	if line != fmt.Sprintf("READY %d", port) {
		return nil, fmt.Errorf("guest start p%d: %s", port, line)
	}
	for i := range uint32(3) {
		stream, e := dialPort(startCtx, m.socket(id), port+i)
		if e != nil {
			return nil, e
		}
		s.streams = append(s.streams, stream)
		path := paths.Stdin
		mode := syscall.O_RDONLY
		if i == 1 {
			path = paths.Stdout
			mode = syscall.O_WRONLY
		}
		if i == 2 {
			path = paths.Stderr
			mode = syscall.O_WRONLY
		}
		var f io.ReadWriteCloser
		if path != "" {
			f, e = fifo.OpenFifo(life, path, mode|syscall.O_NONBLOCK, 0)
			if e != nil {
				return nil, fmt.Errorf("open FIFO %s: %w", path, e)
			}
		}
		if f != nil {
			s.fifos = append(s.fifos, f)
		}
		if i == 0 {
			s.input = f
			go func() {
				if f != nil {
					_, _ = io.Copy(stream, f)
					_ = f.Close()
				}
				_ = stream.CloseWrite()
			}()
		} else {
			s.outputs.Go(func() {
				if f != nil {
					defer f.Close()
					if _, err := io.Copy(f, stream); err != nil {
						_, _ = io.Copy(io.Discard, stream)
					}
				} else {
					_, _ = io.Copy(io.Discard, stream)
				}
			})
		}
	}
	line, err = s.line()
	if err != nil {
		return nil, err
	}
	if !strings.HasPrefix(line, "STARTED ") {
		return nil, fmt.Errorf("guest start p%d: %s", port, line)
	}
	if pid, e := strconv.Atoi(strings.TrimPrefix(line, "STARTED ")); e != nil || pid <= 0 {
		return nil, fmt.Errorf("invalid guest pid: %q", line)
	}
	_ = c.SetDeadline(time.Time{})
	return s, nil
}

func (s *Session) line() (string, error) {
	raw, err := s.reader.ReadSlice('\n')
	line := string(raw)
	if err != nil {
		return "", fmt.Errorf("guest control reply %q: %w", line, err)
	}
	return strings.TrimSpace(line), nil
}

// Wait returns runc's foreground status; stream EOF is never interpreted as success.
func (s *Session) Wait() (uint32, error) {
	line, err := s.line()
	if err != nil {
		return 137, err
	}
	if !strings.HasPrefix(line, "EXIT ") {
		return 137, fmt.Errorf("guest process: %s", line)
	}
	code, err := strconv.ParseUint(strings.TrimPrefix(line, "EXIT "), 10, 8)
	if err != nil {
		return 137, fmt.Errorf("guest exit %q: %w", line, err)
	}
	s.outputs.Wait()
	return uint32(code), nil
}

func (s *Session) CloseInput() {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.input != nil {
		_ = s.input.Close()
	}
	if len(s.streams) > 0 {
		_ = s.streams[0].CloseWrite()
	}
}
func (s *Session) Close() {
	s.cancel()
	s.CloseInput()
	for _, f := range s.fifos {
		_ = f.Close()
	}
	for _, c := range s.streams {
		c.Close()
	}
	s.control.Close()
}

func (m *MicroVM) Signal(ctx context.Context, id string, port, signal uint32, all bool) error {
	if signal > 64 {
		return fmt.Errorf("signal must be 0..64")
	}
	ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	c, err := dialPort(ctx, m.socket(id), 1025)
	if err != nil {
		return err
	}
	defer c.Close()
	_ = c.SetDeadline(time.Now().Add(5 * time.Second))
	a := 0
	if all {
		a = 1
	}
	if _, err = fmt.Fprintf(c, "SIGNAL %d %d %d\n", port, signal, a); err != nil {
		return fmt.Errorf("send signal: %w", err)
	}
	raw, err := bufio.NewReader(c).ReadSlice('\n')
	line := string(raw)
	if err != nil {
		return fmt.Errorf("signal reply: %w", err)
	}
	if strings.TrimSpace(line) != "OK" {
		return fmt.Errorf("signal p%d: %s", port, strings.TrimSpace(line))
	}
	return nil
}

func (m *MicroVM) Resize(ctx context.Context, id string, port, rows, cols uint32) error {
	if rows == 0 || cols == 0 || rows > 65535 || cols > 65535 {
		return fmt.Errorf("terminal size must be 1..65535")
	}
	ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	c, err := dialPort(ctx, m.socket(id), 1025)
	if err != nil {
		return err
	}
	defer c.Close()
	_ = c.SetDeadline(time.Now().Add(5 * time.Second))
	if _, err = fmt.Fprintf(c, "RESIZE %d %d %d\n", port, rows, cols); err != nil {
		return fmt.Errorf("send resize: %w", err)
	}
	raw, err := bufio.NewReader(c).ReadSlice('\n')
	line := string(raw)
	if err != nil {
		return fmt.Errorf("resize reply: %w", err)
	}
	if strings.TrimSpace(line) != "OK" {
		return fmt.Errorf("resize p%d: %s", port, strings.TrimSpace(line))
	}
	return nil
}

func (m *MicroVM) Abort(ctx context.Context, id string, port uint32) error {
	ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	c, err := dialPort(ctx, m.socket(id), 1025)
	if err != nil {
		return err
	}
	defer c.Close()
	_ = c.SetDeadline(time.Now().Add(5 * time.Second))
	if _, err = fmt.Fprintf(c, "ABORT %d 0 0\n", port); err != nil {
		return fmt.Errorf("abort session: %w", err)
	}
	line, err := bufio.NewReader(c).ReadSlice('\n')
	if err != nil {
		return fmt.Errorf("abort reply: %w", err)
	}
	if strings.TrimSpace(string(line)) != "OK" {
		return fmt.Errorf("abort p%d: %s", port, line)
	}
	return nil
}

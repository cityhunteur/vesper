package backend

import (
	"bufio"
	"context"
	"io"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestDialPortRetainsPayload(t *testing.T) {
	path := filepath.Join(t.TempDir(), "v.sock")
	l, err := net.Listen("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	defer l.Close()
	done := make(chan error, 1)
	go func() {
		c, e := l.Accept()
		if e != nil {
			done <- e
			return
		}
		defer c.Close()
		line, e := bufio.NewReader(c).ReadString('\n')
		if e == nil && line != "CONNECT 1029\n" {
			e = io.ErrUnexpectedEOF
		}
		if e == nil {
			_, e = io.WriteString(c, "OK 42\n\x00a\nb\xff")
		}
		done <- e
	}()
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	c, err := dialPort(ctx, path, 1029)
	if err != nil {
		t.Fatal(err)
	}
	defer c.Close()
	got, err := io.ReadAll(c)
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != "\x00a\nb\xff" {
		t.Fatalf("payload lost or reframed: %q", got)
	}
	if err := <-done; err != nil {
		t.Fatal(err)
	}
}

func TestSessionExit(t *testing.T) {
	for _, tc := range []struct {
		reply string
		code  uint32
		bad   bool
	}{
		{"EXIT 0\n", 0, false}, {"EXIT 7\n", 7, false}, {"EXIT 137\n", 137, false}, {"EXIT 143\n", 143, false},
		{"", 137, true},
		{strings.Repeat("X", 8192) + "\n", 137, true}, {"EXIT 256\n", 137, true}, {"EXIT -1\n", 137, true}, {"ERROR {\"message\":\"runc failed\"}\n", 137, true},
	} {
		s := &Session{reader: bufio.NewReader(strings.NewReader(tc.reply))}
		got, err := s.Wait()
		if got != tc.code || (err != nil) != tc.bad {
			t.Errorf("%q: code=%d err=%v", tc.reply, got, err)
		}
	}
}

func TestCloseUnblocksOutputFIFO(t *testing.T) {
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer reader.Close()
	path := filepath.Join(t.TempDir(), "control.sock")
	listener, err := net.Listen("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	accepted := make(chan net.Conn, 1)
	go func() { c, _ := listener.Accept(); accepted <- c }()
	c, err := net.Dial("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	peer := <-accepted
	defer peer.Close()
	s := &Session{control: c.(*net.UnixConn), fifos: []io.ReadWriteCloser{writer}, cancel: func() {}}
	done := make(chan error, 1)
	go func() { _, err := writer.Write(make([]byte, 1<<20)); done <- err }()
	s.Close()
	select {
	case err := <-done:
		if err == nil {
			t.Fatal("blocked output unexpectedly succeeded")
		}
	case <-time.After(time.Second):
		t.Fatal("session close left output blocked")
	}
}

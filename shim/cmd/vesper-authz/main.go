// vesper-authz is dockerd's authorisation plugin in the System VM (ADR 0010). dockerd asks it about every Docker
// API request and refuses the request when it says no, or when it does not answer.
//
// usage: vesper-authz [-socket PATH]
package main

import (
	"errors"
	"flag"
	"fmt"
	"net"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"
	"time"

	"github.com/cityhunteur/vesper/shim/internal/authz"
)

func main() {
	socket := flag.String("socket", "/run/docker/plugins/vesper-authz.sock", "the plugin socket dockerd connects to")
	flag.Parse()
	if err := run(*socket); err != nil {
		fmt.Fprintln(os.Stderr, "vesper-authz:", err)
		os.Exit(1)
	}
}

func run(socket string) error {
	if err := os.MkdirAll(filepath.Dir(socket), 0o700); err != nil {
		return err
	}
	// A socket left by a plugin that was killed would make the bind fail. Anything else at that path is not ours.
	if fi, err := os.Lstat(socket); err == nil {
		if fi.Mode()&os.ModeSocket == 0 {
			return fmt.Errorf("%s exists and is not a socket", socket)
		}
		if err := os.Remove(socket); err != nil {
			return err
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	old := syscall.Umask(0o177) // the socket is created owner-only; only dockerd, as root, connects
	l, err := net.Listen("unix", socket)
	syscall.Umask(old)
	if err != nil {
		return err
	}
	logf := func(format string, a ...any) {
		fmt.Fprintf(os.Stderr, "%s vesper-authz: %s\n", time.Now().UTC().Format(time.RFC3339), fmt.Sprintf(format, a...))
	}
	srv := &http.Server{Handler: authz.Handler(logf), ReadHeaderTimeout: 10 * time.Second}
	stop := make(chan os.Signal, 1)
	signal.Notify(stop, syscall.SIGTERM, syscall.SIGINT)
	go func() {
		<-stop
		_ = srv.Close()
	}()
	logf("listening on %s", socket)
	err = srv.Serve(l)
	_ = os.Remove(socket)
	if errors.Is(err, http.ErrServerClosed) {
		return nil
	}
	return err
}

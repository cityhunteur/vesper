//go:build linux

// vesper-unfs runs one unfs3 server for one shim microVM inside a sandbox (ADR 0009, GH #160 M3), and probes one.
//
//	vesper-unfs run --mount DIR --src REL:ro|rw [--src ...] [--squash] [--root DIR] [--cgroup DIR] [--unfsd PATH]
//	vesper-unfs run ... --selfcheck   (the same sandbox, running the escape checks in place of unfsd)
//	vesper-unfs probe [--server ADDR] [--local ADDR] [--uid N --gid N] [--timeout DUR]
//
// run is started by vesper-microvm under supervise-daemon, inside the microVM's network namespace. It opens each
// source beneath the share mount with no symbolic link on the way, then starts its own `init` in new mount, pid, ipc
// and uts namespaces, which builds a root holding only the exports, pivots into it, drops privileges and becomes
// unfsd. probe runs the M3 battery (nfsprobe.Battery) against a server; it is a test tool.
package main

import (
	"flag"
	"fmt"
	"io"
	"os"
	"strings"
	"time"

	"github.com/cityhunteur/vesper/shim/internal/nfsprobe"
	"github.com/cityhunteur/vesper/shim/internal/unfs"
)

type srcFlags []unfs.Source

func (s *srcFlags) String() string { return fmt.Sprint(*s) }
func (s *srcFlags) Set(v string) error {
	src, err := unfs.ParseSource(v)
	if err != nil {
		return err
	}
	*s = append(*s, src)
	return nil
}

func main() {
	if len(os.Args) < 2 {
		usage()
	}
	switch os.Args[1] {
	case "run":
		os.Exit(run(os.Args[2:]))
	case "init":
		err := unfs.Init() // returns only on failure: on success it is unfsd
		fmt.Fprintf(os.Stderr, "vesper-unfs init: %v\n", err)
		os.Exit(1)
	case "probe":
		os.Exit(probe(os.Args[2:]))
	case "selfcheck": // inside the sandbox only (run --selfcheck)
		mode, self := "root", ""
		if len(os.Args) > 2 {
			mode = os.Args[2]
		}
		if len(os.Args) > 3 {
			self = os.Args[3]
		}
		if !unfs.Selfcheck(mode, self, os.Stdout) {
			os.Exit(1)
		}
	case "selfcheck-userns": // started by selfcheck in a new user and mount namespace
		if !unfs.SelfcheckUserns(os.Stdout) {
			os.Exit(1)
		}
	default:
		usage()
	}
}

func usage() {
	fmt.Fprintln(os.Stderr, "usage: vesper-unfs run --mount DIR --src REL:ro|rw ... [--squash] [--root DIR] [--cgroup DIR] | probe [flags]")
	os.Exit(2)
}

func run(args []string) int {
	fs := flag.NewFlagSet("run", flag.ContinueOnError)
	var cfg unfs.Config
	var srcs srcFlags
	fs.StringVar(&cfg.Mount, "mount", "", "the virtio-fs share mount the sources lie in")
	fs.Var(&srcs, "src", "a source REL:ro or REL:rw, relative to --mount (repeatable)")
	fs.BoolVar(&cfg.Squash, "squash", false, "run unfsd as 65534 with no capabilities (all_squash)")
	fs.StringVar(&cfg.Root, "root", "/run/vesper/unfs-root", "an empty root-owned directory for the server's root")
	fs.StringVar(&cfg.Cgroup, "cgroup", "", "a cgroup directory to join first")
	fs.StringVar(&cfg.Unfsd, "unfsd", "/usr/sbin/unfsd", "the unfsd binary (under /usr)")
	fs.BoolVar(&cfg.Selfcheck, "selfcheck", false, "run the escape checks in the sandbox in place of unfsd (tests)")
	fs.StringVar(&cfg.SelfcheckBin, "selfcheck-bin", "/usr/bin/vesper-unfs", "this binary as the sandbox sees it")
	fs.DurationVar(&cfg.TestPauseNests, "test-pause-nests", 0, "wait this long before placing nests (tests only)")
	fs.SetOutput(io.Discard)
	if err := fs.Parse(args); err != nil {
		fmt.Fprintf(os.Stderr, "vesper-unfs run: %v\n", err) // one line, which vesper-microvm looks for
		return 2
	}
	if cfg.Mount == "" || len(srcs) == 0 || fs.NArg() != 0 || !strings.HasPrefix(cfg.Unfsd, "/usr/") || !strings.HasPrefix(cfg.SelfcheckBin, "/usr/") {
		fmt.Fprintln(os.Stderr, "vesper-unfs run: need --mount, at least one --src, and --unfsd under /usr")
		return 2
	}
	cfg.Sources = srcs
	code, err := unfs.Run(cfg)
	if err != nil {
		fmt.Fprintf(os.Stderr, "vesper-unfs run: %v\n", err)
	}
	return code
}

func probe(args []string) int {
	fs := flag.NewFlagSet("probe", flag.ContinueOnError)
	b := nfsprobe.Battery{Out: os.Stdout}
	var uid, gid uint
	fs.StringVar(&b.Server, "server", unfs.ServerAddr, "server address")
	fs.StringVar(&b.Local, "local", unfs.GuestAddr+":0", "local address to call from")
	fs.UintVar(&uid, "uid", 0, "AUTH_SYS uid")
	fs.UintVar(&gid, "gid", 0, "AUTH_SYS gid")
	fs.DurationVar(&b.Timeout, "timeout", 5*time.Second, "per-call timeout")
	var out, in string
	fs.StringVar(&out, "handles-out", "", "save handles of /s/0 and /s/0/file.txt to this file, and stop")
	fs.StringVar(&in, "handles-in", "", "use handles saved by --handles-out (after a restart), and stop")
	if err := fs.Parse(args); err != nil {
		return 2
	}
	b.Cred = nfsprobe.Cred{UID: uint32(uid), GID: uint32(gid)}
	if out != "" || in != "" {
		var err error
		if out != "" {
			err = b.SaveHandles(out)
		} else {
			err = b.CheckHandles(in)
		}
		if err != nil {
			fmt.Fprintf(os.Stderr, "vesper-unfs probe: %v\n", err)
			return 1
		}
		return 0
	}
	if !b.Run() {
		return 1
	}
	return 0
}

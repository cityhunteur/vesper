//go:build linux

// vesper-unfs runs one unfs3 server for one shim microVM inside a sandbox (ADR 0009, GH #160 M3), and probes one.
//
//	vesper-unfs run --mount DIR --src REL:ro|rw [--src ...] [--mount DIR --src ...] [--squash] [--root DIR] [--cgroup DIR] [--unfsd PATH]
//	vesper-unfs run --args FILE [--root DIR] [--cgroup DIR] [--unfsd PATH]   (vesper-microvm's record; GH #207)
//	vesper-unfs run ... --protect FILE   (what each read-only source was, kept across restarts of this server; TD-073)
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
	"log/syslog"
	"net"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/cityhunteur/vesper/shim/internal/nfsprobe"
	"github.com/cityhunteur/vesper/shim/internal/shares"
	"github.com/cityhunteur/vesper/shim/internal/unfs"
)

// mountFlag and srcFlag fill one unfs.Args in command-line order: each --src belongs to the --mount before it.
type mountFlag struct{ a *unfs.Args }
type srcFlag struct{ a *unfs.Args }

func (f mountFlag) String() string { return "" }
func (f mountFlag) Set(v string) error {
	if err := unfs.CheckMount(v); err != nil {
		return err
	}
	f.a.Mounts = append(f.a.Mounts, v)
	return nil
}
func (f srcFlag) String() string { return "" }
func (f srcFlag) Set(v string) error {
	if len(f.a.Mounts) == 0 {
		return fmt.Errorf("--src %s comes before any --mount", v)
	}
	src, err := unfs.ParseSource(v)
	if err != nil {
		return err
	}
	src.Mount = len(f.a.Mounts) - 1
	f.a.Sources = append(f.a.Sources, src)
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
	case "fanout": // the share notification sink's paths, to each microVM that exports them (GH #160 M6)
		f := &shares.Fanout{
			Dir:      "/run/vesper/microvm/unfs",
			StatPath: "/run/vesper/microvm/unfs/fanout.stat",
			Dial: func(id string) (net.Conn, error) {
				return shares.DialGuest(shares.JailSocket(id), shares.EventPort, 2*time.Second)
			},
		}
		if err := f.Run(os.Stdin); err != nil {
			fmt.Fprintf(os.Stderr, "vesper-unfs fanout: %v\n", err)
			os.Exit(1)
		}
		time.Sleep(2 * time.Second) // let the senders drain what is queued
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
	fmt.Fprintln(os.Stderr, "usage: vesper-unfs run (--args FILE | --mount DIR --src REL:ro|rw ...) [--squash] [--root DIR] [--cgroup DIR] [--guest-sock PATH] [--protect FILE] | probe [flags]")
	os.Exit(2)
}

func run(args []string) int {
	fs := flag.NewFlagSet("run", flag.ContinueOnError)
	var cfg unfs.Config
	var a unfs.Args
	var argsFile, guestSock string
	fs.Var(mountFlag{&a}, "mount", "a virtio-fs share mount the sources after it lie in (repeatable)")
	fs.Var(srcFlag{&a}, "src", "a source REL:ro or REL:rw, relative to the --mount before it (repeatable)")
	fs.BoolVar(&a.Squash, "squash", false, "run unfsd as 65534 with no capabilities (all_squash)")
	fs.StringVar(&argsFile, "args", "", "read the mounts, sources and squash flag from vesper-microvm's record instead")
	fs.StringVar(&cfg.Root, "root", "/run/vesper/unfs-root", "an empty root-owned directory for the server's root")
	fs.StringVar(&cfg.Cgroup, "cgroup", "", "a cgroup directory to join first")
	fs.StringVar(&cfg.Unfsd, "unfsd", "/usr/libexec/vesper/unfsd", "the unfsd binary (under /usr; the default is Vesper's patched build, D-044)")
	fs.BoolVar(&cfg.Selfcheck, "selfcheck", false, "run the escape checks in the sandbox in place of unfsd (tests)")
	fs.StringVar(&cfg.SelfcheckBin, "selfcheck-bin", "/usr/bin/vesper-unfs", "this binary as the sandbox sees it")
	fs.DurationVar(&cfg.TestPauseNests, "test-pause-nests", 0, "wait this long before placing nests (tests only)")
	fs.StringVar(&cfg.ProtectFile, "protect", "", "a file that keeps, across restarts of this server, what each read-only source was (TD-073)")
	fs.StringVar(&guestSock, "guest-sock", "", "the microVM's vsock socket: the guest is told there when a file source was replaced (D-052)")
	fs.SetOutput(io.Discard)
	if err := fs.Parse(args); err != nil {
		fmt.Fprintf(os.Stderr, "vesper-unfs run: %v\n", err) // one line, which vesper-microvm looks for
		return 2
	}
	if argsFile != "" {
		if len(a.Mounts) != 0 || a.Squash {
			fmt.Fprintln(os.Stderr, "vesper-unfs run: --args excludes --mount, --src and --squash")
			return 2
		}
		raw, err := os.ReadFile(argsFile)
		if err == nil {
			a, err = unfs.ParseArgs(string(raw))
		}
		if err != nil {
			fmt.Fprintf(os.Stderr, "vesper-unfs run: --args: %v\n", err)
			return 2
		}
	}
	if err := a.Check(); err != nil || fs.NArg() != 0 || !strings.HasPrefix(cfg.Unfsd, "/usr/") || !strings.HasPrefix(cfg.SelfcheckBin, "/usr/") {
		fmt.Fprintf(os.Stderr, "vesper-unfs run: need --args or --mount with --src, and --unfsd under /usr (%v)\n", err)
		return 2
	}
	cfg.Mounts, cfg.Sources, cfg.Squash = a.Mounts, a.Sources, a.Squash
	if guestSock != "" {
		cfg.NotifyGuest = func(export int) (string, error) { return shares.NotifyBind(guestSock, export) }
	}
	started := time.Now()
	code, err := unfs.Run(cfg)
	if err != nil {
		fmt.Fprintf(os.Stderr, "vesper-unfs run: %v\n", err)
	}
	// Why and when a server ended, where it outlives the server's own log (which goes with the microVM): the System
	// VM's log, under the name vesper-microvm's unfs-log looks for. unfsd reports a segmentation fault on its
	// standard output, which nothing else keeps (TD-075). What matters comes first and the line is short: a system
	// logger may keep only the first 256 bytes of a message.
	if !cfg.Selfcheck {
		tag := "vesper-unfs"
		if argsFile != "" {
			tag += "-" + strings.TrimSuffix(filepath.Base(argsFile), ".args")
		}
		if w, serr := syslog.New(syslog.LOG_DAEMON|syslog.LOG_NOTICE, tag); serr == nil {
			words, told := unfs.LastWords(80)
			how := "by itself"
			if told {
				how = "as told"
			}
			_ = w.Notice(fmt.Sprintf("the server ended %s: code %d after %s (%v); last output: %q", how, code, time.Since(started).Round(time.Millisecond), err, words))
			w.Close()
		}
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

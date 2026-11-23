package bundle

import (
	"encoding/binary"
	"fmt"
	"path"
	"runtime"
	"strings"

	specs "github.com/opencontainers/runtime-spec/specs-go"
)

// A vesper microVM runs the System VM's own architecture and has no emulator (ADR 0011 slice 4b). An image built
// for another one fails inside the guest with "exec format error"; CheckArch refuses it at create instead, with a
// message that names the runtime which can run it (D-099 G3, TD-100).

// FallbackRuntime is the Docker runtime that runs other architectures' images, through the System VM's binfmt
// emulators.
const FallbackRuntime = "vesper-runc"

// defaultPath is runc's PATH when the process gives none.
const defaultPath = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"

// headerLen is what CheckArch reads of a program: an ELF header's machine field, or a "#!" line.
const headerLen = 256

// elfMachine is the e_machine value of the architecture the shim runs on.
var elfMachine = map[string]uint16{"arm64": 183, "amd64": 62}[runtime.GOARCH]

var elfPlatform = map[uint16]string{
	3:   "linux/386",
	8:   "linux/mips",
	20:  "linux/ppc",
	21:  "linux/ppc64",
	22:  "linux/s390x",
	40:  "linux/arm",
	62:  "linux/amd64",
	183: "linux/arm64",
	243: "linux/riscv64",
	258: "linux/loong64",
}

// headerKind is what the start of a program file is.
type headerKind int

const (
	headerOther  headerKind = iota // neither ELF nor "#!": the guest decides
	headerELF                      // an ELF file; machine is its e_machine
	headerScript                   // a "#!" line; interp is the interpreter's path
)

// parseHeader reads the first bytes of a program file.
func parseHeader(b []byte) (kind headerKind, machine uint16, interp string) {
	if len(b) >= 20 && string(b[:4]) == "\x7fELF" {
		switch b[5] { // EI_DATA
		case 1:
			return headerELF, binary.LittleEndian.Uint16(b[18:20]), ""
		case 2:
			return headerELF, binary.BigEndian.Uint16(b[18:20]), ""
		}
		return headerOther, 0, ""
	}
	if len(b) >= 2 && b[0] == '#' && b[1] == '!' {
		line, _, _ := strings.Cut(string(b[2:]), "\n")
		if f := strings.Fields(line); len(f) > 0 && path.IsAbs(f[0]) {
			return headerScript, 0, f[0]
		}
	}
	return headerOther, 0, ""
}

// platformName names an ELF machine as a Docker platform.
func platformName(m uint16) string {
	if p, ok := elfPlatform[m]; ok {
		return p
	}
	return fmt.Sprintf("another architecture (ELF machine %d)", m)
}

// archError is CheckArch's refusal.
func archError(prog string, m uint16) error {
	return fmt.Errorf("vesper: %s in the image is built for %s, and a vesper microVM runs %s only; "+
		"run the container with --runtime %s, which runs other architectures under emulation (ADR 0011)",
		prog, platformName(m), platformName(elfMachine), FallbackRuntime)
}

// programCandidates lists, in the order runc's exec would try them, the paths in the rootfs that the process's first
// argument can name: itself when it has a slash (relative to the working directory), else each PATH entry. A path is
// not cleaned: ".." after a symbolic link is resolved by the open, as the kernel does, and not removed by text
// (council review-01-f01).
func programCandidates(p *specs.Process) []string {
	if p == nil || len(p.Args) == 0 || p.Args[0] == "" {
		return nil
	}
	arg := p.Args[0]
	if strings.Contains(arg, "/") {
		if path.IsAbs(arg) {
			return []string{arg}
		}
		return []string{strings.TrimSuffix(p.Cwd, "/") + "/" + arg}
	}
	search := defaultPath
	for _, e := range p.Env {
		if v, ok := strings.CutPrefix(e, "PATH="); ok {
			search = v
		}
	}
	var out []string
	for dir := range strings.SplitSeq(search, ":") {
		if path.IsAbs(dir) {
			out = append(out, strings.TrimSuffix(dir, "/")+"/"+arg)
		}
	}
	return out
}

// maxInterpreters is how many "#!" levels the kernel follows (BINPRM_MAX_RECURSION).
const maxInterpreters = 4

// checkArch is CheckArch with the file reader given: read returns the first bytes of an executable regular file in
// the rootfs (none for an empty one), and ok=false when there is none at that path.
func checkArch(p *specs.Process, read func(string) (header []byte, ok bool)) error {
	if elfMachine == 0 {
		return nil
	}
	for _, c := range programCandidates(p) {
		b, ok := read(c)
		if !ok {
			continue
		}
		// A script's interpreter is followed as the kernel runs it, to its depth limit.
		prog := c
		for range maxInterpreters + 1 {
			kind, m, interp := parseHeader(b)
			if kind == headerELF && m != elfMachine {
				return archError(prog, m)
			}
			if kind != headerScript {
				break
			}
			if b, ok = read(interp); !ok {
				break
			}
			prog = interp
		}
		return nil // the first file found is the one exec runs
	}
	return nil // no such program: the guest reports it as runc does
}

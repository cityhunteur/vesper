package bundle

import (
	"encoding/binary"
	"reflect"
	"strings"
	"testing"

	specs "github.com/opencontainers/runtime-spec/specs-go"
)

// elfHeader is the first 20 bytes of an ELF file for machine m.
func elfHeader(m uint16, bigEndian bool) []byte {
	b := make([]byte, 64)
	copy(b, "\x7fELF")
	b[4] = 2 // 64-bit
	if bigEndian {
		b[5] = 2
		binary.BigEndian.PutUint16(b[18:], m)
	} else {
		b[5] = 1
		binary.LittleEndian.PutUint16(b[18:], m)
	}
	return b
}

func TestParseHeader(t *testing.T) {
	for _, c := range []struct {
		name    string
		in      []byte
		kind    headerKind
		machine uint16
		interp  string
	}{
		{"amd64", elfHeader(62, false), headerELF, 62, ""},
		{"arm64", elfHeader(183, false), headerELF, 183, ""},
		{"s390x big-endian", elfHeader(22, true), headerELF, 22, ""},
		{"short ELF", []byte("\x7fELF\x02\x01"), headerOther, 0, ""},
		{"bad EI_DATA", append([]byte("\x7fELF\x02\x07"), make([]byte, 20)...), headerOther, 0, ""},
		{"script", []byte("#!/bin/sh -e\necho hi\n"), headerScript, 0, "/bin/sh"},
		{"script env", []byte("#! /usr/bin/env python3\n"), headerScript, 0, "/usr/bin/env"},
		{"relative interpreter", []byte("#!sh\n"), headerOther, 0, ""},
		{"empty #!", []byte("#!\n"), headerOther, 0, ""},
		{"text", []byte("hello"), headerOther, 0, ""},
		{"empty", nil, headerOther, 0, ""},
	} {
		k, m, i := parseHeader(c.in)
		if k != c.kind || m != c.machine || i != c.interp {
			t.Errorf("%s: got (%v, %d, %q), want (%v, %d, %q)", c.name, k, m, i, c.kind, c.machine, c.interp)
		}
	}
}

func TestProgramCandidates(t *testing.T) {
	for _, c := range []struct {
		name string
		p    *specs.Process
		want []string
	}{
		{"nil", nil, nil},
		{"no args", &specs.Process{}, nil},
		{"absolute, not cleaned", &specs.Process{Args: []string{"/alias/../run"}}, []string{"/alias/../run"}},
		{"relative to cwd", &specs.Process{Args: []string{"./run"}, Cwd: "/app/"}, []string{"/app/./run"}},
		{"climbs from cwd", &specs.Process{Args: []string{"../run"}, Cwd: "/alias"}, []string{"/alias/../run"}},
		{"relative, no cwd", &specs.Process{Args: []string{"x/run"}}, []string{"/x/run"}},
		{"PATH from env", &specs.Process{Args: []string{"true"}, Env: []string{"A=1", "PATH=/opt/bin:rel:/bin"}},
			[]string{"/opt/bin/true", "/bin/true"}},
		{"PATH entry with a slash", &specs.Process{Args: []string{"true"}, Env: []string{"PATH=/opt/bin/"}},
			[]string{"/opt/bin/true"}},
		{"default PATH", &specs.Process{Args: []string{"true"}},
			[]string{"/usr/local/sbin/true", "/usr/local/bin/true", "/usr/sbin/true", "/usr/bin/true", "/sbin/true", "/bin/true"}},
	} {
		if got := programCandidates(c.p); !reflect.DeepEqual(got, c.want) {
			t.Errorf("%s: got %q, want %q", c.name, got, c.want)
		}
	}
}

// files is a fake rootfs: path to the program's first bytes.
type files map[string][]byte

func (f files) read(name string) ([]byte, bool) { b, ok := f[name]; return b, ok }

func TestCheckArch(t *testing.T) {
	if elfMachine == 0 {
		t.Skip("no microVM architecture for this GOARCH")
	}
	other := uint16(62)
	if elfMachine == 62 {
		other = 183
	}
	native, foreign := elfHeader(elfMachine, false), elfHeader(other, false)
	proc := func(args ...string) *specs.Process {
		return &specs.Process{Args: args, Env: []string{"PATH=/usr/bin:/bin"}}
	}
	for _, c := range []struct {
		name   string
		fs     files
		p      *specs.Process
		refuse string // the program the refusal names; "" passes
	}{
		{"native", files{"/bin/true": native}, proc("true"), ""},
		{"foreign", files{"/bin/true": foreign}, proc("true"), "/bin/true"},
		{"first on PATH wins", files{"/usr/bin/true": native, "/bin/true": foreign}, proc("true"), ""},
		{"foreign first on PATH", files{"/usr/bin/true": foreign, "/bin/true": native}, proc("true"), "/usr/bin/true"},
		{"missing", files{}, proc("true"), ""},
		{"script, native interpreter", files{"/app/run": []byte("#!/bin/sh\n"), "/bin/sh": native}, proc("/app/run"), ""},
		{"script, foreign interpreter", files{"/app/run": []byte("#!/bin/sh\n"), "/bin/sh": foreign}, proc("/app/run"), "/bin/sh"},
		{"script, missing interpreter", files{"/app/run": []byte("#!/bin/sh\n")}, proc("/app/run"), ""},
		{"nested scripts, foreign at the end", files{"/app/run": []byte("#!/app/i1\n"), "/app/i1": []byte("#!/app/i2\n"),
			"/app/i2": foreign}, proc("/app/run"), "/app/i2"},
		{"script loop", files{"/a": []byte("#!/b\n"), "/b": []byte("#!/a\n")}, proc("/a"), ""},
		{"empty file found first", files{"/usr/bin/true": {}, "/bin/true": foreign}, proc("true"), ""},
		{"neither ELF nor script", files{"/app/run": []byte("data")}, proc("/app/run"), ""},
	} {
		err := checkArch(c.p, c.fs.read)
		switch {
		case c.refuse == "" && err != nil:
			t.Errorf("%s: refused: %v", c.name, err)
		case c.refuse != "" && err == nil:
			t.Errorf("%s: passed, want a refusal naming %s", c.name, c.refuse)
		case c.refuse != "":
			for _, want := range []string{c.refuse + " in the image", platformName(other), "--runtime " + FallbackRuntime} {
				if !strings.Contains(err.Error(), want) {
					t.Errorf("%s: %q does not name %q", c.name, err, want)
				}
			}
		}
	}
}

func TestPlatformName(t *testing.T) {
	if got := platformName(62); got != "linux/amd64" {
		t.Errorf("62: %q", got)
	}
	if got := platformName(9999); !strings.Contains(got, "9999") {
		t.Errorf("9999: %q", got)
	}
}

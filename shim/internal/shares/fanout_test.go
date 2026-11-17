package shares

import (
	"bufio"
	"io"
	"net"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/cityhunteur/vesper/shim/internal/unfs"
)

func TestServerMap(t *testing.T) {
	s := Server{Mount: "/Users/me", Sources: []unfs.Source{{Rel: "proj"}, {Rel: "proj/conf", ReadOnly: true}, {Rel: "."}}}
	got := s.Map("/Users/me/proj/conf/app.conf")
	// The read-only source 1 gets no event: the guest cannot set times on it.
	want := []Event{{Export: 0, Rest: "conf/app.conf"}, {Export: 2, Rest: "proj/conf/app.conf"}}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("Map = %+v", got)
	}
	if got := s.Map("/Users/me/proj"); !reflect.DeepEqual(got, []Event{{Export: 2, Rest: "proj"}}) {
		t.Errorf("a source root is not replayed: %+v", got)
	}
	if got := s.Map("/Users/mex/proj/a"); got != nil {
		t.Errorf("outside the mount: %+v", got)
	}
	if got := (Server{Mount: "/m", Sources: []unfs.Source{{Rel: "a"}}}).Map("/m/ab/x"); got != nil {
		t.Errorf("a sibling prefix: %+v", got)
	}
}

func TestEventLine(t *testing.T) {
	e := Event{Export: 3, Rest: "a b/c"}
	got, err := ParseEvent(strings.TrimSuffix(e.Line(), "\n"))
	if err != nil || got != e {
		t.Fatalf("round trip %+v %v", got, err)
	}
	for _, bad := range []string{"3", "x\ta", "-1\ta", "64\ta", "0\t../a", "0\t/a", "0\ta//b"} {
		if _, err := ParseEvent(bad); err == nil {
			t.Errorf("accepted %q", bad)
		}
	}
}

func TestQueueCoalescesAndBounds(t *testing.T) {
	old := QueueLimit
	QueueLimit = 3
	defer func() { QueueLimit = old }()
	var q Queue
	for _, r := range []string{"a", "b", "a", "c", "d", "e", "d"} {
		q.Add(Event{Rest: r})
	}
	if q.Coalesced != 2 || q.Dropped != 2 || q.Len() != 3 {
		t.Fatalf("coalesced %d dropped %d len %d", q.Coalesced, q.Dropped, q.Len())
	}
	if got := q.Take(10); !reflect.DeepEqual(got, []Event{{Rest: "c"}, {Rest: "d"}, {Rest: "e"}}) {
		t.Fatalf("take %+v", got)
	}
	q.Add(Event{Rest: "c"})
	if q.Len() != 1 {
		t.Fatal("a taken event can be queued again")
	}
}

// A burst of edits reaches a fake guest coalesced, and a microVM whose record is gone is forgotten.
func TestFanoutDelivers(t *testing.T) {
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "vm1.args"), []byte("/Users/me\n0\nproj:rw\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	guest, host := net.Pipe()
	got := make(chan string, 2000)
	go func() {
		sc := bufio.NewScanner(guest)
		for sc.Scan() {
			got <- sc.Text()
		}
	}()
	f := &Fanout{Dir: dir, Dial: func(id string) (net.Conn, error) {
		if id != "vm1" {
			t.Errorf("dialled %q", id)
		}
		return host, nil
	}}
	in := strings.Builder{}
	for i := 0; i < 1000; i++ {
		in.WriteString("/Users/me/proj/same.txt\n")
	}
	in.WriteString("/Users/me/proj/other.txt\n/Users/elsewhere/x\n")
	if err := f.Run(strings.NewReader(in.String())); err != nil {
		t.Fatal(err)
	}
	seen := map[string]int{}
	deadline := time.After(3 * time.Second)
	for len(seen) < 2 {
		select {
		case l := <-got:
			seen[l]++
		case <-deadline:
			t.Fatalf("got %v", seen)
		}
	}
	time.Sleep(100 * time.Millisecond)
	st := f.Snapshot()["vm1"]
	// How many of the 1,000 identical paths coalesce depends on how fast the sender drains; every one is either
	// sent or coalesced.
	if st.Queued != 1001 || st.Sent+st.Coalesced != 1001 {
		t.Errorf("stats %+v", st)
	}
	if seen["0\tsame.txt"] < 1 || seen["0\tother.txt"] != 1 {
		t.Errorf("seen %v", seen)
	}
}

// A sender whose microVM's record is gone stops and forgets the microVM.
func TestFanoutForgetsAStoppedMicroVM(t *testing.T) {
	dir := t.TempDir()
	args := filepath.Join(dir, "vm2.args")
	if err := os.WriteFile(args, []byte("/m\n0\na:rw\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	guest, host := net.Pipe()
	defer guest.Close()
	go func() { _, _ = io.Copy(io.Discard, guest) }()
	f := &Fanout{Dir: dir, Idle: 20 * time.Millisecond, Dial: func(string) (net.Conn, error) { return host, nil }}
	f.Offer("/m/a/x")
	deadline := time.Now().Add(2 * time.Second)
	for f.Snapshot()["vm2"].Sent == 0 && time.Now().Before(deadline) {
		time.Sleep(10 * time.Millisecond)
	}
	if err := os.Remove(args); err != nil {
		t.Fatal(err)
	}
	for time.Now().Before(deadline) {
		if _, ok := f.Snapshot()["vm2"]; !ok {
			return
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatalf("vm2 is still tracked: %+v", f.Snapshot())
}

// An event can carry the rounded-up macOS mtime (TD-066, council REL-3); a queued path keeps its place and takes the
// later mtime; the fanout sends what Mtime reads.
func TestEventMtime(t *testing.T) {
	e := Event{Export: 2, Rest: "a/b", Mtime: 1794933864}
	if e.Line() != "2:1794933864\ta/b\n" {
		t.Fatalf("line %q", e.Line())
	}
	got, err := ParseEvent(strings.TrimSuffix(e.Line(), "\n"))
	if err != nil || got != e {
		t.Fatalf("round trip %+v %v", got, err)
	}
	if got, err := ParseEvent("2\ta:b"); err != nil || got != (Event{Export: 2, Rest: "a:b"}) {
		t.Fatalf("no mtime, colon in rest: %+v %v", got, err)
	}
	for _, bad := range []string{"2:\ta", "2:x\ta", "2:-5\ta", "2:0\ta", "2:+5\ta", "+2\ta", ":5\ta"} {
		if _, err := ParseEvent(bad); err == nil {
			t.Errorf("accepted %q", bad)
		}
	}
	for _, c := range []struct{ sec, nsec, want int64 }{{10, 0, 10}, {10, 1, 11}, {10, 999999999, 11}} {
		if got := RoundUp(c.sec, c.nsec); got != c.want {
			t.Errorf("RoundUp(%d, %d) = %d", c.sec, c.nsec, got)
		}
	}
	var q Queue
	q.Add(Event{Rest: "a", Mtime: 5})
	q.Add(Event{Rest: "b"})
	q.Add(Event{Rest: "a", Mtime: 7})
	q.Add(Event{Rest: "a", Mtime: 6})
	if got := q.Take(10); !reflect.DeepEqual(got, []Event{{Rest: "a", Mtime: 7}, {Rest: "b"}}) || q.Coalesced != 2 {
		t.Fatalf("take %+v coalesced %d", got, q.Coalesced)
	}

	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "vm1.args"), []byte("/Users/me\n0\nproj:rw\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	guest, host := net.Pipe()
	lines := make(chan string, 4)
	go func() {
		sc := bufio.NewScanner(guest)
		for sc.Scan() {
			lines <- sc.Text()
		}
	}()
	f := &Fanout{Dir: dir, Dial: func(string) (net.Conn, error) { return host, nil }, Mtime: func(mount, rel string) int64 {
		if mount != "/Users/me" || rel != "proj/x.txt" {
			t.Errorf("Mtime(%q, %q)", mount, rel)
		}
		return 1794933864
	}}
	f.Offer("/Users/me/proj/x.txt")
	select {
	case l := <-lines:
		if l != "0:1794933864\tx.txt" {
			t.Fatalf("sent %q", l)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("nothing sent")
	}
}

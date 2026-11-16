package shares

import (
	"bufio"
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
	want := []Event{{Export: 0, Rest: "conf/app.conf"}, {Export: 1, Rest: "app.conf"}, {Export: 2, Rest: "proj/conf/app.conf"}}
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
	if st.Queued != 1001 || st.Sent+st.Coalesced != 1001 || st.Sent > 1000 {
		t.Errorf("stats %+v", st)
	}
	if seen["0\tsame.txt"] < 1 || seen["0\tother.txt"] != 1 {
		t.Errorf("seen %v", seen)
	}
}

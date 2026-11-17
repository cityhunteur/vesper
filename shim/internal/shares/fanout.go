package shares

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/cityhunteur/vesper/shim/internal/unfs"
)

// The event hop (ADR 0009, GH #160 M6). The System VM's share notification sink (GH #150) replays each macOS edit on
// the virtio-fs share; Fanout then sends the path, as an export-relative "N<TAB>rest" line, to every running
// microVM whose server exports it. Each microVM has a bounded, coalescing queue: a path already waiting is not
// queued twice, and when the queue is full the oldest waiting path is dropped. The guest replays each line on its
// NFS mount (Replay).

// EventPort is the guest's vsock port for share events.
const EventPort = 1026

// QueueLimit bounds the paths waiting for one microVM.
var QueueLimit = 10000

// Server is what the fanout knows of one microVM's share server: its share mount and sources.
type Server struct {
	ID      string
	Mount   string
	Sources []unfs.Source
}

// Event is one path on one export: Export is the source index, Rest the path below it ("" for its root).
type Event struct {
	Export int
	Rest   string
}

// Line is the wire form of an event.
func (e Event) Line() string { return strconv.Itoa(e.Export) + "\t" + e.Rest + "\n" }

// ParseEvent reads a wire line (without its newline).
func ParseEvent(line string) (Event, error) {
	i := strings.IndexByte(line, '\t')
	if i < 0 {
		return Event{}, fmt.Errorf("event %q: no tab", line)
	}
	n, err := strconv.Atoi(line[:i])
	if err != nil || n < 0 || n >= unfs.MaxSources {
		return Event{}, fmt.Errorf("event %q: bad export", line)
	}
	rest := line[i+1:]
	if rest != "" {
		if err := unfs.CheckRel(rest); err != nil {
			return Event{}, err
		}
	}
	return Event{Export: n, Rest: rest}, nil
}

// Map returns the events a host path gives on this server: one per read-write source that holds it. The root of a
// source is not replayed (GH #150 never replays a share root either), nor is a read-only source: the guest can set
// no times on a read-only mount, so its watchers get no event (a recorded limit, ADR 0009).
func (s Server) Map(p string) []Event {
	if p != s.Mount && !strings.HasPrefix(p, s.Mount+"/") {
		return nil
	}
	rel := strings.TrimPrefix(strings.TrimPrefix(p, s.Mount), "/")
	var out []Event
	for i, src := range s.Sources {
		if src.ReadOnly {
			continue
		}
		var rest string
		switch {
		case src.Rel == ".":
			rest = rel
		case rel == src.Rel:
			rest = ""
		case strings.HasPrefix(rel, src.Rel+"/"):
			rest = rel[len(src.Rel)+1:]
		default:
			continue
		}
		if rest == "" || unfs.CheckRel(rest) != nil {
			continue
		}
		out = append(out, Event{Export: i, Rest: rest})
	}
	return out
}

// Queue is a bounded set of waiting events in arrival order.
type Queue struct {
	order              []Event
	waiting            map[Event]bool
	Coalesced, Dropped int
}

// Add queues e unless it already waits; when full it drops the oldest.
func (q *Queue) Add(e Event) {
	if q.waiting == nil {
		q.waiting = map[Event]bool{}
	}
	if q.waiting[e] {
		q.Coalesced++
		return
	}
	if len(q.order) >= QueueLimit {
		delete(q.waiting, q.order[0])
		q.order = q.order[1:]
		q.Dropped++
	}
	q.order = append(q.order, e)
	q.waiting[e] = true
}

// Take removes and returns up to n waiting events.
func (q *Queue) Take(n int) []Event {
	if n > len(q.order) {
		n = len(q.order)
	}
	out := append([]Event(nil), q.order[:n]...)
	q.order = q.order[n:]
	for _, e := range out {
		delete(q.waiting, e)
	}
	return out
}

// Len is the number waiting.
func (q *Queue) Len() int { return len(q.order) }

// ReadServers lists the running servers from vesper-microvm's records: <dir>/<id>.args holds the share mount, the
// squash flag and one source per line.
func ReadServers(dir string) ([]Server, error) {
	names, err := filepath.Glob(filepath.Join(dir, "*.args"))
	if err != nil {
		return nil, err
	}
	var out []Server
	for _, n := range names {
		raw, err := os.ReadFile(n)
		if err != nil {
			continue
		}
		lines := strings.Split(strings.TrimRight(string(raw), "\n"), "\n")
		if len(lines) < 3 {
			continue
		}
		s := Server{ID: strings.TrimSuffix(filepath.Base(n), ".args"), Mount: lines[0]}
		for _, l := range lines[2:] {
			if src, err := unfs.ParseSource(l); err == nil {
				s.Sources = append(s.Sources, src)
			}
		}
		out = append(out, s)
	}
	sort.Slice(out, func(a, b int) bool { return out[a].ID < out[b].ID })
	return out, nil
}

// Stats counts what one microVM's queue did.
type Stats struct {
	Queued    int `json:"queued"`
	Sent      int `json:"sent"`
	Coalesced int `json:"coalesced"`
	Dropped   int `json:"dropped"`
	Waiting   int `json:"waiting"`
	Failures  int `json:"connect_failures"`
}

// Fanout reads host paths and keeps one queue and one sender per microVM.
type Fanout struct {
	Dir      string                            // vesper-microvm's unfs state directory
	Dial     func(id string) (net.Conn, error) // connects to a guest's EventPort
	StatPath string                            // where the counters are written (JSON), or ""
	Batch    int
	Refresh  time.Duration
	Idle     time.Duration // how long an idle sender waits before it checks that its microVM still exists

	statMu sync.Mutex // one writer of StatPath at a time

	mu      sync.Mutex
	servers []Server
	read    time.Time
	queues  map[string]*Queue
	stats   map[string]*Stats
	wake    map[string]chan struct{}
}

func (f *Fanout) init() {
	if f.queues == nil {
		f.queues, f.stats, f.wake = map[string]*Queue{}, map[string]*Stats{}, map[string]chan struct{}{}
	}
	if f.Batch == 0 {
		f.Batch = 256
	}
	if f.Refresh == 0 {
		f.Refresh = time.Second
	}
	if f.Idle == 0 {
		f.Idle = 5 * time.Second
	}
}

// Offer queues a host path for every microVM that exports it.
func (f *Fanout) Offer(p string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.init()
	if time.Since(f.read) > f.Refresh {
		if s, err := ReadServers(f.Dir); err == nil {
			f.servers = s
		}
		f.read = time.Now()
	}
	for _, s := range f.servers {
		evs := s.Map(p)
		if len(evs) == 0 {
			continue
		}
		q, ok := f.queues[s.ID]
		if !ok {
			q = &Queue{}
			f.queues[s.ID], f.stats[s.ID], f.wake[s.ID] = q, &Stats{}, make(chan struct{}, 1)
			go f.send(s.ID)
		}
		st := f.stats[s.ID]
		for _, e := range evs {
			c, d := q.Coalesced, q.Dropped
			q.Add(e)
			st.Queued++
			st.Coalesced += q.Coalesced - c
			st.Dropped += q.Dropped - d
		}
		st.Waiting = q.Len()
		select {
		case f.wake[s.ID] <- struct{}{}:
		default:
		}
	}
}

func (f *Fanout) alive(id string) bool {
	_, err := os.Stat(filepath.Join(f.Dir, id+".args"))
	return err == nil
}

// send drains one microVM's queue over one connection, reconnecting with a delay; it ends when the server's record
// is gone (the microVM stopped).
func (f *Fanout) send(id string) {
	var conn net.Conn
	defer func() {
		if conn != nil {
			conn.Close()
		}
	}()
	for {
		f.mu.Lock()
		wake := f.wake[id]
		n := f.queues[id].Len()
		f.mu.Unlock()
		if n == 0 {
			select {
			case <-wake:
			case <-time.After(f.Idle):
				if !f.alive(id) {
					f.forget(id)
					return
				}
			}
			continue
		}
		if conn == nil {
			c, err := f.Dial(id)
			if err != nil {
				f.mu.Lock()
				f.stats[id].Failures++
				f.mu.Unlock()
				if !f.alive(id) {
					f.forget(id)
					return
				}
				time.Sleep(time.Second)
				continue
			}
			conn = c
		}
		f.mu.Lock()
		batch := f.queues[id].Take(f.Batch)
		f.mu.Unlock()
		var b strings.Builder
		for _, e := range batch {
			b.WriteString(e.Line())
		}
		_ = conn.SetWriteDeadline(time.Now().Add(5 * time.Second))
		if _, err := io.WriteString(conn, b.String()); err != nil {
			conn.Close()
			conn = nil
			f.mu.Lock()
			q, st := f.queues[id], f.stats[id]
			c, d := q.Coalesced, q.Dropped
			for _, e := range batch { // put them back; the queue bound still holds
				q.Add(e)
			}
			st.Coalesced += q.Coalesced - c // a path offered again meanwhile, or one the bound drops, is counted
			st.Dropped += q.Dropped - d
			st.Waiting = q.Len()
			st.Failures++
			f.mu.Unlock()
			time.Sleep(time.Second)
			continue
		}
		f.mu.Lock()
		f.stats[id].Sent += len(batch)
		f.stats[id].Waiting = f.queues[id].Len()
		f.mu.Unlock()
		f.writeStats()
	}
}

func (f *Fanout) forget(id string) {
	f.mu.Lock()
	delete(f.queues, id)
	delete(f.stats, id)
	delete(f.wake, id)
	f.mu.Unlock()
	f.writeStats()
}

// Snapshot returns the counters per microVM.
func (f *Fanout) Snapshot() map[string]Stats {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := map[string]Stats{}
	for id, s := range f.stats {
		out[id] = *s
	}
	return out
}

func (f *Fanout) writeStats() {
	if f.StatPath == "" {
		return
	}
	f.statMu.Lock()
	defer f.statMu.Unlock()
	b, _ := json.Marshal(f.Snapshot())
	tmp := f.StatPath + ".tmp"
	if os.WriteFile(tmp, append(b, '\n'), 0o600) == nil {
		_ = os.Rename(tmp, f.StatPath)
	}
}

// Run reads one host path per line until r ends.
func (f *Fanout) Run(r io.Reader) error {
	sc := bufio.NewScanner(r)
	sc.Buffer(make([]byte, 64*1024), 64*1024)
	for sc.Scan() {
		if p := sc.Text(); strings.HasPrefix(p, "/") {
			f.Offer(p)
		}
		f.writeStats()
	}
	return sc.Err()
}

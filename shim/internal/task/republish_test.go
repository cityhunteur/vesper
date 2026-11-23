package task

import (
	"context"
	"sync"
	"testing"
	"time"

	eventstypes "github.com/containerd/containerd/api/events"
)

// countingPublisher counts the events published on each topic.
type countingPublisher struct {
	mu sync.Mutex
	n  map[string]int
}

func (p *countingPublisher) Publish(_ context.Context, topic string, _ eventMessage) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.n[topic]++
	return nil
}
func (p *countingPublisher) Close() error { return nil }
func (p *countingPublisher) count(topic string) int {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.n[topic]
}

// TD-098: an exit nobody acted on (no Delete) is published again; once the task is deleted, it is not.
func TestExitIsRepublishedUntilDelete(t *testing.T) {
	oldI, oldN := ExitRepublishInterval, ExitRepublishTimes
	ExitRepublishInterval, ExitRepublishTimes = 20*time.Millisecond, 3
	defer func() { ExitRepublishInterval, ExitRepublishTimes = oldI, oldN }()
	pub := &countingPublisher{n: map[string]int{}}
	s, err := NewService(context.Background(), pub, nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	exit := &eventstypes.TaskExit{ContainerID: "a", ID: "a", ExitStatus: 7}

	stale := newTask("a", t.TempDir())
	stale.MarkExited(7, time.Now())
	s.republishExit(context.Background(), stale, exit)
	if got := pub.count("/tasks/exit"); got != 3 {
		t.Errorf("an undeleted task's exit was published %d more times, want 3", got)
	}

	deleted := newTask("b", t.TempDir())
	deleted.MarkExited(0, time.Now())
	if err := deleted.MarkDeleted(); err != nil {
		t.Fatal(err)
	}
	before := pub.count("/tasks/exit")
	s.republishExit(context.Background(), deleted, exit)
	if got := pub.count("/tasks/exit") - before; got != 0 {
		t.Errorf("a deleted task's exit was published %d more times", got)
	}
}

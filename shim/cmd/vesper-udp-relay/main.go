// vesper-udp-relay is the guest side of a UDP published port (TD-037, D-077). vesper-port-relay-conn runs it for a
// bridge connection whose preface is `UDP <port>`: stdin and stdout are that connection, carrying udpframe frames.
// Each flow id gets one connected UDP socket to 127.0.0.1:<port>, where dockerd's own mapping reaches the
// container; its replies go back framed with the same id. A relay holds at most MaxFlows flows and closes a flow
// after IdleClose without a datagram, telling the host with a Close frame.
package main

import (
	"bufio"
	"errors"
	"fmt"
	"net"
	"os"
	"strconv"
	"sync"
	"syscall"
	"time"

	"github.com/cityhunteur/vesper/shim/internal/udpframe"
)

const (
	MaxFlows  = 1280              // the host holds 1,024; the rest is room for flows whose close is still on its way
	IdleClose = 120 * time.Second // a little after the host's 90 s, so the host normally closes first
)

type flow struct {
	conn *net.UDPConn
	last time.Time
}

type relay struct {
	target *net.UDPAddr
	mu     sync.Mutex
	flows  map[uint32]*flow
	wmu    sync.Mutex
	out    *bufio.Writer
}

func (r *relay) send(kind byte, id uint32, payload []byte) {
	r.wmu.Lock()
	defer r.wmu.Unlock()
	if udpframe.Write(r.out, kind, id, payload) == nil {
		_ = r.out.Flush()
	}
}

func (r *relay) replies(id uint32, f *flow) {
	buf := make([]byte, udpframe.MaxPayload)
	for {
		n, err := f.conn.Read(buf)
		if err != nil {
			// A connected UDP socket reports an ICMP port-unreachable as ECONNREFUSED; the flow lives on.
			if errors.Is(err, syscall.ECONNREFUSED) {
				continue
			}
			return
		}
		r.mu.Lock()
		f.last = time.Now()
		r.mu.Unlock()
		r.send(udpframe.Data, id, buf[:n])
	}
}

func (r *relay) drop(id uint32, tell bool) {
	r.mu.Lock()
	f := r.flows[id]
	delete(r.flows, id)
	r.mu.Unlock()
	if f != nil {
		_ = f.conn.Close()
		if tell {
			r.send(udpframe.Close, id, nil)
		}
	}
}

func (r *relay) janitor() {
	for range time.Tick(10 * time.Second) {
		var idle []uint32
		r.mu.Lock()
		for id, f := range r.flows {
			if time.Since(f.last) > IdleClose {
				idle = append(idle, id)
			}
		}
		r.mu.Unlock()
		for _, id := range idle {
			r.drop(id, true)
		}
	}
}

func main() {
	if len(os.Args) != 2 {
		fmt.Fprintln(os.Stderr, "usage: vesper-udp-relay PORT")
		os.Exit(2)
	}
	port, err := strconv.Atoi(os.Args[1])
	if err != nil || port < 1 || port > 65535 {
		fmt.Fprintln(os.Stderr, "vesper-udp-relay: bad port")
		os.Exit(2)
	}
	r := &relay{target: &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1), Port: port}, flows: map[uint32]*flow{}, out: bufio.NewWriterSize(os.Stdout, 1<<17)}
	go r.janitor()
	in := bufio.NewReaderSize(os.Stdin, 1<<17)
	for {
		kind, id, payload, err := udpframe.Read(in)
		if err != nil {
			return // the host closed the stream, or sent a bad frame
		}
		if kind == udpframe.Close {
			r.drop(id, false)
			continue
		}
		r.mu.Lock()
		f := r.flows[id]
		if f == nil && len(r.flows) < MaxFlows {
			if conn, err := net.DialUDP("udp4", nil, r.target); err == nil {
				f = &flow{conn: conn}
				r.flows[id] = f
				go r.replies(id, f)
			}
		}
		if f != nil {
			f.last = time.Now()
		}
		r.mu.Unlock()
		if f == nil {
			continue // over the cap or no socket: the datagram is lost, as UDP allows
		}
		_, _ = f.conn.Write(payload)
	}
}

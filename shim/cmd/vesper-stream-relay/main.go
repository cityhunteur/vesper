// vesper-stream-relay is the guest side of one bridge stream (TD-093): vesper-docker-relay-conn runs it for the
// Docker socket and vesper-port-relay-conn for a published TCP port. stdin and stdout are the host's connection
// (socat EXEC gives each its own socketpair). Usage: vesper-stream-relay unix PATH | tcp HOST:PORT. It exits when
// the target ends the stream, after passing that end to the host, so a server that closes first is seen as closed.
package main

import (
	"fmt"
	"os"

	"github.com/cityhunteur/vesper/shim/internal/streamrelay"
)

func main() {
	if len(os.Args) != 3 {
		fmt.Fprintln(os.Stderr, "usage: vesper-stream-relay unix PATH | tcp HOST:PORT")
		os.Exit(2)
	}
	target, err := streamrelay.Dial(os.Args[1], os.Args[2])
	if err != nil {
		fmt.Fprintf(os.Stderr, "vesper-stream-relay: %s %s: %v\n", os.Args[1], os.Args[2], err)
		os.Exit(1)
	}
	if err := streamrelay.Relay(os.Stdin, os.Stdout, target); err != nil {
		os.Exit(1)
	}
}

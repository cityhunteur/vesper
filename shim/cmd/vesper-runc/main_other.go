//go:build !linux

// vesper-runc wraps runc in the System VM; it has nothing to do elsewhere.
package main

import (
	"fmt"
	"os"
)

func main() {
	fmt.Fprintln(os.Stderr, "vesper-runc: runs in the System VM only")
	os.Exit(2)
}

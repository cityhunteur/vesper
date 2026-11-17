//go:build !linux

package shares

// ShareMtime reads no mtime outside Linux: the fanout runs in the System VM.
func ShareMtime(mount, rel string) int64 { return 0 }

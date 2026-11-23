package main

import (
	"os"
	"path/filepath"
	"syscall"
)

// fuseSuperMagic is the filesystem type of a virtio-fs mount: the System VM's shared macOS directories.
const fuseSuperMagic = 0x65735546

// onShare is authz.OnShare for the System VM: path, or its deepest existing ancestor, is on a virtio-fs mount.
func onShare(path string) bool {
	for dir := filepath.Clean(path); ; dir = filepath.Dir(dir) {
		var st syscall.Statfs_t
		if err := syscall.Statfs(dir, &st); err == nil {
			return int64(st.Type) == fuseSuperMagic
		}
		if dir == "/" {
			return false
		}
	}
}

// missingSourceOK is authz.MissingSourceOK for the System VM: whether path exists, and if it does not, whether its
// deepest existing ancestor is on a shared macOS directory, where dockerd may create it.
func missingSourceOK(path string) (exists, creatable bool) {
	if _, err := os.Lstat(path); err == nil {
		return true, false
	}
	for dir := filepath.Dir(path); ; dir = filepath.Dir(dir) {
		var st syscall.Statfs_t
		if err := syscall.Statfs(dir, &st); err == nil {
			return false, int64(st.Type) == fuseSuperMagic
		}
		if dir == "/" {
			return false, false
		}
	}
}

//go:build !linux

package main

// The plugin runs in the System VM only; elsewhere every missing bind source is refused.
func missingSourceOK(string) (exists, creatable bool) { return false, false }

// The plugin runs in the System VM only; elsewhere no path is on a share.
func onShare(string) bool { return false }

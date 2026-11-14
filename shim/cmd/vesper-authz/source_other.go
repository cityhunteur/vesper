//go:build !linux

package main

// The plugin runs in the System VM only; elsewhere every missing bind source is refused.
func missingSourceOK(string) (exists, creatable bool) { return false, false }

//go:build !linux

package backend

import (
	"context"
	"errors"

	specs "github.com/opencontainers/runtime-spec/specs-go"

	"github.com/cityhunteur/vesper/shim/internal/bundle"
)

var errLinuxOnly = errors.New("the vesper runtime needs Linux")

func DataDiskInUse() bool { return false }

func ReadDockerFiles([]bundle.DockerFile) (map[string][]byte, error) { return nil, errLinuxOnly }

func (m *MicroVM) PrepareDocker(context.Context, string, string, []specs.LinuxIDMapping, []specs.LinuxIDMapping) (string, bool, error) {
	return "", false, errLinuxOnly
}

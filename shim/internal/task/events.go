package task

import "github.com/containerd/containerd/v2/core/events"

// eventMessage is what the shim publisher accepts (a typeurl-registered protobuf message).
type eventMessage = events.Event

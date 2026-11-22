// Package udpframe is the frame format that carries UDP datagrams over the vsock bridge's byte stream (TD-037,
// D-077): kind (1 byte), flow id (4 bytes, big-endian), length (2 bytes, big-endian), then the payload. The frame
// carries no address: the guest relay sends every flow to the one port named in its preface. The host side is
// src/ports/udp.rs; the two must agree.
package udpframe

import (
	"encoding/binary"
	"fmt"
	"io"
)

const (
	// Data carries one datagram of the flow.
	Data byte = 0
	// Close ends the flow: its socket is closed and its id may be used again.
	Close byte = 1
	// MaxPayload is the largest UDP payload over IPv4.
	MaxPayload = 65507
	headerLen  = 7
)

// Write sends one frame. A Close frame has no payload.
func Write(w io.Writer, kind byte, flow uint32, payload []byte) error {
	if len(payload) > MaxPayload || (kind == Close && len(payload) != 0) || (kind != Data && kind != Close) {
		return fmt.Errorf("udpframe: bad frame kind %d with %d bytes", kind, len(payload))
	}
	buf := make([]byte, headerLen+len(payload))
	buf[0] = kind
	binary.BigEndian.PutUint32(buf[1:5], flow)
	binary.BigEndian.PutUint16(buf[5:7], uint16(len(payload)))
	copy(buf[headerLen:], payload)
	_, err := w.Write(buf)
	return err
}

// Read reads one frame. An unknown kind, a Close with a payload or a length over MaxPayload is an error: the stream
// is then closed, never resynchronised.
func Read(r io.Reader) (kind byte, flow uint32, payload []byte, err error) {
	var head [headerLen]byte
	if _, err = io.ReadFull(r, head[:]); err != nil {
		return 0, 0, nil, err
	}
	kind, flow = head[0], binary.BigEndian.Uint32(head[1:5])
	n := int(binary.BigEndian.Uint16(head[5:7]))
	if (kind != Data && kind != Close) || n > MaxPayload || (kind == Close && n != 0) {
		return 0, 0, nil, fmt.Errorf("udpframe: bad frame kind %d length %d", kind, n)
	}
	payload = make([]byte, n)
	if _, err = io.ReadFull(r, payload); err != nil {
		return 0, 0, nil, io.ErrUnexpectedEOF
	}
	return kind, flow, payload, nil
}

package udpframe

import (
	"bytes"
	"errors"
	"io"
	"testing"
)

func TestRoundTrip(t *testing.T) {
	var b bytes.Buffer
	big := bytes.Repeat([]byte{7}, MaxPayload)
	for _, p := range [][]byte{{}, []byte("x"), big} {
		if err := Write(&b, Data, 42, p); err != nil {
			t.Fatal(err)
		}
	}
	if err := Write(&b, Close, 42, nil); err != nil {
		t.Fatal(err)
	}
	for _, want := range [][]byte{{}, []byte("x"), big} {
		k, f, p, err := Read(&b)
		if err != nil || k != Data || f != 42 || !bytes.Equal(p, want) {
			t.Fatalf("got %d %d %d bytes %v", k, f, len(p), err)
		}
	}
	if k, f, p, err := Read(&b); err != nil || k != Close || f != 42 || len(p) != 0 {
		t.Fatalf("close: %d %d %v %v", k, f, p, err)
	}
	if _, _, _, err := Read(&b); !errors.Is(err, io.EOF) {
		t.Fatalf("end: %v", err)
	}
}

func TestBadFramesAreErrors(t *testing.T) {
	if err := Write(io.Discard, Data, 1, make([]byte, MaxPayload+1)); err == nil {
		t.Fatal("an oversized payload was written")
	}
	for name, raw := range map[string][]byte{
		"unknown kind":     {9, 0, 0, 0, 1, 0, 0},
		"close with bytes": {1, 0, 0, 0, 1, 0, 1, 'x'},
		"over the limit":   {0, 0, 0, 0, 1, 0xff, 0xff},
		"truncated":        {0, 0, 0, 0, 1, 0, 4, 'a'},
	} {
		if _, _, _, err := Read(bytes.NewReader(raw)); err == nil {
			t.Fatalf("%s: read without error", name)
		}
	}
}

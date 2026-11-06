package backend

import (
	"encoding/base64"
	"testing"
)

func TestEncodeCommand(t *testing.T) {
	b64, err := EncodeCommand([]string{"sh", "-c", "echo it's ok"}, []string{"PATH=/bin", "BAD", "X=a b"}, "/work")
	if err != nil {
		t.Fatal(err)
	}
	raw, _ := base64.StdEncoding.DecodeString(b64)
	want := `cd '/work' || exit 126; export PATH='/bin'; export X='a b'; exec 'sh' '-c' 'echo it'\''s ok'`
	if string(raw) != want {
		t.Fatalf("got %q want %q", raw, want)
	}
	if _, err := EncodeCommand(nil, nil, "/"); err == nil {
		t.Fatal("expected an error for no args")
	}
	b64, _ = EncodeCommand([]string{"true"}, nil, "/")
	raw, _ = base64.StdEncoding.DecodeString(b64)
	if string(raw) != "exec 'true'" {
		t.Fatalf("root cwd must not emit cd: %q", raw)
	}
}

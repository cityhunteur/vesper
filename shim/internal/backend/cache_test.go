package backend

import (
	"os"
	"path/filepath"
	"testing"
)

func TestCacheHelpers(t *testing.T) {
	dir := t.TempDir()
	img := filepath.Join(dir, "k.ext4")
	os.WriteFile(img, []byte("image-bytes"), 0o600)
	if ok, _ := cacheValid(img); ok {
		t.Fatal("no digest file must not validate")
	}
	sum, err := fileSha256(img)
	if err != nil || len(sum) != 64 {
		t.Fatalf("digest: %v %q", err, sum)
	}
	os.WriteFile(img+".sha256", []byte(sum+"\n"), 0o644)
	if ok, _ := cacheValid(img); !ok {
		t.Fatal("matching digest must validate")
	}
	os.WriteFile(img, []byte("corrupted"), 0o600)
	if ok, _ := cacheValid(img); ok {
		t.Fatal("a corrupted image must be rejected")
	}
	dst := filepath.Join(dir, "task", "rootfs.ext4")
	os.MkdirAll(filepath.Dir(dst), 0o755)
	if err := copyFile(img, dst); err != nil {
		t.Fatal(err)
	}
	got, _ := os.ReadFile(dst)
	if string(got) != "corrupted" {
		t.Fatalf("copy mismatch: %q", got)
	}
	if _, err := os.Stat(dst + ".tmp"); !os.IsNotExist(err) {
		t.Fatal("temp file must be renamed away")
	}
	e := &PrepareError{Step: "build", Err: os.ErrNotExist, Hint: "install e2fsprogs"}
	if e.Error() != "rootfs: build: file does not exist (install e2fsprogs)" {
		t.Fatalf("message: %s", e.Error())
	}
}

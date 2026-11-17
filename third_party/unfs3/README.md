# unfs3 (patched)

The share server that runs in the System VM for each shim microVM (ADR 0009) is unfs3 0.10.0 with four Vesper
patches. Alpine's package (`unfs3-0.10.0-r2`, pinned in `docs/specs/systemvm-packages.lock.yaml`) stays installed
for its library closure (`libtirpc`), but its `/usr/sbin/unfsd` is not run. The overlay ships this build as
`/usr/libexec/vesper/unfsd` (owner decision D-044).

| Patch | Fixes |
|---|---|
| `patches/0001-create-unchecked-keeps-data.patch` | TD-068: an UNCHECKED CREATE of an existing file truncated it, so a guest's `>>` or `fopen(…, "a")` of a file it had not looked up wiped the file |
| `patches/0002-fh-decode-refuses-malformed-handles.patch` | TD-065: a malformed file handle stopped the server on an assertion; it is now refused (STALE) |
| `patches/0003-nanosecond-times.patch` | TD-066: times were set and reported to the whole second; they are now kept to the nanosecond |
| `patches/0004-fh-cache-keyed-by-handle-path.patch` | TD-072: the handle cache was keyed on device and inode, so a handle from one export could resolve to the path of the same inode in another export; after a read through a read-only export, writes through a read-write one were refused and lost. Entries are now keyed on the handle's path hashes too |
| `patches/alpine-implicit.patch`, `patches/alpine-lfs64.patch` | Alpine's own two patches for musl, from aports `3.22-stable/main/unfs3` |

## Build

`scripts/systemvm/build-unfs3.sh` builds `unfsd` in a digest-pinned `alpine:3.22` arm64 container: it checks the
release tarball and every input against `sources.sha256`, installs the pinned build packages in
`build-packages.txt`, applies the six patches and runs Alpine's `configure` (`build-in-container.sh`). The build is
reproducible: the result must equal `unfsd-aarch64` byte for byte (`unfsd-aarch64.sha256`). The script needs a Docker
daemon that runs linux/arm64: the System VM's (`make docker-up`) locally, a native arm64 runner in CI (job
`unfs3-reproducible` in `.github/workflows/systemvm-image.yml`).

After changing a patch, run `scripts/systemvm/build-unfs3.sh --update`, then refresh `sources.sha256` (the digest
of the tarball, each patch, `build-packages.txt` and `build-in-container.sh`) and commit the new binary and digests
together. `scripts/systemvm/build-overlay.sh` refuses a binary that does not match its digest.

## Licence

BSD-3-Clause; `LICENSE` is unfs3's own. The patches are under the same licence.

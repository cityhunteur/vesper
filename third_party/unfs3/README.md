# unfs3 (patched)

The share server that runs in the System VM for each shim microVM (ADR 0009) is unfs3 0.10.0 with six Vesper
patches. Alpine's package (`unfs3-0.10.0-r2`, pinned in `docs/specs/systemvm-packages.lock.yaml`) stays installed
for its library closure (`libtirpc`), but its `/usr/sbin/unfsd` is not run. The overlay ships this build as
`/usr/libexec/vesper/unfsd` (owner decision D-044).

| Patch | Fixes |
|---|---|
| `patches/0001-create-unchecked-keeps-data.patch` | TD-068: an UNCHECKED CREATE of an existing file truncated it, so a guest's `>>` or `fopen(…, "a")` of a file it had not looked up wiped the file |
| `patches/0002-fh-decode-refuses-malformed-handles.patch` | TD-065: a malformed file handle stopped the server on an assertion; it is now refused (STALE) |
| `patches/0003-nanosecond-times.patch` | TD-066: times were set and reported to the whole second; they are now kept to the nanosecond |
| `patches/0004-fh-cache-keyed-by-handle-path.patch` | TD-072: the handle cache was keyed on device and inode, so a handle from one export could resolve to the path of the same inode in another export; after a read through a read-only export, writes through a read-write one were refused and lost. Entries are now keyed on the handle's path hashes too |
| `patches/0005-svc-run-private-pollfd.patch` | TD-075: the main loop polled libtirpc's own descriptor array and let the library walk it. Taking a connection makes the library reallocate that array; with a second descriptor ready in the same round the walk read freed memory, and the server ended with a segmentation fault on about one task start in three (a mount connects to the NFS port and at once to the mount port). The loop now polls a private copy and gives each ready descriptor to `svc_getreq_common`, as libtirpc's own `svc_run` does |
| `patches/0006-protect-by-identity.patch` | TD-073, TD-076: a read-only export inside a read-write one was kept read-only by a mount on one name, and a client reached the object under other names (another letter case on a volume that ignores case, a hard link, a symbolic link, a directory moved into another export). The server now reads `<exports file>.protect` (`p PATH`, a place whose objects it remembers (768 in all, for all places together, newest by when each was last told or found, TD-079; a list that is read again from the start does not tell its old lines again), and which it looks at before every changing request, at every request and on every turn of its main loop; `i DEV INO`, an object by number; `s DEV INO`, an object that is about to be put at a place, added by the supervisor while the server runs: the server reads what was added to the list before it judges a request, TD-078; `a DEV INO`, an anchor, a directory on the way to a read-only export inside a read-write one: RENAME (from and onto), RMDIR and REMOVE of that directory itself are refused, so that a client cannot change what the read-only export's path names, TD-080) and refuses a request that changes a listed object, or anything under one, by device and inode number. The check resolves the directory part of the path through every symbolic link (`realpath`) and, for a call that acts on a link's target, follows the last name by the link's own text (without trailing slashes), so a target that is not there yet is judged by its directory. `exports_rw` checks the request's path, CREATE, REMOVE, RMDIR and RENAME check the named object too, and LINK checks its source. A list that cannot be read, or that has a line of no known kind or more `p` or `i` lines than the server has room for, refuses every change (of the `s` and `a` lines the server keeps the newest). The comment above `PROT_FIXED` in `Config/exports.y` is the definition of the list; `shim/internal/unfs/protect.go` writes it. Without the file nothing changes |
| `patches/alpine-implicit.patch`, `patches/alpine-lfs64.patch` | Alpine's own two patches for musl, from aports `3.22-stable/main/unfs3` |

## Build

`scripts/systemvm/build-unfs3.sh` builds `unfsd` in a digest-pinned `alpine:3.22` arm64 container: it checks the
release tarball and every input against `sources.sha256`, installs the pinned build packages in
`build-packages.txt`, applies the eight patches and runs Alpine's `configure` (`build-in-container.sh`). The build is
reproducible: the result must equal `unfsd-aarch64` byte for byte (`unfsd-aarch64.sha256`). The script needs a Docker
daemon that runs linux/arm64: the System VM's (`make docker-up`) locally, a native arm64 runner in CI (job
`unfs3-reproducible` in `.github/workflows/systemvm-image.yml`).

After changing a patch, run `scripts/systemvm/build-unfs3.sh --update`, then refresh `sources.sha256` (the digest
of the tarball, each patch, `build-packages.txt` and `build-in-container.sh`) and commit the new binary and digests
together. `scripts/systemvm/build-overlay.sh` refuses a binary that does not match its digest.

## Licence

BSD-3-Clause; `LICENSE` is unfs3's own. The patches are under the same licence.

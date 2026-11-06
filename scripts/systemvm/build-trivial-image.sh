#!/usr/bin/env bash
# Build a minimal, offline container image tarball from the pinned boot ISO
# (busybox + musl from its apks/) for `docker import`, so "run a trivial
# container" needs no registry. Output: build/systemvm/trivial-rootfs.tar
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
IMAGE_DIR="${ROOT_DIR}/build/systemvm"
WORK="${IMAGE_DIR}/trivial-rootfs"
OUT="${IMAGE_DIR}/trivial-rootfs.tar"

cd "${ROOT_DIR}"
eval "$(cargo run -q --bin systemvmctl -- lock-env)"
ISO="${IMAGE_DIR}/${VESPER_IMAGE_BOOT_MEDIA_FILE}"
[ -f "${ISO}" ] || { echo "build-trivial-image: ${ISO} missing; run 'make systemvm-fetch'" >&2; exit 2; }

if command -v bsdtar >/dev/null 2>&1; then TAR=bsdtar; else TAR=tar; fi
rm -rf "${WORK}" && mkdir -p "${WORK}/apks" "${WORK}/rootfs"
"${TAR}" -xf "${ISO}" -C "${WORK}/apks" --strip-components 2 'apks/aarch64/busybox-1*.apk' 'apks/aarch64/musl-1*.apk'
for apk in "${WORK}"/apks/*.apk; do
  # An apk is a gzip tar; the leading .SIGN/.PKGINFO members are skipped.
  tar -xzf "${apk}" -C "${WORK}/rootfs" --exclude '.SIGN*' --exclude '.PKGINFO' --exclude '.pre-*' --exclude '.post-*' --exclude '.trigger' 2>/dev/null || true
done
[ -x "${WORK}/rootfs/bin/busybox" ] || { echo "build-trivial-image: busybox missing from the extracted packages" >&2; exit 1; }
ln -sf busybox "${WORK}/rootfs/bin/sh"
ln -sf busybox "${WORK}/rootfs/bin/echo"
# Deterministic tar: sorted, root owner, epoch mtime.
python3 -I - "${WORK}/rootfs" "${OUT}" <<'PY'
import os, sys, tarfile, stat
root, out = sys.argv[1], sys.argv[2]
entries = []
for dirpath, dirs, files in os.walk(root):
    dirs.sort()
    for name in sorted(dirs + files):
        full = os.path.join(dirpath, name)
        entries.append((os.path.relpath(full, root), full))
with tarfile.open(out, "w", format=tarfile.USTAR_FORMAT) as tar:
    for arcname, full in sorted(entries):
        st = os.lstat(full)
        info = tarfile.TarInfo(arcname)
        info.uid = info.gid = 0; info.uname = info.gname = "root"; info.mtime = 0
        if stat.S_ISLNK(st.st_mode):
            info.type = tarfile.SYMTYPE; info.linkname = os.readlink(full); info.mode = 0o777; tar.addfile(info)
        elif stat.S_ISDIR(st.st_mode):
            info.type = tarfile.DIRTYPE; info.mode = 0o755; tar.addfile(info)
        else:
            info.type = tarfile.REGTYPE; info.mode = 0o755 if st.st_mode & 0o111 else 0o644; info.size = st.st_size
            with open(full, "rb") as fh: tar.addfile(info, fh)
print(f"wrote {out} ({os.path.getsize(out)} bytes)")
PY

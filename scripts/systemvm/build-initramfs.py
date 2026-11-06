#!/usr/bin/env python3
"""Build the System VM overlay and the derived initramfs.

Usage: build-initramfs.py --overlay-dir systemvm/overlay --apks-dir build/systemvm/cache/apks
                          --apk FILE [--apk FILE ...] --base-initrd build/systemvm/initramfs-virt
                          --apkovl-path /vesper/localhost.apkovl.tar.gz
                          --out-apkovl build/systemvm/localhost.apkovl.tar.gz
                          --out-initrd build/systemvm/initramfs-vesper

The overlay tar holds the overlay tree plus var/lib/vesper/apks/<FILE> for every
--apk, with owner root, mtime 0 and sorted entries. It is placed at --apkovl-path in a
newc cpio archive (gzip, mtime 0) appended to the pinned initramfs; the kernel unpacks
concatenated archives in order. Standard library only; deterministic for identical inputs.
"""
import argparse
import gzip
import hashlib
import io
import os
import stat
import sys
import tarfile


def add_tree(tar, root, prefix=""):
    entries = []
    for dirpath, dirs, files in os.walk(root):
        dirs.sort()
        rel = os.path.relpath(dirpath, root)
        if rel != ".":
            entries.append((os.path.join(prefix, rel), dirpath))
        for name in sorted(files):
            if name == "README.md" and dirpath == root:
                continue  # repository documentation, not guest content
            entries.append((os.path.join(prefix, rel, name) if rel != "." else os.path.join(prefix, name), os.path.join(dirpath, name)))
    for arcname, path in sorted(entries):
        add_path(tar, arcname.lstrip("./"), path)


def add_path(tar, arcname, path):
    st = os.lstat(path)
    info = tarfile.TarInfo(arcname)
    info.uid = info.gid = 0
    info.uname = info.gname = "root"
    info.mtime = 0
    if stat.S_ISLNK(st.st_mode):
        info.type = tarfile.SYMTYPE
        info.linkname = os.readlink(path)
        info.mode = 0o777
        tar.addfile(info)
    elif stat.S_ISDIR(st.st_mode):
        info.type = tarfile.DIRTYPE
        info.mode = 0o755
        tar.addfile(info)
    else:
        info.type = tarfile.REGTYPE
        info.mode = 0o755 if st.st_mode & 0o111 else 0o644
        info.size = st.st_size
        with open(path, "rb") as handle:
            tar.addfile(info, handle)


def build_apkovl(overlay_dir, apks, out_path):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w", format=tarfile.USTAR_FORMAT) as tar:
        add_tree(tar, overlay_dir)
        if apks:
            for part in ("var", "var/lib", "var/lib/vesper", "var/lib/vesper/apks"):
                info = tarfile.TarInfo(part)
                info.type = tarfile.DIRTYPE
                info.mode = 0o755
                info.mtime = 0
                info.uname = info.gname = "root"
                tar.addfile(info)
            for path in sorted(apks, key=os.path.basename):
                add_path(tar, "var/lib/vesper/apks/" + os.path.basename(path), path)
    with open(out_path, "wb") as handle:
        with gzip.GzipFile(fileobj=handle, mode="wb", mtime=0) as gz:
            gz.write(buffer.getvalue())


def newc_entry(name, mode, data, ino):
    fields = [ino, mode, 0, 0, 1, 0, len(data), 0, 0, 0, 0, len(name) + 1, 0]
    header = b"070701" + "".join(f"{value:08x}" for value in fields).encode()
    body = header + name.encode() + b"\0"
    body += b"\0" * ((4 - len(body) % 4) % 4)
    body += data + b"\0" * ((4 - len(data) % 4) % 4)
    return body


def build_initrd(base_initrd, apkovl_path, apkovl_bytes, out_path):
    archive = io.BytesIO()
    parts = apkovl_path.strip("/").split("/")
    ino = 1
    for depth in range(1, len(parts)):
        archive.write(newc_entry("/".join(parts[:depth]), 0o040755, b"", ino))
        ino += 1
    archive.write(newc_entry("/".join(parts), 0o100644, apkovl_bytes, ino))
    archive.write(newc_entry("TRAILER!!!", 0, b"", 0))
    extra = io.BytesIO()
    with gzip.GzipFile(fileobj=extra, mode="wb", mtime=0) as gz:
        gz.write(archive.getvalue())
    with open(base_initrd, "rb") as base, open(out_path, "wb") as out:
        out.write(base.read())
        out.write(extra.getvalue())


def sha256_of(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--overlay-dir", required=True)
    parser.add_argument("--apk", action="append", default=[])
    parser.add_argument("--base-initrd", required=True)
    parser.add_argument("--apkovl-path", required=True)
    parser.add_argument("--out-apkovl", required=True)
    parser.add_argument("--out-initrd", required=True)
    args = parser.parse_args()
    if not os.path.isdir(args.overlay_dir):
        sys.exit(f"overlay dir {args.overlay_dir} is missing")
    for apk in args.apk:
        if not os.path.isfile(apk):
            sys.exit(f"package {apk} is missing; run make systemvm-overlay (fetch-packages.sh)")
    build_apkovl(args.overlay_dir, args.apk, args.out_apkovl)
    with open(args.out_apkovl, "rb") as handle:
        apkovl = handle.read()
    build_initrd(args.base_initrd, args.apkovl_path, apkovl, args.out_initrd)
    print(f"apkovl {args.out_apkovl} sha256={sha256_of(args.out_apkovl)} size={len(apkovl)}")
    print(f"initrd {args.out_initrd} sha256={sha256_of(args.out_initrd)} size={os.path.getsize(args.out_initrd)}")


if __name__ == "__main__":
    main()

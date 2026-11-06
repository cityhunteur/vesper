#!/usr/bin/env python3
"""Unpack a Linux EFI zboot kernel (MZ..zimg header, gzip payload) to the raw ARM64 Image.

Usage: unpack-zboot.py INPUT OUTPUT
Exit 1 when the input is not a gzip zboot image or the result lacks the ARM64 Image magic.
Standard library only; run with -I by the fetch script.
"""
import gzip
import struct
import sys


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: unpack-zboot.py INPUT OUTPUT", file=sys.stderr)
        return 2
    src, dst = sys.argv[1], sys.argv[2]
    with open(src, "rb") as handle:
        data = handle.read()
    if data[:2] != b"MZ" or data[4:8] != b"zimg":
        print(f"{src}: not a Linux EFI zboot image (missing MZ/zimg header)", file=sys.stderr)
        return 1
    payload_offset, payload_size = struct.unpack_from("<II", data, 8)
    compression = data[24:32].rstrip(b"\0")
    if compression != b"gzip":
        print(f"{src}: unsupported zboot compression {compression!r}", file=sys.stderr)
        return 1
    payload = data[payload_offset : payload_offset + payload_size]
    if len(payload) != payload_size:
        print(f"{src}: truncated payload", file=sys.stderr)
        return 1
    image = gzip.decompress(payload)
    if image[56:60] != b"ARM\x64":
        print(f"{src}: unpacked payload is not an ARM64 Linux Image", file=sys.stderr)
        return 1
    with open(dst, "wb") as handle:
        handle.write(image)
    print(f"unpacked {src} -> {dst} ({len(image)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())

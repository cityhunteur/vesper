#!/bin/sh
# Runs inside the pinned Alpine container (scripts/systemvm/build-unfs3.sh): builds unfsd from the pinned unfs3
# release with Alpine's two patches and Vesper's five, the way Alpine's APKBUILD does, and writes /out/unfsd.
# Inputs in /in: the source tarball and patches/. Every input is checked against unfs3.lock.yaml's digests first.
set -eu
mkdir -p /out
cd /in
sha256sum -c sources.sha256
apk add --no-cache -q $(cat build-packages.txt)
export SOURCE_DATE_EPOCH=1700000000
mkdir -p /build && tar -xzf unfs3-0.10.0.tar.gz -C /build
cd /build/unfs3-0.10.0
for p in alpine-implicit alpine-lfs64 0001-create-unchecked-keeps-data 0002-fh-decode-refuses-malformed-handles 0003-nanosecond-times 0004-fh-cache-keyed-by-handle-path 0005-svc-run-private-pollfd; do
  patch -p1 -s < "/in/patches/$p.patch"
done
# The same configure as Alpine's APKBUILD; paths in the binary are made independent of the build directory.
CFLAGS="-Os -fstack-clash-protection -Wformat -Werror=format-security -ffile-prefix-map=/build/unfs3-0.10.0=." \
  ./configure --build=aarch64-alpine-linux-musl --host=aarch64-alpine-linux-musl --prefix=/usr --mandir=/usr/share/man >/dev/null
make -s >/dev/null
strip --strip-all -o /out/unfsd unfsd
sha256sum /out/unfsd

#!/usr/bin/env bash
# Builds the System VM's patched unfsd (TD-065, TD-066, TD-068; D-044) in a digest-pinned Alpine 3.22 arm64
# container, from the pinned unfs3 0.10.0 release, Alpine's two patches and Vesper's five in third_party/unfs3, and
# compares the result with the binary the overlay ships (third_party/unfs3/unfsd-aarch64).
#
#   build-unfs3.sh            build and check: exit 1 if the build differs from the shipped binary
#   build-unfs3.sh --update   build and replace the shipped binary and its digest (after a patch change)
#
# Needs a Docker daemon that runs linux/arm64: the System VM's (make docker-up) or an arm64 Linux host (CI).
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="${ROOT_DIR}/third_party/unfs3"
OUT="${ROOT_DIR}/build/unfs3"
IMAGE="alpine@sha256:49340b346798875b5592e5d420241f2f9cfba5f47e62ebb1446a1b34734e898f" # alpine:3.22, 3.22.8
URL="https://github.com/unfs3/unfs3/releases/download/unfs3-0.10.0/unfs3-0.10.0.tar.gz"
mkdir -p "${OUT}"
tarball="${OUT}/unfs3-0.10.0.tar.gz"
want="$(awk '$2 == "unfs3-0.10.0.tar.gz" {print $1}' "${SRC}/sources.sha256")"
if [ ! -f "${tarball}" ] || [ "$(shasum -a 256 "${tarball}" | cut -d' ' -f1)" != "${want}" ]; then
  curl -fsSL -o "${tarball}.part" "${URL}" && mv "${tarball}.part" "${tarball}"
fi
[ "$(shasum -a 256 "${tarball}" | cut -d' ' -f1)" = "${want}" ] || { echo "build-unfs3: ${URL} does not match its pinned digest" >&2; exit 1; }
cid="$(docker create --platform linux/arm64 "${IMAGE}" /in/build-in-container.sh)"
trap 'docker rm -f "${cid}" >/dev/null 2>&1 || true' EXIT
docker cp "${SRC}/." "${cid}:/in" >/dev/null
docker cp "${tarball}" "${cid}:/in/unfs3-0.10.0.tar.gz" >/dev/null
docker start -a "${cid}"
docker cp "${cid}:/out/unfsd" "${OUT}/unfsd" >/dev/null
got="$(shasum -a 256 "${OUT}/unfsd" | cut -d' ' -f1)"
pinned="$(cut -d' ' -f1 "${SRC}/unfsd-aarch64.sha256" 2>/dev/null || true)"
if [ "${1:-}" = --update ]; then
  cp "${OUT}/unfsd" "${SRC}/unfsd-aarch64"
  echo "${got}  unfsd-aarch64" > "${SRC}/unfsd-aarch64.sha256"
  echo "build-unfs3: updated third_party/unfs3/unfsd-aarch64 (${got})"
elif [ "${got}" = "${pinned}" ]; then
  echo "build-unfs3: the build matches the shipped binary (${got})"
else
  echo "build-unfs3: the build (${got}) differs from the shipped binary (${pinned:-none})" >&2; exit 1
fi

#!/usr/bin/env bash
# GH #39 checklist from macOS: binfmt_misc + qemu-user registered in the System VM (fix-binary flag), the
# execution mode surfaced per architecture, `docker run --platform linux/amd64` working through the ingress,
# architecture metadata in image and container inspect, a clear error for an unsupported architecture, and
# the emulated-vs-native cost of a fixed shell workload. Writes run/systemvm/crossarch-report.json.
# Needs `make systemvm-up && make docker-up` and outbound network for the test images.
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOCKET="${DOCKER_SOCKET:-${ROOT_DIR}/run/vesper/docker.sock}"
REPORT="${ROOT_DIR}/run/systemvm/crossarch-report.json"
NATIVE_IMAGE="${XA_IMAGE:-alpine:3.22}"
AMD64_IMAGE="${XA_AMD64_IMAGE:-busybox:1.37}"
RATIO_LIMIT="${XA_RATIO_LIMIT:-40}"
DOCKER="${DOCKER_BIN:-docker}"
CTL=(cargo run -q --bin systemvmctl -- control)
export DOCKER_HOST="unix://${SOCKET}"
cd "${ROOT_DIR}"; mkdir -p "$(dirname "${REPORT}")"
steps=()
record() { local d; d="$(printf '%s' "$3" | tr '\n' ' ' | cut -c1-300 | sed 's/\\/\\\\/g; s/"/\\"/g')"; steps+=("{\"name\":\"$1\",\"ok\":$2,\"detail\":\"${d}\"}"); if [ "$2" = true ]; then echo "ok   $1: ${d:0:150}"; else echo "FAIL $1: ${d:0:200}"; fi; }
[ -S "${SOCKET}" ] || { echo "crossarch-suite: ${SOCKET} missing; run 'make docker-up'" >&2; exit 2; }
arch="$("${DOCKER}" image inspect --format '{{.Architecture}}' "${NATIVE_IMAGE}" 2>/dev/null)"
[ "${arch}" = "arm64" ] || "${DOCKER}" pull -q --platform linux/arm64 "${NATIVE_IMAGE}" >/dev/null 2>&1 || { echo "crossarch-suite: cannot pull ${NATIVE_IMAGE}" >&2; exit 2; }
"${DOCKER}" rmi -f "${AMD64_IMAGE}" >/dev/null 2>&1

out="$("${CTL[@]}" arch status 2>&1 | tr '\n' ';')"
case "${out}" in *"binfmt_misc=enabled"*"INTERP qemu-x86_64 enabled flags="*F*) record binfmt_qemu_registered true "${out}" ;; *) record binfmt_qemu_registered false "${out}" ;; esac
m1="$("${CTL[@]}" arch mode linux/arm64 2>&1)"; m2="$("${CTL[@]}" arch mode linux/amd64 2>&1)"
[ "${m1}" = "MODE arm64 native" ] && [ "${m2}" = "MODE amd64 emulated:qemu-x86_64" ] && record execution_mode_surfaced true "${m1}; ${m2}" || record execution_mode_surfaced false "${m1}; ${m2}"
m3="$("${CTL[@]}" arch mode linux/riscv64 2>/dev/null | grep -m1 '^MODE')"
case "${m3}" in *unsupported*"no binfmt_misc interpreter"*) record unsupported_arch_mode_clear true "${m3}" ;; *) record unsupported_arch_mode_clear false "${m3}" ;; esac

out="$("${DOCKER}" run --rm --platform linux/amd64 "${AMD64_IMAGE}" uname -m 2>&1 | tail -1)"
[ "${out}" = "x86_64" ] && record run_platform_amd64 true "docker run --platform linux/amd64 ${AMD64_IMAGE} uname -m -> ${out}" || record run_platform_amd64 false "${out}"
ia="$("${DOCKER}" image inspect --format '{{.Os}}/{{.Architecture}}' "${AMD64_IMAGE}" 2>&1)"; na="$("${DOCKER}" image inspect --format '{{.Os}}/{{.Architecture}}' "${NATIVE_IMAGE}" 2>&1)"
[ "${ia}" = "linux/amd64" ] && [ "${na}" = "linux/arm64" ] && record image_inspect_architecture true "${AMD64_IMAGE}: ${ia}; ${NATIVE_IMAGE}: ${na}" || record image_inspect_architecture false "${ia} / ${na}"
"${DOCKER}" rm -f xa-c >/dev/null 2>&1; "${DOCKER}" run -d --name xa-c --platform linux/amd64 "${AMD64_IMAGE}" sleep 60 >/dev/null 2>&1
cimg="$("${DOCKER}" inspect --format '{{.Image}}' xa-c 2>&1)"; ca="$("${DOCKER}" image inspect --format '{{.Os}}/{{.Architecture}}' "${cimg}" 2>&1)"; cp="$("${DOCKER}" inspect --format '{{.Platform}}' xa-c 2>&1)"
[ "${ca}" = "linux/amd64" ] && record container_inspect_architecture true "container xa-c: platform ${cp}, image ${cimg:7:12} is ${ca} (host arm64)" || record container_inspect_architecture false "${cp} ${ca}"
"${DOCKER}" rm -f xa-c >/dev/null 2>&1
# A separate reference again: a riscv64 pull would re-tag the native image.
out="$("${DOCKER}" run --rm --platform linux/riscv64 "${AMD64_IMAGE}" uname -m 2>&1 | grep -i -m1 -E 'platform|manifest|format')"
[ -n "${out}" ] && record unsupported_platform_docker_error true "linux/riscv64 ${AMD64_IMAGE}: ${out} (vesper-arch mode: $("${CTL[@]}" arch mode linux/riscv64 2>/dev/null | grep -m1 '^MODE'))" || record unsupported_platform_docker_error false "no error for an unsupported platform"
"${DOCKER}" rmi -f "${AMD64_IMAGE}" >/dev/null 2>&1; "${DOCKER}" pull -q --platform linux/amd64 "${AMD64_IMAGE}" >/dev/null 2>&1

work='i=0; while [ $i -lt 20000 ]; do i=$((i+1)); done; echo done'
t0=$(python3 -c 'import time; print(int(time.time()*1000))'); "${DOCKER}" run --rm "${NATIVE_IMAGE}" sh -c "${work}" >/dev/null 2>&1; t1=$(python3 -c 'import time; print(int(time.time()*1000))')
"${DOCKER}" run --rm --platform linux/amd64 "${AMD64_IMAGE}" sh -c "${work}" >/dev/null 2>&1; t2=$(python3 -c 'import time; print(int(time.time()*1000))')
nat=$(( t1 - t0 )); emu=$(( t2 - t1 )); ratio=$(python3 -c "print(round(${emu}/max(${nat},1),1))")
if python3 -c "import sys; sys.exit(0 if ${ratio} <= ${RATIO_LIMIT} else 1)"; then record emulated_cost_within_limit true "20k-iteration shell loop incl. container start: native ${nat} ms, emulated ${emu} ms, ratio ${ratio}x (limit ${RATIO_LIMIT}x)"; else record emulated_cost_within_limit false "native ${nat} ms, emulated ${emu} ms, ratio ${ratio}x > ${RATIO_LIMIT}x"; fi
"${DOCKER}" rmi -f "${AMD64_IMAGE}" >/dev/null 2>&1
arch="$("${DOCKER}" image inspect --format '{{.Architecture}}' "${NATIVE_IMAGE}" 2>/dev/null)"
[ "${arch}" = "arm64" ] && record native_image_untouched true "${NATIVE_IMAGE} still ${arch}" || record native_image_untouched false "${NATIVE_IMAGE} is now ${arch}"

passed=true; for s in "${steps[@]}"; do case "$s" in *'"ok":false'*) passed=false;; esac; done
printf '{"socket":"%s","steps":[%s],"passed":%s}\n' "${SOCKET}" "$(IFS=,; echo "${steps[*]}")" "${passed}" > "${REPORT}"
echo "crossarch suite: passed=${passed} (report: ${REPORT})"
[ "${passed}" = true ]

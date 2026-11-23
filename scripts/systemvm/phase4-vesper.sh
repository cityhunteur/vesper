#!/usr/bin/env bash
# ADR 0011 slice 4b: the Phase 4 suites with every container in a microVM. Each suite runs with
# VESPER_DOCKER_RUNTIME=vesper and DOCKER_BIN=scripts/systemvm/docker-runtime.sh, which gives each `run`, `create` and
# compose service the runtime. A failing step must be one of the recorded deviations below; the run fails on any
# other failure, and also when a recorded deviation passes (so the list stays true). Needs
# `make systemvm-up-shares && make docker-up` (binds-suite needs its three shares) and outbound network. Writes
# run/systemvm/phase4-vesper-report.json and keeps each suite's log and report in run/systemvm/phase4-vesper/.
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"
OUT="${ROOT_DIR}/run/systemvm/phase4-vesper"
REPORT="${ROOT_DIR}/run/systemvm/phase4-vesper-report.json"
SUITES="${PHASE4_SUITES:-docker-conformance ports-suite dns-suite workflow-suite binds-suite crossarch-suite}"
mkdir -p "${OUT}"
# suite:step  reason (each line one deviation of the vesper runtime from runc for that step)
DEVIATIONS='workflow-suite:platform_amd64 --platform linux/amd64 is refused: a microVM runs linux/arm64 only (D-087)
crossarch-suite:run_platform_amd64 --platform linux/amd64 is refused: a microVM runs linux/arm64 only (D-087)
crossarch-suite:container_inspect_architecture the amd64 container is never created: --platform linux/amd64 is refused (D-087)'
export VESPER_DOCKER_RUNTIME=vesper DOCKER_BIN="${ROOT_DIR}/scripts/systemvm/docker-runtime.sh"
rows=(); bad=0
for suite in ${SUITES}; do
  start=$(date +%s)
  timeout 2400 make "${suite}" < /dev/null > "${OUT}/${suite}.log" 2>&1
  rc=$?
  failed=$(sed -n 's/^FAIL \([a-z0-9_]*\):.*/\1/p' "${OUT}/${suite}.log" | sort -u | tr '\n' ' ')
  passed=$(grep -c '^ok ' "${OUT}/${suite}.log")
  report=$(ls -t run/systemvm/*-report.json 2>/dev/null | head -1)
  [ -n "${report}" ] && [ "$(stat -f %m "${report}")" -ge "${start}" ] && cp "${report}" "${OUT}/${suite}-report.json"
  unexpected=""; for step in ${failed}; do
    printf '%s\n' "${DEVIATIONS}" | grep -q "^${suite}:${step} " || unexpected="${unexpected} ${step}"
  done
  fixed=""; for d in $(printf '%s\n' "${DEVIATIONS}" | sed -n "s/^${suite}:\([a-z0-9_]*\) .*/\1/p"); do
    case " ${failed} " in *" ${d} "*) ;; *) fixed="${fixed} ${d}" ;; esac
  done
  # A suite that stopped before its steps (rc 2 with no step) is a failure too.
  [ "${rc}" != 0 ] && [ -z "${failed}" ] && unexpected="${unexpected} (exit ${rc} with no failing step; see ${OUT}/${suite}.log)"
  state=ok; { [ -n "${unexpected}" ] || [ -n "${fixed}" ]; } && { state=FAIL; bad=1; }
  echo "${state} ${suite}: ${passed} steps passed; deviations: ${failed:-none}${unexpected:+; UNEXPECTED:${unexpected}}${fixed:+; recorded but passing:${fixed}}"
  rows+=("{\"suite\":\"${suite}\",\"exit\":${rc},\"passed\":${passed},\"failed\":\"${failed% }\",\"unexpected\":\"${unexpected# }\",\"recorded_but_passing\":\"${fixed# }\"}")
done
printf '{"runtime":"vesper","suites":[%s],"passed":%s,"deviations":%s}\n' "$(IFS=,; echo "${rows[*]}")" \
  "$([ "${bad}" = 0 ] && echo true || echo false)" \
  "$(printf '%s\n' "${DEVIATIONS}" | python3 -I -c 'import json,sys; print(json.dumps([{"step": l.split(" ",1)[0], "reason": l.split(" ",1)[1]} for l in sys.stdin.read().splitlines() if l]))')" > "${REPORT}"
echo "phase4-vesper: passed=$([ "${bad}" = 0 ] && echo true || echo false) (report: ${REPORT})"
[ "${bad}" = 0 ]

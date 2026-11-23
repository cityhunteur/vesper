#!/usr/bin/env bash
# The Docker CLI with every container on one runtime (ADR 0011 slice 4b): the Phase 4 suites take it as DOCKER_BIN,
# so `VESPER_DOCKER_RUNTIME=vesper DOCKER_BIN=scripts/systemvm/docker-runtime.sh make <suite>` runs the same steps with
# each container in a microVM. `run` and `create` get `--runtime "$VESPER_DOCKER_RUNTIME"` first, so a step that names
# its own runtime (a runc control step) keeps it: the last --runtime wins. `compose` gets an override file that gives
# every service of the project the runtime. Everything else passes through unchanged.
set -euo pipefail
RT="${VESPER_DOCKER_RUNTIME:?VESPER_DOCKER_RUNTIME names the runtime}"
DOCKER="${VESPER_DOCKER_REAL:-docker}"
case "${1:-}" in
  run|create)
    sub=$1; shift
    exec "${DOCKER}" "${sub}" --runtime "${RT}" "$@" ;;
  container)
    if [ "${2:-}" = run ] || [ "${2:-}" = create ]; then
      sub=$2; shift 2
      exec "${DOCKER}" container "${sub}" --runtime "${RT}" "$@"
    fi ;;
  compose)
    shift
    # The compose files and project flags come before the compose subcommand; the override names every service.
    files=(); pre=()
    while [ $# -gt 0 ]; do
      case "$1" in
        -f|--file) files+=("$2"); pre+=("$1" "$2"); shift 2 ;;
        -p|--project-name|--project-directory|--env-file|--profile) pre+=("$1" "$2"); shift 2 ;;
        -*) pre+=("$1"); shift ;;
        *) break ;;
      esac
    done
    # No compose file (compose version, compose ls): nothing to give a runtime.
    [ "${#files[@]}" -gt 0 ] || exec "${DOCKER}" compose ${pre[@]+"${pre[@]}"} "$@"
    override="$(mktemp "${TMPDIR:-/tmp}/vesper-compose-runtime.XXXXXX")"
    trap 'rm -f "${override}"' EXIT
    {
      echo "services:"
      "${DOCKER}" compose ${pre[@]+"${pre[@]}"} config --services | while read -r svc; do
        [ -n "${svc}" ] && printf '  %s:\n    runtime: %s\n' "${svc}" "${RT}"
      done
    } > "${override}"
    "${DOCKER}" compose ${pre[@]+"${pre[@]}"} -f "${override}" "$@"
    exit $? ;;
esac
exec "${DOCKER}" "$@"

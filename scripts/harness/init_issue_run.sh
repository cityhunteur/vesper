#!/usr/bin/env bash
set -euo pipefail

ISSUE_ID="${1:-}"
REPO_ROOT="${2:-.}"

if [ -z "$ISSUE_ID" ]; then
  echo "usage: $0 ISSUE-ID [REPO-ROOT]" >&2
  exit 2
fi

cd "$REPO_ROOT"

RUN_ROOT=".symphony/runs/${ISSUE_ID}/current"
LOCAL_RUN_ROOT="run/${ISSUE_ID}"
RESULTS_TSV=".symphony/runs/${ISSUE_ID}/results.tsv"

mkdir -p "${RUN_ROOT}/"{api,proxy,systemvm,council}
mkdir -p "${LOCAL_RUN_ROOT}/"{api,proxy,systemvm}

if [ ! -f "${RESULTS_TSV}" ]; then
  mkdir -p "$(dirname "${RESULTS_TSV}")"
  printf 'attempt\tstarted_at\tresult\tnotes\n' > "${RESULTS_TSV}"
fi

if [ ! -f "${RUN_ROOT}/verification.yaml" ] && [ -f ".symphony/templates/verification-manifest-template.yaml" ]; then
  cp ".symphony/templates/verification-manifest-template.yaml" "${RUN_ROOT}/verification.yaml"
fi

if [ ! -f "${RUN_ROOT}/notes.md" ]; then
  cat > "${RUN_ROOT}/notes.md" <<EOF
# Notes

- issue: ${ISSUE_ID}
- started_at: $(date -u +"%Y-%m-%dT%H:%M:%SZ")
- git_head: $(git rev-parse --short HEAD 2>/dev/null || echo unknown)
EOF
fi

cat > "${RUN_ROOT}/env.sh" <<EOF
export VESPER_ISSUE_ID="${ISSUE_ID}"
export VESPER_RUN_DIR="$(pwd)/${LOCAL_RUN_ROOT}"
export VESPER_BUNDLE_DIR="$(pwd)/${RUN_ROOT}"
EOF

echo "initialized issue run for ${ISSUE_ID}"
echo "local run dir: ${LOCAL_RUN_ROOT}"
echo "bundle dir: ${RUN_ROOT}"

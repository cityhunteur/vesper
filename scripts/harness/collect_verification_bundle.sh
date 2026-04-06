#!/usr/bin/env bash
set -euo pipefail

ISSUE_ID="${1:-}"
REPO_ROOT="${2:-.}"

if [ -z "$ISSUE_ID" ]; then
  echo "usage: $0 ISSUE-ID [REPO-ROOT]" >&2
  exit 2
fi

cd "$REPO_ROOT"

BUNDLE_DIR=".symphony/runs/${ISSUE_ID}/current"
mkdir -p "${BUNDLE_DIR}"/{api,proxy,systemvm}

{
  echo "issue: ${ISSUE_ID}"
  echo "collected_at: $(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  echo "git_head: $(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
  echo "git_branch: $(git branch --show-current 2>/dev/null || echo unknown)"
} > "${BUNDLE_DIR}/git-info.txt"

git status --short > "${BUNDLE_DIR}/git-status.txt" 2>/dev/null || true

copy_if_exists() {
  local src="$1"
  local dst="$2"
  if [ -f "$src" ]; then
    mkdir -p "$(dirname "$dst")"
    cp "$src" "$dst"
  fi
}

# System VM artifacts
copy_if_exists "build/systemvm/image-manifest.json" "${BUNDLE_DIR}/systemvm/image-manifest.json"
copy_if_exists "build/systemvm/launch-config.json" "${BUNDLE_DIR}/systemvm/launch-config.json"
copy_if_exists "run/systemvm/boot.log" "${BUNDLE_DIR}/systemvm/boot.log"
copy_if_exists "run/systemvm/boot-report.json" "${BUNDLE_DIR}/systemvm/boot-report.json"
copy_if_exists "run/${ISSUE_ID}/systemvm/boot.log" "${BUNDLE_DIR}/systemvm/boot.issue.log"
copy_if_exists "run/${ISSUE_ID}/systemvm/boot-report.json" "${BUNDLE_DIR}/systemvm/boot-report.issue.json"

# Proxy artifacts
copy_if_exists "run/${ISSUE_ID}/proxy/proxy.log" "${BUNDLE_DIR}/proxy/proxy.log"
copy_if_exists "run/proxy/proxy.log" "${BUNDLE_DIR}/proxy/proxy.log"

# API artifacts
copy_if_exists "run/${ISSUE_ID}/api/requests.log" "${BUNDLE_DIR}/api/requests.log"
copy_if_exists "run/${ISSUE_ID}/api/responses.log" "${BUNDLE_DIR}/api/responses.log"

if [ ! -f "${BUNDLE_DIR}/verification.yaml" ] && [ -f ".symphony/templates/verification-manifest-template.yaml" ]; then
  cp ".symphony/templates/verification-manifest-template.yaml" "${BUNDLE_DIR}/verification.yaml"
fi

if [ ! -f "${BUNDLE_DIR}/notes.md" ]; then
  cat > "${BUNDLE_DIR}/notes.md" <<EOF
# Notes

- issue: ${ISSUE_ID}
- collected_at: $(date -u +"%Y-%m-%dT%H:%M:%SZ")
EOF
fi

echo "collected verification bundle for ${ISSUE_ID} at ${BUNDLE_DIR}"

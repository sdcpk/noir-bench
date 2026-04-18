#!/usr/bin/env bash
set -euo pipefail

TARGET_FILE="src/core/schema.rs"

if [[ "${ALLOW_CORE_SCHEMA_CHANGE:-0}" == "1" ]]; then
  echo "Schema guard bypassed: ALLOW_CORE_SCHEMA_CHANGE=1"
  exit 0
fi

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "error: not inside a git repository"
  exit 1
fi

range=""
if [[ -n "${GITHUB_BASE_REF:-}" ]]; then
  base_ref="origin/${GITHUB_BASE_REF}"

  if ! git rev-parse --verify "${base_ref}" >/dev/null 2>&1; then
    git fetch --no-tags --depth=1 origin "${GITHUB_BASE_REF}" >/dev/null 2>&1 || true
  fi

  if git rev-parse --verify "${base_ref}" >/dev/null 2>&1; then
    base_commit="$(git merge-base "${base_ref}" HEAD)"
    range="${base_commit}..HEAD"
  elif git rev-parse --verify FETCH_HEAD >/dev/null 2>&1; then
    base_commit="$(git merge-base FETCH_HEAD HEAD)"
    range="${base_commit}..HEAD"
  fi
fi

if [[ -z "${range}" ]] && git rev-parse --verify HEAD~1 >/dev/null 2>&1; then
  range="HEAD~1..HEAD"
fi

if [[ -n "${range}" ]]; then
  changed="$(git diff --name-only "${range}" -- "${TARGET_FILE}")"
else
  changed="$(git show --name-only --pretty='' HEAD -- "${TARGET_FILE}")"
fi

if [[ -n "${changed}" ]]; then
  echo "error: ${TARGET_FILE} changed in this workflow range."
  echo "Set ALLOW_CORE_SCHEMA_CHANGE=1 only when schema mutation is explicitly authorized."
  exit 1
fi

echo "Schema guard passed: ${TARGET_FILE} unchanged."

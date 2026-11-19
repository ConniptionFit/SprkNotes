#!/usr/bin/env bash
# Fails on the words record/recording/recorded in user-facing text (SPEC 13.1).
# Developer docs (CLAUDE.md, SPEC.md, docs/adr, this script) are excluded.
set -euo pipefail
cd "$(dirname "$0")/.."
targets=(README.md app/ui/src app/ui/index.html app/src-tauri/src packaging docs/admin-guide.md)
existing=(); for t in "${targets[@]}"; do [ -e "$t" ] && existing+=("$t"); done
if grep -rniwE 'record|records|recording|recordings|recorded' "${existing[@]}" 2>/dev/null; then
  echo "check-terms: use 'transcribing' / 'transcript' instead"; exit 1
fi
echo "check-terms: ok"

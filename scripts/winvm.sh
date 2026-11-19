#!/usr/bin/env bash
# Sync the current branch to the Windows VM and run a command there.
# Usage: scripts/winvm.sh <test|run-app|release|check|shell> [args]
# Env: SPRK_WINVM (ssh host, default sprk-winvm), SPRK_WINVM_DIR (default C:/sprknotes)
set -euo pipefail
cd "$(dirname "$0")/.."
HOST="${SPRK_WINVM:-sprk-winvm}"
DIR="${SPRK_WINVM_DIR:-C:/sprknotes}"
cmd="${1:-}"; shift || true

ssh_run() { ssh -o BatchMode=yes -o ConnectTimeout=10 "$HOST" "$@"; }

sync() {
  git diff --quiet && git diff --cached --quiet || echo "note: uncommitted changes are NOT synced; commit first" >&2
  local branch bundle; branch=$(git rev-parse --abbrev-ref HEAD)
  bundle=$(mktemp -u).bundle
  git bundle create "$bundle" "$branch" >/dev/null 2>&1
  scp -q -o BatchMode=yes "$bundle" "$HOST:sprk.bundle"; rm -f "$bundle"
  ssh_run "powershell -NoProfile -Command \"if (!(Test-Path '$DIR')) { git init '$DIR' }; cd '$DIR'; git fetch (Resolve-Path ~/sprk.bundle) '+${branch}:refs/remotes/bundle/${branch}'; git checkout -f -B '${branch}' 'refs/remotes/bundle/${branch}'\""
}

case "$cmd" in
  check) ssh_run "powershell -NoProfile -Command \"'reachable'; [Environment]::OSVersion.VersionString\"" ;;
  test) sync; ssh_run "powershell -NoProfile -Command \"cd '$DIR'; cargo test --workspace\"" ;;
  run-app) sync; ssh_run "powershell -NoProfile -Command \"cd '$DIR/app/ui'; npm ci; npm run tauri dev\"" ;;
  release) sync; ssh_run "powershell -NoProfile -Command \"cd '$DIR'; pwsh scripts/release.ps1\"" ;;
  shell) ssh -t "$HOST" ;;
  *) echo "usage: $0 <check|test|run-app|release|shell>"; exit 2 ;;
esac

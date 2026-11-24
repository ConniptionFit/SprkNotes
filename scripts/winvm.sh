#!/usr/bin/env bash
# Sync the current branch to the Windows VM and run a command there.
# Usage: scripts/winvm.sh <test|run-app|release|release-tag|dist|check|shell> [args]
# Env: SPRK_WINVM (ssh host, default sprk-winvm), SPRK_WINVM_DIR (default C:/sprknotes)
set -euo pipefail
cd "${SPRK_WINVM_ROOT:-$(dirname "$0")/..}"
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
  run-app) sync; ssh_run "powershell -NoProfile -Command \"cd '$DIR/app'; npm.cmd --prefix ui ci; .\\ui\\node_modules\\.bin\\tauri.cmd dev\"" ;;
  # The ssh service keeps the PATH it started with, so tools installed later (CMake, LLVM, wix) are
  # invisible over ssh until it restarts. Re-read PATH from the registry first.
  # The command is sent as -EncodedCommand (base64 UTF-16LE) so neither bash nor the remote shell can expand or re-quote it.
  release) sync
        ps="\$env:PATH = [Environment]::GetEnvironmentVariable('PATH','Machine') + ';' + [Environment]::GetEnvironmentVariable('PATH','User') + ';' + \$env:PATH; Set-Location '$DIR'; pwsh scripts/release.ps1 $*; exit \$LASTEXITCODE"
        enc=$(printf '%s' "$ps" | iconv -f UTF-8 -t UTF-16LE | base64 -w0)
        ssh_run "powershell -NoProfile -EncodedCommand $enc" ;;
  # Build the exact tagged commit: scripts/winvm.sh release-tag v0.2.0-alpha.1 -SherpaX64 ... (the tag's code, nothing else)
  release-tag) tag="${1:?tag}"; shift; git rev-parse -q --verify "refs/tags/$tag" >/dev/null || { echo "no tag $tag (git fetch --tags)"; exit 1; }
        root="$PWD"; me=$(mktemp); cp "$0" "$me"   # run this script, not the tag's older copy
        git switch -q -C release-build "$tag"; SPRK_WINVM_ROOT="$root" exec bash "$me" release "$@" ;;
  # Copy the finished dist folder back to ./dist-win (then: scripts/publish-release.sh all vX ./dist-win)
  dist) rm -rf dist-win; scp -q -r -o BatchMode=yes "$HOST:${DIR#C:/}/dist" dist-win 2>/dev/null || scp -q -r -o BatchMode=yes "$HOST:$DIR/dist" dist-win
        find dist-win -mindepth 1 -maxdepth 1 -type d -exec rm -rf {} +; ls -l dist-win ;;
  shell) ssh -t "$HOST" ;;
  *) echo "usage: $0 <check|test|run-app|release [release.ps1 args]|release-tag TAG [args]|dist|shell>"; exit 2 ;;
esac

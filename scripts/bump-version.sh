#!/usr/bin/env bash
# Usage: scripts/bump-version.sh <x.y.z[-beta.N|-alpha.N]> | --check
# Root Cargo.toml [workspace.package] version is the only source. This script
# mirrors it into the files that cannot inherit it (Tauri crate, UI package).
set -euo pipefail
cd "$(dirname "$0")/.."
src() { sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p}' Cargo.toml; }
tauri() { sed -n '0,/^version = "\(.*\)"/s//\1/p' app/src-tauri/Cargo.toml; }
ui() { sed -n 's/.*"version": "\(.*\)".*/\1/p' app/ui/package.json | head -1; }
if [ "${1:-}" = "--check" ]; then
  v=$(src); ok=0
  [ "$(tauri)" = "$v" ] || { echo "app/src-tauri version $(tauri) != $v"; ok=1; }
  [ "$(ui)" = "$v" ] || { echo "app/ui version $(ui) != $v"; ok=1; }
  [ $ok = 0 ] && echo "versions aligned at $v"; exit $ok
fi
new="${1:?usage: bump-version.sh <x.y.z> | --check}"
[[ "$new" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-(alpha|beta)\.[0-9]+)?$ ]] || { echo "bad version"; exit 2; }
sed -i '/^\[workspace.package\]/,/^\[/s/^version = ".*"/version = "'"$new"'"/' Cargo.toml
sed -i '0,/^version = ".*"/s//version = "'"$new"'"/' app/src-tauri/Cargo.toml
sed -i '0,/"version": ".*"/s//"version": "'"$new"'"/' app/ui/package.json
echo "bumped to $new"

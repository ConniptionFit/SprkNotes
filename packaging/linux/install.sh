#!/bin/sh
# Per-user install of a native Linux build: scripts/linux build output -> ~/.local
# Usage: packaging/linux/install.sh [path-to-sprknotes-binary]
# Needs webkit2gtk-4.1; "Others" (system audio) needs parec from pipewire-pulse/pulseaudio-utils.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
bin=${1:-$repo/app/src-tauri/target/release/sprknotes}
[ -x "$bin" ] || { echo "binary not found: $bin (build with: cd app/src-tauri && cargo build --release --features engine,tauri/custom-protocol)" >&2; exit 1; }
install -Dm755 "$bin" "$HOME/.local/bin/sprknotes"
# The assistant helper (ADR-009), if it was built: crates/sprk-assistant-llama (cargo build --release).
helper=$repo/crates/sprk-assistant-llama/target/release/sprk-assistant
[ -x "$helper" ] && install -Dm755 "$helper" "$HOME/.local/bin/sprk-assistant"
install -Dm644 "$here/sprknotes.desktop" "$HOME/.local/share/applications/sprknotes.desktop"
if [ -f "$repo/app/src-tauri/icons/128x128.png" ]; then
  install -Dm644 "$repo/app/src-tauri/icons/128x128.png" "$HOME/.local/share/icons/hicolor/128x128/apps/sprknotes.png"
fi
echo "Installed. Launch SprkNotes from your app menu or run: sprknotes"

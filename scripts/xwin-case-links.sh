#!/usr/bin/env bash
# cargo-xwin stores the Windows SDK import libs with lowercase names; the prebuilt sherpa-onnx
# static libs ask for `PathCch.lib`, which a case-sensitive Linux filesystem will not match.
# Add the missing mixed-case links (idempotent, touches only the user-level xwin cache).
set -euo pipefail
XWIN="${XWIN_CACHE_DIR:-$HOME/.cache/cargo-xwin}/xwin"
for arch in x86_64 aarch64; do
  d="$XWIN/sdk/lib/um/$arch"
  [ -d "$d" ] || { echo "no $d yet: run a cargo xwin build once first" >&2; continue; }
  [ -e "$d/PathCch.lib" ] || { ln -s pathcch.lib "$d/PathCch.lib"; echo "linked $arch/PathCch.lib" >&2; }
done

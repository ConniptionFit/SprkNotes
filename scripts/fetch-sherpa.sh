#!/usr/bin/env bash
# Fetch the prebuilt sherpa-onnx static libs for the targets we build (dev tooling only).
# Nothing here is committed or shipped: libs go to $SPRK_CACHE (default ~/.cache/sprknotes).
# Usage: scripts/fetch-sherpa.sh [linux-x64|win-x64|win-arm64|win-x64-shared|win-arm64-shared|android-arm64 ...]
#   (default: linux-x64 win-x64 win-arm64-shared)
# Then build with, for example:
#   SHERPA_ONNX_LIB_DIR="$(scripts/fetch-sherpa.sh --print linux-x64)" cargo test -p sprk-asr --features sherpa
set -euo pipefail
VER=2.0.1   # must match the sherpa-onnx crate version in Cargo.lock
CACHE="${SPRK_CACHE:-$HOME/.cache/sprknotes}/sherpa"
BASE="https://github.com/k2-fsa/sherpa-onnx/releases/download/v$VER"

archive() {
  case "$1" in
    linux-x64) echo "sherpa-onnx-v$VER-linux-x64-static-lib" ;;
    win-x64)   echo "sherpa-onnx-v$VER-win-x64-static-MT-Release-lib" ;;
    win-arm64) echo "sherpa-onnx-v$VER-win-arm64-static-MT-Release-lib" ;;
    win-x64-shared)   echo "sherpa-onnx-v$VER-win-x64-shared-MT-Release-lib" ;;
    win-arm64-shared) echo "sherpa-onnx-v$VER-win-arm64-shared-MT-Release-lib" ;;
    android-arm64) echo "sherpa-onnx-v$VER-android" ;;
    *) echo "unknown target '$1' (linux-x64|win-x64|win-arm64|win-x64-shared|win-arm64-shared|android-arm64)" >&2; exit 2 ;;
  esac
}

print_only=0
[ "${1:-}" = "--print" ] && { print_only=1; shift; }
targets=("$@"); [ ${#targets[@]} -eq 0 ] && targets=(linux-x64 win-x64 win-arm64-shared)

mkdir -p "$CACHE"
for t in "${targets[@]}"; do
  name=$(archive "$t"); dir="$CACHE/$name"
  if [ "$t" = android-arm64 ]; then
    # Phones (ADR-015 Decision 3): arm64 only, and only the two libraries the C API needs
    # (libsherpa-onnx-c-api.so and libonnxruntime.so, about 26 MB before APK compression).
    # The archive also carries JNI and C++ wrappers and three other ABIs we do not ship.
    dir="$CACHE/sherpa-onnx-v$VER-android-arm64"
    if [ ! -d "$dir/lib" ]; then
      [ $print_only -eq 1 ] && { echo "not fetched: run scripts/fetch-sherpa.sh $t" >&2; exit 1; }
      echo "fetching $name.tar.bz2" >&2
      curl -fL --retry 3 -o "$CACHE/$name.tar.bz2.part" "$BASE/$name.tar.bz2"
      mv "$CACHE/$name.tar.bz2.part" "$CACHE/$name.tar.bz2"
      sha256sum "$CACHE/$name.tar.bz2" >&2
      tmp="$CACHE/$name.extract"; rm -rf "$tmp"; mkdir -p "$tmp"
      tar -xjf "$CACHE/$name.tar.bz2" -C "$tmp"
      mkdir -p "$dir/lib"
      for so in libsherpa-onnx-c-api.so libonnxruntime.so; do
        cp "$(find "$tmp" -path '*arm64-v8a*' -name "$so" | head -1)" "$dir/lib/"
      done
      rm -rf "$tmp" "$CACHE/$name.tar.bz2"
    fi
    [ $print_only -eq 1 ] && echo "$dir/lib" || echo "$t: $dir/lib" >&2
    continue
  fi
  if [ ! -d "$dir/lib" ]; then
    [ $print_only -eq 1 ] && { echo "not fetched: run scripts/fetch-sherpa.sh $t" >&2; exit 1; }
    echo "fetching $name.tar.bz2" >&2
    curl -fL --retry 3 -o "$CACHE/$name.tar.bz2.part" "$BASE/$name.tar.bz2"
    mv "$CACHE/$name.tar.bz2.part" "$CACHE/$name.tar.bz2"
    sha256sum "$CACHE/$name.tar.bz2" >&2
    tar -xjf "$CACHE/$name.tar.bz2" -C "$CACHE"
  fi
  [ $print_only -eq 1 ] && echo "$dir/lib" || echo "$t: $dir/lib" >&2
done

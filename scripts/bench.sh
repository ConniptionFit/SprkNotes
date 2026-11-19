#!/usr/bin/env bash
# Engine accuracy on synthetic and public-domain fixtures (SPEC 13.1, M1).
# Usage: scripts/bench.sh --accuracy-only [model-id ...]
# Needs: scripts/fetch-models.sh and scripts/fetch-sherpa.sh to have run (this script runs
# them if the cache is empty), espeak-ng and ffmpeg. Audio is generated into the cache, never
# into the repo. Speed numbers printed here come from the dev host and are NOT gates (SPEC 13.3).
set -euo pipefail
cd "$(dirname "$0")/.."
[ "${1:-}" = "--accuracy-only" ] || { echo "usage: $0 --accuracy-only [model-id ...]" >&2; exit 2; }
shift
CACHE="${SPRK_CACHE:-$HOME/.cache/sprknotes}"
MODELS="$CACHE/models"; SYNTH="$CACHE/fixtures-synthetic"

[ -d "$MODELS/silero-vad" ] || scripts/fetch-models.sh
SHERPA_ONNX_LIB_DIR="$(scripts/fetch-sherpa.sh --print linux-x64 2>/dev/null || { scripts/fetch-sherpa.sh linux-x64; scripts/fetch-sherpa.sh --print linux-x64; })"
export SHERPA_ONNX_LIB_DIR
cargo build -q --release -p sprkctl --features engine
CTL=target/release/sprkctl

# Synthetic speech from the committed sentences: 16 kHz mono, written only under $CACHE.
if [ ! -f "$SYNTH/trans.txt" ]; then
  mkdir -p "$SYNTH"; : > "$SYNTH/trans.txt"; i=0
  while IFS= read -r line; do
    i=$((i+1)); n=$(printf 'synth%02d.wav' $i)
    espeak-ng -v en-us -s 150 --stdout "$line" | ffmpeg -loglevel error -y -i - -ar 16000 -ac 1 "$SYNTH/$n"
    printf '%s %s\n' "$n" "$line" >> "$SYNTH/trans.txt"
  done < tests/fixtures/accuracy/sentences.txt
fi
LIBRI="$CACHE/fixtures-whisper-small.en"   # LibriSpeech clips shipped with the model tarball

models=("$@"); [ ${#models[@]} -eq 0 ] && models=(parakeet-tdt-0.6b-v2-int8 whisper-small.en whisper-base.en)
for m in "${models[@]}"; do
  echo "================ $m"
  for set in "$LIBRI" "$SYNTH"; do
    [ -f "$set/trans.txt" ] || continue
    echo "--- fixtures: $(basename "$set")"
    "$CTL" bench accuracy "$MODELS/$m" "$set" --vad "$MODELS/silero-vad"
  done
done

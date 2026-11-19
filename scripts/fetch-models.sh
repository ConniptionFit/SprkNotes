#!/usr/bin/env bash
# Fetch candidate transcription models and turn each into a model folder with a verified
# model.json (SPEC 16.5). Dev tooling only: weights are never committed or shipped from here.
# Usage: scripts/fetch-models.sh [parakeet-tdt-0.6b-v2-int8 whisper-small.en whisper-base.en silero-vad]
# Output: $SPRK_CACHE/models/<id>/   (default ~/.cache/sprknotes/models)
set -euo pipefail
CACHE="${SPRK_CACHE:-$HOME/.cache/sprknotes}"
OUT="$CACHE/models"; DL="$CACHE/downloads"
REL=https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models
mkdir -p "$OUT" "$DL"

fetch() {  # url file
  [ -f "$DL/$2" ] || { curl -fL --retry 3 -o "$DL/$2.part" "$1" && mv "$DL/$2.part" "$DL/$2"; }
  sha256sum "$DL/$2" >&2
}

# write_manifest dir id version display engine punct casing license source notes role=file...
write_manifest() {
  local dir=$1 id=$2 ver=$3 disp=$4 engine=$5 punct=$6 casing=$7 lic=$8 src=$9 notes=${10}; shift 10
  local files="" hashes="" sep=""
  for kv in "$@"; do
    role=${kv%%=*}; file=${kv#*=}
    files+="$sep\"$role\": \"$file\""
    hashes+="$sep\"$file\": \"$(sha256sum "$dir/$file" | cut -d' ' -f1)\""
    sep=", "
  done
  cat > "$dir/model.json" <<JSON
{
  "schema": 1,
  "id": "$id",
  "version": "$ver",
  "display_name": "$disp",
  "engine": "$engine",
  "languages": ["en"],
  "sample_rate": 16000,
  "features": { "punctuation": $punct, "casing": $casing, "streaming": false },
  "accelerators": ["cpu"],
  "min_app_version": "0.0.1",
  "files": { $files },
  "sha256": { $hashes },
  "license": "$lic",
  "source": "$src",
  "notes": "$notes"
}
JSON
}

VER=2026.11.0
targets=("$@"); [ ${#targets[@]} -eq 0 ] && targets=(parakeet-tdt-0.6b-v2-int8 whisper-small.en whisper-base.en silero-vad)
for t in "${targets[@]}"; do
  case "$t" in
    parakeet-tdt-0.6b-v2-int8)
      n=sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8
      fetch "$REL/$n.tar.bz2" "$n.tar.bz2"; tar -xjf "$DL/$n.tar.bz2" -C "$DL"
      d="$OUT/$t"; mkdir -p "$d"; cp "$DL/$n"/{encoder.int8.onnx,decoder.int8.onnx,joiner.int8.onnx,tokens.txt} "$d/"
      write_manifest "$d" "$t" "$VER" "Parakeet TDT 0.6B v2 (int8)" sherpa-onnx/offline-transducer true true \
        "CC-BY-4.0" "$REL/$n.tar.bz2" "Benchmark candidate (M1)" \
        encoder=encoder.int8.onnx decoder=decoder.int8.onnx joiner=joiner.int8.onnx tokens=tokens.txt
      rm -rf "$d/test_wavs"; [ -d "$DL/$n/test_wavs" ] && ln -sfn "$DL/$n/test_wavs" "$CACHE/fixtures-$t" ;;
    whisper-small.en|whisper-base.en)
      n=sherpa-onnx-$t
      fetch "$REL/$n.tar.bz2" "$n.tar.bz2"; tar -xjf "$DL/$n.tar.bz2" -C "$DL"
      d="$OUT/$t"; mkdir -p "$d"; s=${t#whisper-}; s=${s%.en}
      cp "$DL/$n/$s.en-encoder.int8.onnx" "$DL/$n/$s.en-decoder.int8.onnx" "$DL/$n/$s.en-tokens.txt" "$d/"
      write_manifest "$d" "$t" "$VER" "Whisper $s.en (int8)" sherpa-onnx/whisper true true \
        "MIT" "$REL/$n.tar.bz2" "Benchmark candidate (M1)" \
        encoder="$s.en-encoder.int8.onnx" decoder="$s.en-decoder.int8.onnx" tokens="$s.en-tokens.txt"
      [ -d "$DL/$n/test_wavs" ] && ln -sfn "$DL/$n/test_wavs" "$CACHE/fixtures-$t" ;;
    silero-vad)
      fetch "$REL/silero_vad.onnx" silero_vad.onnx
      d="$OUT/silero-vad"; mkdir -p "$d"; cp "$DL/silero_vad.onnx" "$d/"
      write_manifest "$d" silero-vad "$VER" "Silero VAD" sherpa-onnx/vad-silero false false \
        "MIT" "$REL/silero_vad.onnx" "Voice activity detection (M1)" vad=silero_vad.onnx ;;
    *) echo "unknown model '$t'" >&2; exit 2 ;;
  esac
  echo "ready: $OUT/$t" >&2
done

# Accuracy fixtures

Audio is **never committed** (`*.wav` is git-ignored). This folder holds only reference text.

- `sentences.txt`: original, synthetic sentences. `scripts/bench.sh` renders them with
  `espeak-ng` (a synthetic voice, so scores are a pessimistic floor, not a human-speech number).
- LibriSpeech clips (public-domain book text, CC-BY-4.0 audio) come inside the Whisper model
  tarballs that `scripts/fetch-models.sh` downloads; they are used from the cache, not copied here.

Run: `scripts/bench.sh --accuracy-only`

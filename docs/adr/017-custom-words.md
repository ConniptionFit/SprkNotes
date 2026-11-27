# ADR-017: Custom words

Status: accepted (2026-11-26, John's request)

## Context
People need names, medicines and product names spelled right ("metoprolol", a colleague's surname, the company name). There was no custom vocabulary. John asked for a **Custom words** list in Settings, an admin policy key, and the two lists combined.

Spike against the pinned engine (sherpa-onnx 2.0.1, `scripts/fetch-sherpa.sh` `VER=` and the `sherpa-onnx = "=2.0.1"` binding; sources read at the `v2.0.1` tag, nothing downloaded into the build):
- The NeMo transducer recognizer (`offline-recognizer-transducer-nemo-impl.h`) supports `decoding_method=modified_beam_search` with hotwords for TDT models (`OfflineTransducerModifiedBeamSearchNeMoDecoder`, `IsTDT()` passed through). Greedy search stays the default.
- The Rust binding exposes `decoding_method`, `max_active_paths`, `hotwords_file`, `hotwords_score`, `modeling_unit`, `bpe_vocab` and `OfflineRecognizer::create_stream_with_hotwords`, so hints can go to each stream from memory with no hotwords file on disk.
- Turning a hint into the model's tokens needs `modeling_unit=bpe` **and** a `bpe_vocab` file (the SentencePiece vocabulary). The Parakeet TDT 0.6B v2 int8 release ships only encoder, decoder, joiner and `tokens.txt`, and so does our pack. Without `bpe_vocab` the native code dereferences a null encoder on the first hint; with an unknown modeling unit it calls `exit(-1)`. Neither comes back as an error, so "try it and fall back" cannot catch them.
- Other native hazards: `/` separates hints, a word starting with `:` is read as a score through `std::stof` (a bad one throws across the FFI boundary and aborts), the binding panics on a NUL, and a hint that fails to encode is echoed to stderr with its text.

## Decision
Two layers, both behind the engine seam (hard rule 15):
1. **Word hints, gated on the pack.** `Engine::bias_toward(&[String]) -> bool` (default false) is called once at session start. The sherpa-onnx transducer adapter answers true only when the pack's `model.json` declares the optional `bpe_vocab` file role: it then builds a second recognizer with `modified_beam_search`, `max_active_paths=4`, `hotwords_score=1.5`, `modeling_unit=bpe` and that file, and only replaces the greedy one if creation succeeds. Hints are passed to every stream from memory, cleaned to letters, digits and plain apostrophes so none of the hazards above can be reached. Empty list, no `bpe_vocab`, or a failed build: decoding stays greedy and unchanged. Whisper never takes hints.
2. **Spelling fixes after decoding, always on when the list is not empty.** `sprk_asr::custom_words::Corrected` wraps the session engine, so committed lines, the live line and captions all get the same fix. It rewrites near misses to the listed spelling with conservative rules (SPEC 4.3a): only entries of five or more letters match loosely, the first letter must agree, at most one letter may differ (two from ten letters up), common English words and words with digits are never changed loosely, and words run together or split apart only match exactly.

Because today's Parakeet pack has no `bpe_vocab`, layer 2 is what ships in practice. Layer 1 turns on by itself for a future pack that adds the file, with no app change.

Settings shows what the last session did (how many words, and whether the model took them as hints). The words are PHI-grade: never logged or put in errors. The person's list lives in `settings.json` (`customWords`); the admin list comes from the `CustomWords` policy key and adds to it.

## Consequences
- No new crate, no model download, no file written for the hints.
- Layer 2 can still change a rare real word that is one letter from a listed name of five or more letters (for example "Lasik" with "Lasix" listed). The rules keep that to listed words the person chose.
- Layer 1 has not been run against a real model in this repo: it needs a pack with `bpe_vocab` (exported from the NeMo tokenizer) and then accuracy and speed checks, since modified beam search is slower than greedy search. The Snapdragon laptop must confirm the cost before such a pack ships (SPEC 13.3). Hints that fail to encode are echoed by sherpa-onnx to stderr, which the app does not keep, but a pack whose `bpe_vocab` does not match its `tokens.txt` must not ship.

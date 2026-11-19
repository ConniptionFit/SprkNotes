# Changelog

## 0.0.1 (unreleased)
- M10 (portable core): model stores (active.json, activate, rollback, prune, remove), resolver (pinned, machine, user, bundled, allow-list, fallback), .sprkmodel pack and verify (zip-slip, size limits, allow-list, signature trait), import flow with staging, smoke test and rejects, `sprkctl model pack|verify|hash|install|import|list|activate|rollback|remove` with RMM exit codes.
- M11/M12 (portable core): audio source activity tracker with the 18.4 timing rules and exclusions; call title parsing and labeling modes with two-poll hysteresis and spec event lines.
- M3 (portable core): transcript format 5.1 writer (append + flush, sidecar JSONL, atomic finalize, rename ranking), naming and sanitizing, crash recovery; commit buffer with filler filter, fragment merge, long-utterance split and bleed dedupe (M4 layer 3). ADR-003 accepted (Parakeet).
- M1: `model.json` schema 1 (parse, validate, hash verify), engine registry, sherpa-onnx adapters (Parakeet transducer, Whisper) and Silero VAD, WAV reader, WER scoring, streaming resampler, `sprkctl transcribe` (WAV and live mic) and `sprkctl bench accuracy`, `scripts/bench.sh`, fetch scripts, ADR-003 (proposed, laptop numbers pending).
- M0: workspace scaffold, platform seam traits and fakes, Tauri + Svelte shell, dev scripts, ADR-001 and ADR-002.

# ADR-003: Default transcription model and speech engine adapter

Status: **accepted** (Parakeet TDT 0.6B v2 int8 chosen as the default by the product owner, 2026-11-19). Accuracy and dev-host numbers are in. The ARM64 and Intel laptop numbers
(SPEC 13.3) are still pending; if a gate fails, revisit this decision.

## Context
M1 needs one speech engine adapter and a default transcription model, chosen on measured
numbers, behind the model-folder + `model.json` mechanism (SPEC 16). Gates (SPEC 13.3): real-time
factor (both streams) at most 0.3 on Snapdragon X and 0.5 on Intel, RAM while transcribing at most
1.5 GB, a line visible within 2 s (3 s Intel) after an utterance ends.

## Decision
- **Engine:** the official `sherpa-onnx` Rust crate (2.0.1, Apache-2.0, safe wrapper over the C API),
  behind the existing `Engine` trait and `Registry`. Two engine families register today:
  `sherpa-onnx/offline-transducer` (Parakeet) and `sherpa-onnx/whisper`. Silero VAD is a separate
  `sherpa-onnx/vad-silero` pack. File names come from `model.json` `files` roles; nothing is hard-coded.
- **Default model (recommended): Parakeet TDT 0.6B v2, int8**, shipped as a `.sprkmodel` pack, with
  **Whisper base.en (int8)** as the documented fallback for any device that misses the memory or
  speed gate (it is 4x smaller on disk and uses less than half the RAM, but is not faster on the dev host and has higher WER).
- **Link mode:** static on Linux and Windows x64, shared (DLLs next to the exe) on Windows ARM64.
  See Consequences.

## Evidence (dev host: Linux x64, 16 threads available, decoding capped at 4 threads)
Informational only: SPEC 13.3 says dev-host speed is not a gate. Accuracy is host-independent.
Text accuracy used 3 LibriSpeech clips (public-domain book text, 77 words) and 12 synthetic
espeak-ng sentences (122 words), all through the Silero VAD path. The sets are tiny, so
differences of a word or two are noise; treat the table as a sanity check, not a ranking.

| Model | Size on disk | Peak RSS (1 stream) | LibriSpeech WER | Synthetic WER | RTF (dev host) |
|---|---|---|---|---|---|
| Parakeet TDT 0.6B v2 int8 | 631 MiB | 1,144 MiB | 3.9% | 4.1% | 0.04 |
| Whisper small.en int8 | 359 MiB | 1,069 MiB | 2.6% | 5.7% | 0.17 to 0.20 |
| Whisper base.en int8 | 154 MiB | 486 MiB | 5.2% | 7.4% | 0.06 to 0.07 |

Windows x64 in the KVM VM (4 vCPU) reproduced the Parakeet accuracy exactly (3.9%), with RTF 0.08 to 0.09.
Live capture (44.1 kHz stereo to 16 kHz mono, VAD, Parakeet) transcribed 3 of 3 synthetic sentences correctly
on Linux; WASAPI device enumeration works on Windows via the same code path.

## Why Parakeet
- Fastest of the three on the dev host: 4 to 5x faster than Whisper small.en at similar accuracy, and ahead of base.en, so it has the most headroom for two concurrent streams.
- Native punctuation and casing, which the transcript format needs; no separate punctuation model.
- English-only, which matches v1 scope.

## Risks and open items (need the pilot laptops)
1. **Memory.** 1.14 GB for one stream is inside the 1.5 GB gate, but two model instances would not be.
   The pipeline must share one recognizer between the Me and Others streams (serialize decode on one
   worker) rather than load one per stream.
2. **Speed on Snapdragon and Intel** is unmeasured. Run the commands below and fill in the results.
3. **Size.** The Parakeet pack is about 460 MiB compressed, 631 MiB installed. SPEC 15 asks whether a
   default ships inside the installer; that choice (bundle Parakeet, bundle Whisper base.en, or ship the app small and
   let admins add the pack) is a product decision for the owner.
4. **Licence.** Parakeet TDT 0.6B v2 is CC-BY-4.0: the app needs an attribution notice. Whisper is MIT.
5. LibriSpeech clips are CC-BY-4.0 audio; they are used from the model tarball cache and never committed.

## Pending: laptop measurements (mark results here)
Copy the portable folder, then on each laptop (PowerShell):
```
.\sprkctl.exe bench accuracy models\parakeet-tdt-0.6b-v2-int8 fixtures --vad models\silero-vad
.\sprkctl.exe bench accuracy models\whisper-base.en fixtures --vad models\silero-vad
```
Record `RTF` and peak working set (Task Manager, Details, `sprkctl.exe`). Both streams roughly double the
decode load, so the gate to compare against is `RTF x 2 <= 0.3` (Snapdragon) or `<= 0.5` (Intel).

| Machine | Model | RTF | Peak RAM | Result |
|---|---|---|---|---|
| Snapdragon X (ARM64) | Parakeet | pending | pending | pending |
| Snapdragon X (ARM64) | Whisper base.en | pending | pending | pending |
| Intel (x64) | Parakeet | pending | pending | pending |
| Intel (x64) | Whisper base.en | pending | pending | pending |

## Alternatives
- **Whisper small.en:** best LibriSpeech WER here, but about 4x slower than Parakeet and similar memory.
- **Whisper base.en as default:** smallest and quickest to ship, but weakest accuracy.
- **sherpa-rs (community wrapper):** rejected in favour of the official crate, same C API, one fewer dependency owner.

## Consequences
- The sherpa adapter is behind off-by-default cargo features so plain `cargo test --workspace` needs no
  native libs. Build it with `SHERPA_ONNX_LIB_DIR` pointing at libs from `scripts/fetch-sherpa.sh`.
- **Windows ARM64 uses shared (DLL) linking.** The prebuilt static ARM64 libs reference MSVC STL helpers
  (`__std_find_trivial_1` and others) that the cargo-xwin CRT only provides for ARM64EC, so the static link
  fails from Linux. The shared build links import libs only and works; the DLLs (onnxruntime, sherpa-onnx-c-api,
  about 22 MiB) ship beside the exe. Revisit static linking if a newer xwin CRT fixes it.
- `scripts/xwin-case-links.sh` adds a `PathCch.lib` link that the xwin SDK cache (lowercase names) lacks.

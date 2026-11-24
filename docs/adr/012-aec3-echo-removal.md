# ADR-012: WebRTC AEC3 on the microphone, and a loopback clock that keeps time

Status: **accepted** (alpha). Supersedes the software canceller part of ADR-004.

## Context
A work laptop on speakers transcribed a YouTube video as "Me". Two causes were found in the code:

1. **Layers 1 and 2 of SPEC 4.2 were never in the live path.** `NlmsCanceller` existed but nothing called it, so only
   transcript dedupe (layer 3) ran.
2. **Dedupe compared lines on different clocks.** Each stream is timestamped by counting its own samples. WASAPI
   loopback delivers no packets while nothing is playing, so the system stream's clock fell behind the microphone's
   (about 50 s in the report: Me at 1:28, Others at 0:36 for the same audio). Dedupe looks for Others lines within
   plus or minus 2 s, so it never matched.

## Options weighed
- **Denoise the mic (RNNoise, DTLN, Windows noise suppression).** Does not help: bleed is clean speech, and a
  denoiser keeps speech. Also changes audio the transcription model should hear untouched.
- **Windows communications-mode AEC** (`IAcousticEchoCancellationControl`, SPEC layer 1). Good when the driver
  supports it, but depends on the audio driver and Windows build, cannot be tested on the Linux host, and gives no
  signal about whether it worked. Kept as a later optional layer.
- **Speaker embeddings** ("is this my voice?"). Needs an extra model, enrollment, and fails on the user's own cold or
  a second person in the room. Not needed once echo is removed. Revisit if needed.
- **Energy or correlation gating only.** Cheap but crude: it mutes the user when they talk over playback.
- **Speex MDF / NLMS.** Older, weaker on delay drift and nonlinear speaker distortion. The existing NLMS has no delay
  search and costs a shift of its whole history per sample.
- **WebRTC AEC3** (what Chrome, Teams-class and Zoom-class clients are built on): delay estimator, partitioned-block
  frequency-domain adaptive filter, echo audibility and a nonlinear suppressor. A maintained pure-Rust port,
  `sonora` (BSD-3-Clause), builds for every target with no C++ toolchain, so ADR-004's concern about cross-compiling
  WebRTC for ARM64 Windows does not apply.

## Decision
- Run AEC3 (echo canceller only; no noise suppression, no gain control, no resampling) on the microphone stream with
  the loopback stream as the reference, through `sprk_audio::echo::EchoPath`, owned by `Session`. 10 ms frames at 16 kHz.
  Mic audio waits at most 300 ms for its reference, then is processed without one, so a missing reference never
  stalls the mic. Active only while both sources are on; restarted on pause, resume and source toggles.
- `LiveCapture` pads a stream that falls more than 400 ms behind the wall clock with silence, so both streams share a
  timeline and dedupe overlaps line up.
- Dedupe (layer 3) stays as the backstop for anything the canceller leaves.
- No new setting (CLAUDE.md rule 8). Headsets need no special case: with no echo there is nothing to cancel.

## Consequences
- Synthetic tests (`sprk_audio::echo`): 43 dB echo reduction with a 100 ms path and a distorted speaker model; 86%
  of the user's speech kept when talking over playback; a silent reference leaves the mic untouched.
- CPU is small (partitioned FFT filter, a few percent of one core). Must still be measured on the Snapdragon and Intel
  laptops (SPEC 13.3 gates).
- Real-room performance is unproven until the bleed test (SPEC 13.2) runs on laptops.
- A tab or app that plays through a different output than the one looped back is not heard by the reference and is
  still caught only by dedupe.

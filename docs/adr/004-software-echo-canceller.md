# ADR-004: Software echo canceller fallback

Status: **accepted** (pilot). Revisit before 1.0 if the bleed test (SPEC 13.2) fails on the Snapdragon laptop.

## Context
SPEC 4.2 layer 2 names WebRTC Audio Processing (AEC3) as the fallback when the render endpoint has no
Windows AEC control. WebRTC APM needs a C++ toolchain per target, and cross-compiling it for
`aarch64-pc-windows-msvc` from the Linux dev host is unproven. Transcript dedupe (layer 3) is always on and
catches bleed that survives layers 1 and 2.

## Decision
Ship a pure-Rust NLMS adaptive-filter canceller (`sprk_audio::aec::NlmsCanceller`) behind the existing
`EchoCanceller` trait, selected by `aec::plan()` (headset: dedupe only, Windows AEC if available, else software).
It has a double-talk guard so near speech is not trained away. WebRTC APM can be added later as another
adapter behind the same trait (SPEC 17) with its own ADR, with no pipeline change.

## Consequences
- Portable and fully unit-tested on Linux (echo removal over 20 dB, near speech preserved).
- Filter length (default 256 to 2048 taps) trades echo path length against CPU; tune on the laptops.
- Real-room performance is unproven until the bleed test runs on hardware.

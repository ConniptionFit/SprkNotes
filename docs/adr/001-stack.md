# ADR-001: Stack

Status: accepted (M0)

## Context
SprkNotes must transcribe two audio streams live on Windows, on-device, on ARM64 (Snapdragon) and x64, in a small, quiet app for a HIPAA-regulated company. See SPEC.md 3.1.

## Decision
- **Rust** core as a Cargo workspace, **Tauri v2** shell (WebView2), **Svelte 5 + TypeScript + Vite** UI with plain CSS.
- **sherpa-onnx** (Apache-2.0) as the first speech engine family, behind the `Engine` trait in `sprk-asr`.
- Every Windows API sits behind a trait with a `cfg(windows)` implementation and a fake (SPEC 3.4).

## Alternatives
- Rust + Slint/egui: lighter RAM, slower UI iteration. Fallback if WebView2 memory becomes a problem.
- C# + WinUI 3: heavier runtime and Windows App SDK deployment friction.
- Electron: too heavy.

## Consequences
- Native-speed audio and ASR, first-class ARM64, small binaries.
- `app/src-tauri` needs WebView2, so it is excluded from the Linux workspace build (see ADR-002).
- Reversing the UI layer later is costly; reversing the engine is cheap (trait + model packs).

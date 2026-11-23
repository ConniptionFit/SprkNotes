# CLAUDE.md: SprkNotes

SprkNotes is a lightweight, on-device Windows app that transcribes microphone and system audio together, live, into crash-safe Markdown files. It is built for a HIPAA-regulated company. **SPEC.md is the source of truth.** Read it before starting any task.

---

## Hard rules (never break these)

1. **On-device only.** Audio is captured, processed and transcribed locally. Audio samples live in memory only. Never write audio to disk (no WAV, no temp files, no debug dumps).
2. **No network.** SprkNotes makes no outbound network connections, with two exceptions (ADR-005 models, ADR-011 app updates; see below): https GETs for **model packs only** (the model index at launch, and packs when the user presses "Download model", for speech or for Ask Sprk), verified by checksum and the normal import rules, switchable off by policy (`ModelDownload=0`). sprkctl's other exception is the admin-run `model sync`, which reads an internal file share. No Entra app, no Microsoft Graph, no external APIs. Allowed local interfaces only:
   - Outlook COM (classic Outlook) for the calendar.
   - The Teams local API on `localhost`, probed silently at session start; any failure is silent and falls back.
   - The AI handoff, which only launches Claude/Copilot (link or clipboard); the user sends from that app.
   No telemetry, no analytics, no crash upload. App updates (ADR-011) add exactly one more https use: the release list, a signed update manifest and the installer MSI from the configured release repository, switchable off by policy (`UpdateMode=off`) and never during a session. No CDN fonts or scripts in the UI.
3. **Terminology.** User-facing strings say "transcribing", "transcript", "paused". Never "recording", "record", "recorded". This includes tooltips, logs shown to users, installer text, README and the admin guide. A CI check enforces this (see SPEC.md 13.1).
4. **No PHI anywhere outside transcript files.** Logs never contain transcript text, meeting subjects, attendee names or meeting bodies. Log IDs and counts only.
5. **Public repo hygiene.** No real names, no tenant or client IDs, no certificates, no secrets. Test audio and sample transcripts are synthetic or public-domain only.
6. **Crash safety.** Every committed transcript line is appended and flushed to disk immediately. Nothing may require a clean shutdown to save the user's work.
7. **Version alignment.** The workspace version in the root `Cargo.toml` is the only version source. MSIX, MSI and portable builds are always produced together by `scripts/release.ps1`. Never bump one artifact alone.
8. **Minimal UI.** Do not add controls, settings or screens that SPEC.md does not call for. Propose them instead.
9. **Models are data, not code.** Never hard-code a model path, name or file. The pipeline asks the resolver (SPEC.md 16.4) for the active model. Models arrive only as `.sprkmodel` packs with a verified `model.json`. Packs never contain or run executable code.
10. **No model swap mid-session.** A new or rolled-back model takes effect at the next session start. A failed model load falls back to the previous good model; transcription must keep working.
11. **Context data is PHI-grade.** Audio source titles, window/tab titles and participant names go only into transcript files. Never log them, never put them in errors or telemetry. Honor `AudioSourceExcludePatterns` before anything is stored.
12. **Read-only inspection of other apps.** Audio session metering, media session info, window titles and UI Automation are read-only. Never inject input, click, or alter another app. UI Automation is used only on browser tab strips (audio source titles). Never automate Teams.
13. **When unsure who spoke, say Others.** Never guess a remote speaker's name. Named labels only in confirmed 1:1 calls (SPEC.md 19.4). Never rewrite already-committed lines.
14. **Model signing is a 1.0 release gate.** Pilot builds may use the hash allow-list (SPEC.md 16.10). Do not tag or package 1.0 unless Ed25519 (OpenSSH SSHSIG) signature enforcement is implemented and tested. Signing goes through the 1Password SSH agent; code never reads or stores a private key.
15. **Extend through interfaces.** New engines, exporters, calendar providers, filters or handoff targets are added as adapters behind the traits in SPEC.md 17, each with an ADR. Do not branch on specific models or vendors inside the pipeline.

Wording: call speech models "transcription models", not "LLMs". The on-device assistant (ADR-009) is called the "assistant" and its models "assistant models". Its prompts, questions, answers and titles are PHI-grade: never log them, keep question history in memory only.

---

## Stack

| Layer | Choice |
|---|---|
| Core | Rust (stable), Cargo workspace |
| App shell | Tauri v2 (WebView2) |
| UI | Svelte 5 + TypeScript + Vite, plain CSS, no component library |
| Windows APIs | `windows` crate (windows-rs): WASAPI, AEC control, COM (Outlook), WinRT |
| Speech | sherpa-onnx (official Rust API or `sherpa-rs`), ONNX Runtime, VAD + offline English model (see SPEC.md 4) |
| Echo cancellation | Windows AEC (`IAcousticEchoCancellationControl`), WebRTC APM fallback |
| Models | `.sprkmodel` packs (zip + `model.json` + Ed25519 SSHSIG `model.sig`, required from 1.0), managed by `sprk-models` crate and `sprkctl` CLI (SPEC.md 16) |
| Context | `sprk-context`: WASAPI session enumeration + metering, Global System Media Transport Controls, UI Automation (browser tabs only), window enumeration, Teams local API on localhost (SPEC.md 18, 19) |
| Exports | Markdown (native), TXT, JSON, DOCX (`docx-rs`) |
| Packaging | MSIX bundle (x64 + arm64), WiX v5 MSI per arch + PowerShell installer, portable zip |
| Targets | `aarch64-pc-windows-msvc` (primary), `x86_64-pc-windows-msvc` |

Change any of these only through an ADR in `docs/adr/`.

---

## Repo layout

See SPEC.md 3.3. Keep crate boundaries clean: crates under `crates/` must not depend on Tauri. Only `app/src-tauri` talks to the UI. `tools/sprkctl` and the app share `sprk-models`; model logic lives there, never duplicated.

---

## Development environment

| Machine | Role | What runs there |
|---|---|---|
| **Dev host:** CachyOS (Arch-based Linux), x86_64 | Where Claude works | Edit code; build and test portable crates natively; cross-compile both Windows targets with `cargo-xwin`; speech engine accuracy tests (sherpa-onnx runs on Linux) |
| **`sprk-winvm`:** Windows 11 x64 VM on the dev host (KVM/QEMU, virtio, PipeWire audio) | Windows integration tests | WASAPI capture, AEC, Outlook COM, Teams local API, window titles, Tauri UI, packaging (WiX, MSIX). Reached over SSH (Windows OpenSSH server) |
| **Snapdragon laptop** (ARM64) | Manual checks | Run the ARM64 portable build each milestone; performance gates (13.3). No Claude, no dev tools needed |
| **Intel laptop** (x64) | Manual checks | Performance gates (13.3) |

Rules:
- **No ARM64 emulation.** ARM64 is cross-compiled on the dev host every milestone and only ever run on real Snapdragon hardware.
- **Performance numbers from the dev host or VM don't count.** Only the laptops measure 13.3 gates.
- **Synthetic audio only** on the dev host and VM. Never real meetings.
- The VM is a KVM virtual machine (hardware virtualization), not emulation, so x64 Windows tests run at near-native speed.

---

## Commands

```bash
# --- Dev host (CachyOS) ---
# Portable crates: build, test, lint (Windows-only code is behind cfg(windows) and skipped here)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check

# Cross-compile both Windows targets (cargo-xwin downloads the MSVC CRT/SDK on first run)
cargo xwin build --release --target x86_64-pc-windows-msvc
cargo xwin build --release --target aarch64-pc-windows-msvc

# Engine accuracy on fixtures (Linux build of sherpa-onnx; speed numbers here are NOT gates)
# First run fetches native libs and models into ~/.cache/sprknotes (scripts/fetch-sherpa.sh, fetch-models.sh)
./scripts/bench.sh --accuracy-only

# Transcribe a WAV or live input from a model folder (engine needs SHERPA_ONNX_LIB_DIR; see docs/dev-environment.md)
cargo run --release -p sprkctl --features engine,live -- transcribe <model-folder> <wav> --vad <vad-folder>

# --- Windows VM (one command from the dev host) ---
# Syncs the current branch to sprk-winvm and runs the Windows test suite there
./scripts/winvm.sh test            # cargo test --workspace on Windows
./scripts/winvm.sh run-app         # npm run tauri dev inside the VM (view via the VM console)
./scripts/winvm.sh release         # pwsh scripts/release.ps1 (all artifacts, one version)

# Model packs (portable; machine scope must run in the VM elevated)
cargo run -p sprkctl -- model pack ./models-src/<id> -o ./dist/<id>@<ver>.sprkmodel
cargo run -p sprkctl -- model verify ./dist/<id>@<ver>.sprkmodel
cargo run -p sprkctl -- model list --json
```

`scripts/winvm.sh` is built in M0: it pushes the working branch to the VM over SSH (git bundle or push to a bare repo on the VM), runs the command there, and streams output back. Update this section whenever a command changes.

---

## Coding standards

- **Rust:** `clippy -D warnings`, `rustfmt`. No `unwrap()`/`expect()` outside tests and `main` setup. `thiserror` in libraries, `anyhow` at the app edge. `tracing` for logs, with a redaction layer.
- **Threads:** audio capture runs on dedicated real-time threads. No allocation or locking on the audio callback path; use lock-free ring buffers. ASR runs on its own worker pool.
- **Panics:** a panic hook flushes the transcript writer and marks the session interrupted before exit.
- **Unsafe:** only inside `windows`-crate wrappers, each block commented with why it is sound.
- **Platform seam:** every Windows API (WASAPI, AEC, COM, window enumeration, media sessions, UI Automation, Teams local API, registry, Credential Manager) sits behind a trait with a `cfg(windows)` implementation and a fake. All logic (pipeline, dedupe, writer, naming, labeling modes, audio-source timing, model resolver) must be testable on the Linux dev host using the fakes.
- **UI:** small Svelte components, no global state library. All Tauri commands are typed on both sides.
- **Tauri security:** strict CSP, minimal capabilities per window, no remote URLs loaded, `shell`/`fs` access scoped to the transcripts root.

---

## Workflow

1. Work one milestone at a time from SPEC.md 14. Stop at its acceptance criteria and report results.
2. Before code, write a short plan: files to touch, risks, how you will test.
3. Tests first where practical (format, writer, dedupe, naming and recovery logic are all pure and testable).
4. Any decision not already in SPEC.md gets an ADR. Any behavior change updates SPEC.md in the same commit.
5. Small commits, Conventional Commit messages (`feat(audio): ...`, `fix(writer): ...`).
6. Never claim something works without running it. Report the command and its output.

## Definition of done

- Linux host: tests and clippy pass; both Windows targets cross-compile with `cargo xwin`.
- Windows VM: `./scripts/winvm.sh test` passes for anything touching Windows APIs.
- ARM64 portable build handed off for the manual Snapdragon check when the milestone touches audio, UI or performance.
- Acceptance criteria in SPEC.md for the milestone are demonstrated.
- No hard rule broken (terminology check, no-audio-to-disk check, log redaction check incl. titles and names, no hard-coded model paths, no guessed speaker names).
- SPEC.md, ADRs, README and this file are updated if anything changed.

# Architecture overview

```
 Mic (WASAPI, AEC) ─┐
                    ├─> 16 kHz mono ─> VAD ─> ASR ─> segments
 System (loopback) ─┘                                    │
                       Commit buffer (6 s): dedupe, fillers, merge, split
                                                         │
                      Transcript writer (append + flush .md, JSONL sidecar) ─> UI

 In-progress speech ─> Captioner (re-decode last 10 s, never stored) ─> `caption` events ─> overlay + live line
```

On Linux the same pipeline runs with `parec` (PipeWire or PulseAudio monitor) for Others and cpal for the microphone.

## Crates
| Crate | Role | Status |
|---|---|---|
| `sprk-transcript` | Format, writer, recovery, parser, exporters | Done (portable) |
| `sprk-pipeline` | Commit buffer, dedupe, rules, `Session` orchestrator, session host, `Captioner` (ADR-006, ADR-007, ADR-008) | Done (portable) |
| `sprk-asr` | `Engine` and `Vad` traits, sherpa-onnx adapters | Done |
| `sprk-models` | Packs, manifest, resolver, stores, import, signing | Done (portable) |
| `sprk-context` | Audio source timing, call title rules, labeling modes | Windows sources and call context wired; UI Automation tabs, Teams local API pending |
| `sprk-calendar` | Meeting selection, context section, classic Outlook (COM) provider | Done; real Outlook check pending |
| `sprk-handoff` | Claude/Copilot plans, prompt template, Windows launcher | Done; UI button pending |
| `sprk-audio` | Capture and AEC traits, cpal backend (WASAPI mic and loopback), Linux monitor capture via `parec`, software echo canceller | Done; hardware bleed test pending |
| `sprk-config` | Settings and admin policy (`HKLM`/`HKCU` Policies) | Done; UI lock indicators pending |
| `app/src-tauri`, `app/ui` | Tauri shell (commands, model download, tray, overlay window) and Svelte UI (main window, Settings, overlay) | Built; Windows and native Linux |
| `tools/sprkctl` | Admin CLI | Done for models (including `index` and split packs) and bench |

## Windows and UI
- **Main window** (`App.svelte`): session name, source switches, status light, transcript with live line, Start/Pause/End, Settings.
- **Overlay** (`index.html#mini`, same bundle): an orb with grip, light, clock, captions and hover actions, plus a caption drawer that flips above the orb in the lower half of the screen. The Rust shell sizes the window to its content and owns position, flip and tray (`shell.rs`).
- **Events** from the session host to the UI: `transcript-line`, `session-status`, `caption` (transient), `model-download`, `model-update`. The UI has a browser mock for each, which is how the documentation screenshots are produced (`scripts/screenshots.mjs`).
- **Model download** (ADR-005): portable index and pack logic in `sprk_models::remote`, with a WinHTTP client on Windows and a `ureq` client elsewhere.

## Platform seam
Every Windows API sits behind a trait with a `cfg(windows)` implementation and a fake, so all logic is tested on Linux. Windows-only code is verified in the VM. The app itself also builds and runs natively on Linux.

## Extension points
New engines, VADs, exporters, calendar providers, handoff targets and filters are adapters behind traits (SPEC 17), each with an ADR. The pipeline never branches on a specific model or vendor.

## Hard rules
See [CLAUDE.md](../CLAUDE.md): no audio on disk, no network, terminology ("transcribing"), no PHI in logs, crash safety, models are data, never guess a speaker.

## Decisions
See [docs/adr](adr/): stack (001), dev environment (002), default model (003), echo canceller (004), model download (005), live captions (006) and streaming caption hypotheses (007) and caption lock-in (008).

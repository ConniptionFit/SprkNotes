# Architecture overview

```
 Mic (WASAPI, AEC) ─┐
                    ├─> 16 kHz mono ─> VAD ─> ASR ─> segments
 System (loopback) ─┘                                    │
                       Commit buffer (6 s): dedupe, fillers, merge, split
                                                         │
                      Transcript writer (append + flush .md, JSONL sidecar) ─> UI
```

## Crates
| Crate | Role | Status |
|---|---|---|
| `sprk-transcript` | Format, writer, recovery, parser, exporters | Done (portable) |
| `sprk-pipeline` | Commit buffer, dedupe, rules, `Session` orchestrator | Done (portable) |
| `sprk-asr` | `Engine` and `Vad` traits, sherpa-onnx adapters | Done |
| `sprk-models` | Packs, manifest, resolver, stores, import, signing | Done (portable) |
| `sprk-context` | Audio source timing, call title rules, labeling modes | Portable core done; Windows sources pending |
| `sprk-calendar` | Meeting selection, context section | Portable core done; Outlook COM pending |
| `sprk-handoff` | Claude/Copilot plans, prompt template | Portable core done; launcher pending |
| `sprk-audio` | Capture and AEC traits | Traits and fakes; WASAPI pending |
| `sprk-config` | Settings and policy | Skeleton |
| `app/src-tauri`, `app/ui` | Tauri shell, Svelte UI | Main window built; commands pending |
| `tools/sprkctl` | Admin CLI | Done for models and bench |

## Platform seam
Every Windows API sits behind a trait with a `cfg(windows)` implementation and a fake, so all logic is tested on Linux. Windows-only code is verified in the VM.

## Extension points
New engines, VADs, exporters, calendar providers, handoff targets and filters are adapters behind traits (SPEC 17), each with an ADR. The pipeline never branches on a specific model or vendor.

## Hard rules
See [CLAUDE.md](../CLAUDE.md): no audio on disk, no network, terminology ("transcribing"), no PHI in logs, crash safety, models are data, never guess a speaker.

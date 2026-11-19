# SprkNotes Specification

**Status:** Draft v0.5 (v0.5: Linux dev host + Windows VM test environment, 13.4 and 3.4; v0.2: model management + extensibility, 16 and 17; v0.3: model trust tiers 16.10, audio sources 18, call context 19; v0.4: fully on-device, no Entra/Graph or external APIs, 1Password SSH signing, Teams roster reader removed) · **Owner:** John · **Repo:** Forgejo `SprkNotes` (public)

---

## 0. Summary

SprkNotes is a minimal Windows desktop app that transcribes the user's microphone ("Me") and the computer's audio output ("Others") at the same time, fully on-device. It writes the transcript live into a Markdown file so nothing is lost if the app crashes, removes duplicate lines caused by speaker sound bleeding into the mic, names files from the current Outlook meeting, and can hand the finished transcript to Claude or Microsoft 365 Copilot for a summary.

### Glossary

| Term | Meaning |
|---|---|
| Session | One start-to-end transcription run, producing one transcript file |
| Segment / statement | One line of transcript: a single coherent statement from one speaker |
| Me | Audio from the selected microphone |
| Others | Audio from system output (loopback) |
| Bleed | System audio played on speakers that the mic also picks up |
| Commit delay | Short hold before a segment is written to disk, so bleed checks can drop duplicates first |
| Transcription model | The speech-to-text (ASR) model. Not an LLM; it only converts speech to text |
| Model pack | A single `.sprkmodel` file (zip) holding model files plus a signed `model.json` manifest |
| Active model | The model the app loads at session start, chosen by `active.json` or admin policy |
| Audio source | An app playing sound through system output (e.g. Firefox tab, Teams), with its title and active duration |
| Merge window | How long an audio source may go silent and still count as the same continuous activity |
| Call context | Who the user is actually in a call with right now (not just who was invited) |
| Labeling mode | `named` (1:1 call: remote speaker shown by name) or `group` (3+ people or unknown: remote speakers shown as Others) |

**Wording rule:** the product says "transcribing", never "recording". Audio is never stored.

---

## 1. Goals and non-goals

### Goals
- Live, accurate English transcription of mic + system audio on Snapdragon X (Copilot+) laptops with 16 GB RAM.
- Crash-safe: loss on crash is limited to the last few seconds.
- Transcripts that read well for people and parse reliably for LLMs.
- Very small, quiet UI with a floating mini controller.
- Deployable by RMM (NinjaOne), Intune, or as a no-install zip, all from one release.
- Transcription models can be updated, rolled back and centrally managed **without an app update** (section 16).
- Built to grow: engines, exporters, calendar providers and AI handoff targets plug in through defined interfaces (section 17).
- Rich context for people and LLMs: which apps were heard (section 18) and who was actually on the call (section 19), with remote speakers named on 1:1 calls.
- Model packs are cryptographically signed and enforced by the 1.0 release (16.10).
- **No cloud dependencies:** no Entra app registration, no Microsoft Graph, no external APIs. Everything SprkNotes does runs on the device.

### Non-goals (v1)
- Storing audio of any kind.
- Identifying individual remote speakers (diarization). Labels are Me / Others only.
- Languages other than English.
- Built-in AI summarization (handled by the Claude / Copilot handoff).
- Cloud sync (handled by the Documents folder, which is OneDrive-backed).
- Auto-update (RMM handles updates).

---

## 2. Constraints

| Area | Constraint |
|---|---|
| Compliance | HIPAA. All audio and transcription processing on-device. PHI only ever lands in transcript files in the user's profile. |
| OS | Windows 11 23H2+ (AEC reference control needs build 22621+). |
| Hardware (primary) | Dell Snapdragon X, Copilot+, 16 GB RAM, ARM64. |
| Hardware (secondary) | Intel x64, 16 GB RAM, no dGPU. In scope if M1 benchmarks pass the gates in 13.3; otherwise documented as unsupported. |
| Privilege | Runs and installs at user level (no admin) for MSIX, per-user MSI and portable. |
| Network | **SprkNotes makes no outbound network connections** except model downloads (ADR-005): https GETs for the model index at launch and for model packs when the user presses "Download model", verified by checksum and the import rules, switchable off by policy (`ModelDownload`, `ModelUpdateCheck`, `ModelIndexUrl`). Local-only interfaces: Outlook COM (classic Outlook) and the optional Teams local API on `localhost`. The AI handoff only launches Claude/Copilot and copies a prompt; any upload happens in those apps, by the user. |
| Outlook | Mixed classic and new Outlook. Classic: calendar read locally via COM. New Outlook has no local programming interface, so those users get naming from call context and audio sources instead (6.2). |

---

## 3. Architecture

### 3.1 Stack decision (ADR-001)

**Rust core + Tauri v2 shell + Svelte UI + sherpa-onnx speech engine.**

| Option | Verdict |
|---|---|
| **Rust + Tauri v2** (chosen) | Native-speed audio and ASR, memory safety, small binaries, open source (MIT/Apache), first-class ARM64. UI is web tech in the OS's WebView2, which is quick to iterate on. |
| Rust + Slint / egui (pure native UI) | Lighter RAM, but slower UI iteration and harder rich text scrolling. Fallback if WebView2 memory becomes a problem. |
| C# .NET + WinUI 3 | Strong Windows integration, but heavier runtime, larger footprint, Windows App SDK deployment friction. |
| Electron | Too heavy. Rejected. |

Speech engine: **sherpa-onnx** (Apache-2.0) because it supports Windows ARM64 and x64, has Rust bindings, ships VAD, and supports Qualcomm NPU (QNN) acceleration for some English models. The engine sits behind a trait so Whisper (whisper.cpp) or another engine can be swapped in.

### 3.2 Data flow

```
 Mic (WASAPI capture, AEC on, reference = default render)  ─┐
                                                            ├─> resample 16 kHz mono ─> VAD ─> ASR worker ─> segments
 System (WASAPI loopback of default render)                ─┘                                         │
                                                                                                      v
                                              Commit buffer (default 6 s): bleed dedupe, filler filter, sentence split
                                                                                                      │
                                       ┌──────────────────────────────────────────────┬───────────────┘
                                       v                                              v
                          Transcript writer (append + flush .md,               UI live pane
                          JSONL sidecar)                                      (Tauri events)
```

### 3.3 Repo layout

```
sprknotes/
  CLAUDE.md  SPEC.md  README.md  CHANGELOG.md  LICENSE
  Cargo.toml                # workspace; version source of truth
  crates/
    sprk-audio/             # WASAPI mic + loopback, device events, resample, AEC control, APM fallback
    sprk-asr/               # Engine trait + adapters (sherpa-onnx families), VAD
    sprk-models/            # model packs: manifest, verify, import, resolve active, rollback, smoke test
    sprk-context/           # audio source detection (section 18), call context + participants (section 19)
    sprk-pipeline/          # session orchestration, commit buffer, dedupe, speaker labels, pause/resume
    sprk-transcript/        # format model, md writer, recovery, exporters (txt, json, docx)
    sprk-calendar/          # Outlook COM provider (classic Outlook), meeting picker logic
    sprk-handoff/           # Claude + Copilot handoff, prompt templates
    sprk-config/            # user settings + HKLM/HKCU policy overrides
  app/
    src-tauri/              # Tauri app: commands, events, windows (main, mini), tray
    ui/                     # Svelte 5 + TS
  tools/
    sprkctl/                # admin CLI: model pack/verify/install/list/activate/rollback/bench/sync
  packaging/
    msix/  wix/  portable/  ninja/  intune/  models/
  scripts/
    release.ps1 (Windows VM)  bump-version.sh  bench.sh  check-terms.sh  winvm.sh
  docs/adr/
  tests/fixtures/           # synthetic / public-domain audio only
```

### 3.4 Platform seam (testability on a Linux dev host)
- Every Windows-specific API is wrapped behind a trait in its crate, with a `cfg(windows)` implementation and an in-memory fake (`AudioCapture`, `EchoCanceller`, `CalendarProvider`, `WindowEnumerator`, `MediaSessionSource`, `TeamsLocalApi`, `PolicyStore`, `SecretStore`).
- Portable logic (pipeline, commit buffer, dedupe, transcript writer, recovery, naming chain, labeling modes, audio-source timing, model resolver and verification) compiles and is fully tested on Linux using the fakes and recorded event scripts.
- The whole workspace must cross-compile for `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc` from Linux (`cargo-xwin`). Packaging (WiX MSI, MSIX) runs only on Windows.

---

## 4. Audio and transcription pipeline

### 4.1 Capture
- **Mic:** WASAPI shared-mode capture on the user-selected input (default: system default communications device).
- **System:** WASAPI loopback on the default render device. Follow default-device changes (headset plugged in, Bluetooth connects) without stopping the session.
- Either source can be toggled off. Labels: mic = **Me**, loopback = **Others**, or the remote person's name on a confirmed 1:1 call (section 19).
- Convert to 16 kHz mono f32 in the pipeline. Audio stays in in-memory ring buffers only.

### 4.2 Bleed removal (three layers)
1. **Windows AEC:** open the mic stream as a communications stream; if `IAcousticEchoCancellationControl` is available, set the render endpoint being looped back as the AEC reference.
2. **Software AEC fallback:** if the endpoint does not support AEC control, run WebRTC Audio Processing (AEC3) on the mic using the loopback stream as the far-end reference.
3. **Transcript dedupe (always on):** in the commit buffer, drop a Me segment when an Others segment overlaps it in time (±2 s) and normalized text similarity is ≥ 0.6 (token-level ratio; threshold configurable). Prefer the Others version.
- **Headset detection:** if the render endpoint form factor is headphones/headset, layers 1 and 2 may be skipped; layer 3 stays on.
- **Known limitation:** people speaking in the same room as the user are labeled Me.

### 4.3 Segmentation and ASR
- VAD (Silero or TEN-VAD via sherpa-onnx) splits each stream into utterances.
- Each utterance is decoded by the **active transcription model** (section 16), an offline English model with punctuation and casing. The app never hard-codes a model; it loads whatever the resolver selects. Candidates to benchmark in M1: **Parakeet TDT 0.6B (int8)**, **Whisper small.en / base.en**, plus a streaming model for partials if needed.
- **Partials (optional, default on if budget allows):** re-decode the in-progress utterance about every 0.8 s and show it in muted text at the bottom of the live pane. Partials are never written to disk.
- **Statement rules:**
  - One line = one coherent statement from one speaker.
  - Utterances over ~40 words are split at sentence boundaries.
  - Standalone fillers ("um", "uh", "mm-hmm") are dropped by default (setting).
  - Consecutive short fragments from the same speaker within 1 s are merged.
- **NPU acceleration (v1.1):** evaluate sherpa-onnx QNN models on Snapdragon. CPU int8 is the v1 baseline.

### 4.4 Commit buffer
- Finalized segments wait **6 s** (configurable 3 to 15 s) for dedupe and merge, then commit in timestamp order.
- On pause, end, or panic: flush the buffer immediately.

### 4.4a Live captions

Live captions (ADR-006) show speech in progress in the overlay. They are transient: never written to the transcript, logged or stored, and the committed lines (4.4) are unchanged. The last 10 s of audio per wanted source is re-decoded about every 0.7 s (Others by default, Me optional); the decode interval backs off when slow, and captions switch themselves off if the engine cannot keep up. Default off until the overlay's captions toggle turns them on.

### 4.5 Pause / resume / end
- **Pause:** capture stops, buffer flushes, a pause event line is written.
- **Resume:** capture restarts, a resume event line is written. Timestamps are wall-clock elapsed since start, so gaps stay visible.
- **End:** flush, finalize file (6.3), show the name/save confirmation.

---

## 5. Transcript format (`sprknotes-transcript/1`)

### 5.1 Markdown (default, source of truth)

```markdown
---
format: sprknotes-transcript/1
title: "Weekly IT Sync"
date: 2026-11-18
start: "2026-11-18T13:00:04-06:00"
end: "2026-11-18T13:42:15-06:00"
duration: "00:42:11"
status: complete            # in-progress | complete | interrupted
sources: [mic, system]
speakers: [Me, Sam Sample, Others]
name_source: calendar           # calendar | call | audio-source | default | user
meeting:
  subject: "Weekly IT Sync"
  platform: teams               # teams | zoom | other | none
  organizer: "Alex Example"
  invitees: ["Sam Sample", "Jordan Test"]
  location: "Microsoft Teams Meeting"
  scheduled: "13:00 to 13:45"
call:
  provider: teams
  participants:                 # identified 1:1 participants, from call context (section 19)
    - { name: "Sam Sample", joined: "00:00:00", left: "00:42:11" }
  group_periods:                # times the call had 3+ people (others not identified)
    - { start: "00:20:05", end: "00:31:40" }
audio_sources:                  # apps heard through system audio (section 18)
  - { app: "Microsoft Teams", title: "Weekly IT Sync | Microsoft Teams", start: "00:00:02", end: "00:42:09", duration: "00:42:07" }
  - { app: "Firefox", title: "Group rules overview - YouTube", start: "00:22:10", end: "00:25:40", duration: "00:03:30" }
engine: "parakeet-tdt-0.6b-int8"
app: "SprkNotes 0.1.0"
---

# Weekly IT Sync
**Wed, Nov 18, 2026 · 1:00 PM to 1:42 PM · 42 min · Me + Sam Sample (group call 00:20 to 00:31)**

## Meeting context
**Organizer:** Alex Example
**Invitees:** Sam Sample, Jordan Test
**Agenda (from Outlook):**
> Review group rule changes.
> Plan the next pilot.

## Participants
| Name | Joined | Left |
|---|---|---|
| Sam Sample | 00:00:00 | 00:42:11 |

**Group call:** 00:20:05 to 00:31:40 (additional participants not identified)

## Audio sources
| App | Title | Start | End | Duration |
|---|---|---|---|---|
| Microsoft Teams | Weekly IT Sync \| Microsoft Teams | 00:00:02 | 00:42:09 | 42:07 |
| Firefox | Group rules overview - YouTube | 00:22:10 | 00:25:40 | 3:30 |

## Transcript

- `00:00:04` **Sam Sample:** Let's get started with the group rule changes.
- `00:00:09` **Sam Sample:** Did the new rule go out this morning?

- `00:00:12` **Me:** It did, and the first sync finished without errors.

> `00:14:02` Transcription paused
> `00:16:40` Transcription resumed

- `00:16:43` **Sam Sample:** Great, next topic is the pilot.

> `00:20:05` Call became a group call: remote speakers labeled Others
- `00:20:12` **Others:** Hi both, sorry I'm late.

> `00:22:10` Audio source started: Firefox "Group rules overview - YouTube"
> `00:25:40` Audio source ended: Firefox "Group rules overview - YouTube" (3:30)

> `00:31:40` Back to 1:1 with Sam Sample: remote speaker labeled Sam Sample
- `00:31:45` **Sam Sample:** Okay, back to the pilot.
```

**Grammar (parsers rely on this):**
- Statement line: `- \`HH:MM:SS\` **<Speaker>:** <text>` (one statement, one line, no line breaks inside). Speaker is `Me`, `Others`, or a participant display name (section 19).
- Event line: `> \`HH:MM:SS\` <event text>`. Event types: paused, resumed, interrupted, source changed, call started/ended, labeling mode changed (1:1 / group), audio source started/ended.
- A blank line is inserted when the speaker changes (readability; parsers ignore blank lines).
- Front matter is YAML. `format` is versioned; any breaking change bumps it. The `call`, `audio_sources` and `name_source` fields and the Participants / Audio sources sections are additive in format 1.
- Meeting context section appears only if the user accepted it (7.3). Participants and Audio sources sections are written at finalization (6.3) from the events already in the file, so a crash loses none of that data.

### 5.2 Other exports
| Format | Content |
|---|---|
| `.txt` | Header block, then `[HH:MM:SS] Speaker: text` lines |
| `.json` | `{ format, meta{...}, meeting{...}, segments[{id, start_ms, end_ms, speaker, source, text}], events[{t_ms, type}] }` |
| `.docx` | Title, meta table, meeting context, transcript with timestamp + bold speaker per paragraph |

Exports are generated on demand from the `.md` file plus the JSONL sidecar.

---

## 6. Files, naming and crash safety

### 6.1 Location
- Root: `%USERPROFILE%\Documents\SprkNotes\` resolved with the Known Folder API (`FOLDERID_Documents`) so OneDrive folder redirection is respected.
- Transcripts: `...\SprkNotes\Transcripts\` (user subfolders allowed).
- Sidecars: `...\SprkNotes\Transcripts\.sprknotes\<session-id>.jsonl` (hidden).
- Root is configurable by the user and overridable by admin policy (10.2).

### 6.2 Naming
- Pattern: `YYYY-MM-DD HHmm <Title>.md`.
- **Title fallback chain** (first match wins; recorded as `name_source`):
  1. **Calendar:** matched meeting subject (Outlook, Teams or Zoom meetings on the Exchange calendar, section 7).
  2. **Call:** live call context (section 19): 1:1 call -> `Call with <Name>`; group call -> the Teams call window title.
  3. **Audio source:** the longest audio source detected so far (section 18), e.g. `Group rules overview - YouTube`.
  4. **Default:** `Transcript`.
- Lower-priority names are provisional: the file is renamed automatically when a better source appears (for example, an audio source is found 40 s in). **A name the user typed always wins** and stops automatic renaming (`name_source: user`).
- Sanitize Windows-invalid characters, trim to 120 characters, add ` (2)` on collision.
- The name is editable in the main window at any time during the session. Renaming moves the open file atomically (file opened with `FILE_SHARE_DELETE`).

### 6.3 Write strategy
- **Start:** create the file with front matter (`status: in-progress`), heading, optional meeting context, and `## Transcript`.
- **During:** each committed segment is appended and flushed (`FlushFileBuffers`). Same for the JSONL sidecar.
- **End:** rewrite front matter (end, duration, `status: complete`) by writing a temp file and atomically replacing (`ReplaceFileW`). This is the only time the file is rewritten.
- **Max loss on crash:** the commit delay plus the in-progress utterance (target ≤ 10 s).

### 6.4 Recovery
- On launch, scan for `status: in-progress` files not owned by a live session.
- Prompt once: "SprkNotes closed unexpectedly. The transcript up to that point was saved." Options: **Open**, **Mark finished**.
- Marking finished appends `> \`HH:MM:SS\` Transcription interrupted` and sets `status: interrupted`.

### 6.5 Crash dumps
- Disable full/heap Windows Error Reporting dumps for the app (memory may hold audio or text). Panics log a redacted message only.

---

## 7. Calendar and meeting integration: Outlook, Teams, Zoom (optional, off by default)

Scheduled Teams and Zoom meetings live on the user's Outlook calendar, so one local calendar integration covers all three. Live call context (including unscheduled Teams calls) is section 19. **No Entra app, no Graph, no network calls.**

### 7.1 Provider: classic Outlook via COM (local only)
- `Outlook.Application > GetNamespace("MAPI") > GetDefaultFolder(olFolderCalendar)`, `Items.IncludeRecurrences = true`, sorted by `[Start]`, `Restrict` to the window now-12 h to now+15 min.
- Fields: `Subject`, `Start`, `End`, `Organizer`, `Recipients` (required/optional), `Location`, `Body`, `MeetingStatus` (skip cancelled), `AllDayEvent`, `BusyStatus` (skip free).
- Reads the locally cached Exchange calendar; works offline. Only used if classic Outlook is installed with a profile. Never launches Outlook UI; if Outlook is not running, COM starts it hidden, and SprkNotes releases it afterward.
- **Platform detection:** a `teams.microsoft.com` join link in Location/Body -> Teams; `zoom.us` -> Zoom; otherwise other/none. Stored as `meeting.platform`.
- **New Outlook:** has no COM or local API, so the provider reports "unavailable" silently and the naming chain (6.2) falls through to call context and audio sources.
- Setting: `calendar.provider = com | off`. Kept behind the `CalendarProvider` trait (17.2) so another local source can be added later.
- The Outlook object-model security prompt does not appear when Windows Defender (or another registered antivirus) is current; if it ever appears, the provider backs off and reports unavailable.

### 7.2 Meeting selection (on Start)
1. Skip cancelled, all-day and "free" events.
2. Prefer the meeting in progress (most recent start). Otherwise the soonest one starting within 15 minutes.
3. If several match, the name field shows a small dropdown to pick one.
4. No match: fall through the title chain in 6.2 (call context, then audio source, then `Transcript`).
5. Calendar matching runs at Start and again every 5 minutes while transcribing, so a meeting that begins mid-session can still be offered (never auto-renames a user-typed name).

### 7.3 Meeting context prompt
- When a meeting is matched, show a non-blocking banner: "Add meeting details from Outlook to this transcript?" **Add** / **Skip** (+ "Always" / "Never" options in settings).
- Accepted details: subject, organizer, invitees, location, scheduled time, full body (as text).
- If answered after the file started, the context is inserted at finalization (6.3).

---

## 8. User interface

**Principle:** one main window, one floating mini controller, nothing else visible. Neutral, quiet, light and dark following Windows theme.

### 8.1 Main window
- **Top bar:** editable session name, source toggles (Me mic, Others system) with device picker on click, status pill ("Transcribing", "Paused", "Ready").
- **Controls:** Start · Pause/Resume · End. Context-aware (only valid actions enabled).
- **Live pane:** transcript lines styled per 5.1 (timestamp muted, speaker bold, Me and Others visually distinct).
- **Footer:** Library, Settings, AI handoff (after End or from Library).

### 8.2 Live pane scrolling (must feel calm)
- **Follow mode** is on when the user is within 48 px of the bottom. New lines keep the view pinned to the bottom smoothly.
- Scrolling up turns follow mode **off**. New lines must not move the user's view (use scroll anchoring; partial-line updates happen below the viewport).
- While off, show a floating **"Return to live"** pill with a count of new lines. Clicking it smooth-scrolls to the bottom and turns follow mode back on. Keyboard: `End`.
- Long sessions use list virtualization to stay smooth (target 5,000+ lines).

### 8.3 Floating overlay
- Appears when the main window is minimized or closed to the tray while a session is running (transcribing or paused), and can be pinned on via setting. Once shown it stays until the main window returns, so Pause, Stop and Start never close it.
- Small, always-on-top, draggable (grip handle); remembers position per monitor. Dark by default; light when the app's Appearance setting is Light.
- **Always visible, never moving:** grip, status light (no text), elapsed time, captions toggle. These keep their position in every state; everything else grows to the right and below them.
- **On hover (or keyboard focus):** icon-only buttons with plain tooltips: Start or Pause or Resume, Stop, Open SprkNotes.
- **Live captions:** the captions toggle opens a fixed-height caption bar under the overlay (about 3 lines, newest at the bottom, the line being spoken in full colour). Default shows Others only; Settings can add Me. Captions are partials (see 4.3 partials), never written to disk or logged.
- No text says "recording". Tooltip example: "SprkNotes is transcribing".
- Close hides the main window to the system tray and the session keeps running; if the desktop has no tray, Close minimizes instead. The tray icon mirrors state with the same actions. On Linux the app runs through XWayland by default so the overlay can be positioned and stay on top (set `SPRKNOTES_WAYLAND=1` to opt out).
- Optional global hotkeys: start/pause and open app (configurable, off by default).

### 8.4 Library
- List of transcripts (newest first): name, date, duration, folder. Search by name and full text.
- Folder tree for user subfolders: create, rename, move transcripts (drag or menu).
- Transcript view: rendered Markdown, **View in folder** (`explorer /select,`), **Export** (md, txt, docx, json), **Rename**, **Summarize with Claude**, **Summarize with Copilot**, **Delete** (to Recycle Bin, with confirm).

### 8.5 Settings (keep short)
- Audio: mic device, Others on/off, filler removal, commit delay (advanced).
- Outlook: provider, context prompt behavior (Ask / Always / Never).
- Context: detect audio sources (on/off, merge window), Teams call context (on/off, poll interval), name 1:1 speakers (on/off), excluded apps/title patterns.
- Files: transcripts root, default export format.
- AI handoff: show Claude button, show Copilot button, edit prompt template.
- Models (16.7): active model, installed models, Import, Switch, Roll back. Locked with a "Managed by your organization" note when policy pins a model.
- Appearance: System, Light or Dark (the overlay stays dark unless Light is chosen).
- Live captions: on or off, and Others only (default) or Others and Me.
- Startup: launch at sign-in, start minimized to tray.

---

## 9. AI handoff

SprkNotes itself makes no network call here: it only opens the Claude or Copilot app with a prompt and the file. Sending the transcript to the cloud service happens inside that app, when the user presses send. Admin policy can hide either button (10.2). Only enterprise-protected services are allowed (Claude under the company's agreement, Microsoft 365 Copilot with work sign-in; never consumer Copilot).

### 9.1 Claude
- Build a Claude Desktop link: `claude://cowork/new?q=<prompt>&file=<transcript path>&folder=<transcript folder>` (all values URL-encoded).
- The transcript is attached as a file, not pasted into `q` (prompt text is capped at about 14,000 characters).
- The user reviews and sends in Claude. Claude saves the summary into the same folder (Claude asks for folder confirmation).
- If `claude://` is not registered: copy the prompt to the clipboard, reveal the file in Explorer, show a toast with instructions.

### 9.2 Microsoft 365 Copilot
- No documented link that attaches a local file. Flow: copy the prompt to the clipboard, open the configured Copilot target (default: Microsoft 365 Copilot app or `https://m365.cloud.microsoft/chat`; verify in M8), reveal the transcript in Explorer, toast: "Prompt copied. Attach the highlighted file and paste."
- Copilot cannot save to the folder; the user saves the result. (Revisit if Microsoft ships a file-attach link.)

### 9.3 Prompt template (default, editable)

Stored at `prompts/summary.md` in the app; overridable at `%ProgramData%\SprkNotes\prompts\summary.md` (admin) and `%APPDATA%\SprkNotes\prompts\summary.md` (user). Placeholders: `{basename}`, `{folder}`, `{title}`, `{date}`.

```
The attached file is a meeting transcript from SprkNotes (format sprknotes-transcript/1).
Each statement is one line: - `HH:MM:SS` **Speaker:** text
"Me" is the person who ran SprkNotes. A named speaker (e.g. "Sam Sample") is the only other person on a 1:1 call.
"Others" means one or more remote people when the call had 3+ people or nobody could be identified; do not guess who said an "Others" line.
Lines starting with ">" are events: pauses, people joining or leaving, and audio sources (videos or apps playing sound). Speech during an audio source window may be from a video, not a person in the meeting; treat it that way.
The front matter and the Meeting context, Participants and Audio sources sections give extra context. Invitees are not proof someone attended. During a group-call period, other participants were not identified.
Speech recognition can mishear words. Fix obvious errors from context, but never invent content.

Create a meeting summary and save it in {folder} as "{basename} - Summary.md" (also save a .docx with the same name if you can).

Structure:
1. Title, date, time, who actually participated (Participants section; fall back to invitees and say so) and any media played (Audio sources)
2. TL;DR: 3 to 5 bullets
3. Discussion by topic: short bullets
4. Decisions, each with its transcript timestamp
5. Action items table: Owner | Task | Due (only if stated, else "Not stated") | Timestamp
6. Open questions and follow-ups
7. Up to 3 notable quotes with timestamps

Rules: use only the transcript and meeting context. Mark uncertain items "(unclear)". Include personal or health details only when an action item requires them.
```

---

## 10. Configuration

### 10.1 User settings
JSON at `%APPDATA%\SprkNotes\settings.json` (portable mode: next to the exe when `portable.flag` exists). Schema-versioned with migrations.

### 10.2 Admin policy (RMM-friendly)
Registry overrides win over user settings and lock the matching UI control:
- `HKLM\Software\Policies\SprkNotes` (machine) then `HKCU\Software\Policies\SprkNotes` (user).
- Keys: `TranscriptsRoot`, `CalendarProvider` (`com`/`off`), `ContextPrompt`, `ShowClaudeHandoff`, `ShowCopilotHandoff`, `CopilotTarget`, `PromptTemplatePath`, `FillerRemoval`, `CommitDelaySeconds`.
- Model keys (section 16): `PinnedModel` (`id@version`), `AllowUserModels` (0/1), `AllowedModelHashes` (multi-string, SHA-256 of `model.json`), `RequireSignedModels` (0/1; forced on from 1.0), `ModelPublicKeys` (multi-string, OpenSSH `allowed_signers` lines for Ed25519 keys), `KeepModelVersions` (default 2).
- Context keys (sections 18, 19): `AudioSourceDetection` (0/1), `AudioMergeWindowSeconds`, `CallMergeWindowSeconds`, `AudioSourceExcludePatterns` (multi-string: process names or title regexes), `CallContext` (0/1), `CallPollSeconds`, `NameOneToOneSpeakers` (0/1), `TeamsLocalApi` (0/1, default 1: probed silently at session start).

### 10.3 Models
See section 16.

---

## 11. Packaging and deployment

One release = one version = all artifacts, built by `scripts/release.ps1` from the root `Cargo.toml` version.

| Artifact | Use | Notes |
|---|---|---|
| `SprkNotes_<ver>.msixbundle` | Intune (LOB app), manual install | One file containing x64 + arm64. Per-user, no admin. Requires a trusted code-signing cert. MSIX version is `<ver>.0`. |
| `SprkNotes_<ver>_arm64.msi` + `SprkNotes_<ver>_x64.msi` | NinjaOne, GPO | Built with WiX v5+ (MSI is single-arch by design). Per-user default; per-machine via `ALLUSERS=1`. |
| `Install-SprkNotes.ps1` | NinjaOne | Detects OS architecture, installs the matching MSI silently, logs to `%ProgramData%\SprkNotes\install.log`. Includes `-Uninstall`. One script for the whole fleet. |
| `SprkNotes_<ver>.intunewin` | Intune (Win32 app) | Optional alternative: MSIs + script wrapped with IntuneWinAppUtil, with detection rule docs. |
| `SprkNotes_<ver>_portable.zip` | No-install use | Contains `arm64\` and `x64\` builds plus a tiny `SprkNotes.exe` launcher that starts the native build. Settings stored beside the exe. Delete the folder to remove. |
| `sprkctl.exe` (arm64 + x64) | All | Admin CLI shipped inside every artifact (section 16.6). |
| `<model-id>@<model-ver>.sprkmodel` | All | Model packs ship **separately from the app** with their own version. One default pack is also bundled read-only in the app so it works out of the box. |
| `Install-SprkModel.ps1` | NinjaOne / Intune | Wraps `sprkctl model install --scope machine`. Runs as SYSTEM. One script for the whole fleet. |
| `SHA256SUMS.txt`, `release-manifest.json` | All | Hashes and the version of every artifact. |

**Alignment checks in `release.ps1`:** every artifact reports the same version (`SprkNotes.exe --version` per arch, MSI ProductVersion, MSIX manifest); fail the release otherwise.

**Uninstall** never deletes transcripts. Cache and settings removal is opt-in.

**Signing:** all executables, MSIs and MSIX are signed (company cert or Azure Trusted Signing). Unsigned builds are dev-only.

**CI:** Forgejo Actions on a Linux runner for lint, portable tests and cross-compiles. Windows tests and release packaging run in the Windows 11 VM on the dev host (13.4), driven by `scripts/winvm.sh`; a Forgejo runner can be installed in that VM later. WiX and MSIX tooling need Windows.

---

## 12. Security and privacy checklist

- [ ] No audio written to disk anywhere (code search + runtime file-activity test).
- [ ] No outbound network connections from SprkNotes except the model index and pack downloads (firewall log shows only `localhost` Teams API traffic, plus the model host when downloads are on; nothing at all with `ModelDownload=0`). sprkctl's only network use is the admin-run `model sync` from an internal UNC share.
- [ ] Logs redacted: no transcript text, subjects, names, bodies.
- [ ] WER heap dumps disabled for the app.
- [ ] Tauri CSP strict, no remote content, capabilities minimal per window.
- [ ] File access scoped to transcripts root, settings and models.
- [ ] Third-party licenses collected into `THIRD_PARTY_NOTICES.md`.
- [ ] Repo scan for secrets, IDs and real names before each push.
- [ ] Model packs: hashes verified on every load; signature required from 1.0 (pilot: hash allow-list); packs never execute code; zip extraction blocks path traversal and size bombs.
- [ ] Model signing private key never in the repo or on endpoints; signing done only through the 1Password SSH agent (16.10).
- [ ] Audio source titles and participant names never appear in logs; excluded apps/patterns honored before anything is stored.
- [ ] UI Automation reads are read-only and limited to browser tab strips (18.3). No UI Automation of Teams.

---

## 13. Testing and acceptance

### 13.1 Automated
- Unit: format writer, front matter finalize, naming/sanitizing, dedupe, filler filter, statement splitting, meeting selection, policy overrides, recovery.
- Integration: feed fixture audio (both streams) through the pipeline, compare to expected transcript (WER threshold).
- Models: manifest parsing, hash allow-list and Ed25519 SSHSIG signature checks, resolver precedence (16.4), truncated/corrupt pack rejected, path-traversal zip rejected, rollback, pruning, no swap during an active session.
- `check-terms.sh` (runs on the Linux host and in CI): fails on "record", "recording", "recorded" (whole words) in UI strings, installer text, README and the admin guide. Developer docs (CLAUDE.md, SPEC.md, ADRs) are excluded so they can state the rule.

### 13.2 Manual / device
- **Bleed test:** laptop speakers on, play a fixture through system audio while silent user, then while user talks. Target: ≤ 2% duplicate lines.
- **Crash test:** kill the process mid-session. File holds all committed lines; recovery prompt works.
- **Scroll test:** scroll up during a busy session for 60 s; view must not move; "Return to live" works.
- **Device change test:** plug/unplug headset and Bluetooth mid-session; session continues.
- **Audio source test:** play a video in Firefox and Edge; pause 20 s and resume (one activity, gap counted); stop at 1:30 and wait 2:00 (duration 1:30); switch to a different video (new activity); excluded app never appears.
- **Call context test:** 1:1 Teams call (remote speaker named); add a third person (switch to Others within one poll + confirm cycle, event line written); drop back to 2 (name restored); unscheduled call works; Teams closed, Teams API disabled, or pairing declined: no prompt, no error, labels fall back to Others, Settings shows the provider as unavailable.
- **Offline test:** with networking disabled, every feature except the AI handoff works identically.
- **Model update test:** while transcribing, install a new pack as SYSTEM. Current session is untouched; next session uses the new model; `rollback` restores the old one; a deliberately broken pack is rejected and the app keeps working.

### 13.2a Per-milestone ARM64 check (manual, Snapdragon laptop)
For any milestone touching audio, ASR, UI or packaging: copy the ARM64 portable build to the Snapdragon laptop, run a 5-minute synthetic session (fixture played through speakers + talking), confirm transcript, labels, scrolling and mini controller, and record the result in the milestone report.

### 13.3 Performance gates
| Metric | Snapdragon X 16 GB | Intel 16 GB |
|---|---|---|
| Real-time factor (both streams) | ≤ 0.3 | ≤ 0.5 |
| Line visible after utterance ends | ≤ 2 s | ≤ 3 s |
| RAM while transcribing | ≤ 1.5 GB | ≤ 1.5 GB |
| CPU when idle (not transcribing) | ~0% | ~0% |

Intel devices that miss the gates are documented as unsupported. Gates are measured **only on the real laptops**; numbers from the dev host or VM are informational.

### 13.4 Test environments
| Environment | Hardware | Runs | Does not run |
|---|---|---|---|
| Dev host | CachyOS, x86_64 desktop | Portable crate tests with fakes, engine accuracy on fixtures, clippy/fmt, cross-compile x64 + ARM64 Windows | Anything needing Windows APIs; ARM64 execution; performance gates |
| Windows VM `sprk-winvm` | Windows 11 x64 guest on KVM, virtio drivers, PipeWire/SPICE audio, OpenSSH server | Full workspace tests, WASAPI capture/loopback and AEC behavior, Tauri UI, classic Outlook COM (test mailbox, no PHI), Teams local API, packaging and installer tests | ARM64 execution; performance gates |
| Snapdragon laptop | Dell Snapdragon X, 16 GB | ARM64 portable build, 13.2a check, ARM performance gates | Development tools |
| Intel laptop | Intel x64, 16 GB | x64 performance gates | Development tools |

- **ARM64 emulation is not used**: a Windows ARM64 guest on an x64 host would run under full CPU emulation, too slow for meaningful audio or latency testing.
- **Wine is not used** for app testing (WebView2, WASAPI and COM behavior differ from real Windows).
- The VM gets a dedicated test identity with a synthetic mailbox/calendar if Outlook and Teams integration tests are needed.

---

## 14. Milestones

| # | Milestone | Acceptance |
|---|---|---|
| M0 | Scaffold + dev environment | Workspace with platform seam (3.4); portable crates test on the Linux host; both Windows targets cross-compile with cargo-xwin; `scripts/winvm.sh` syncs and runs tests in the Windows VM; Tauri app opens in the VM; version script; ADR-001 and ADR-002-dev-environment |
| M1 | Engine spike + model format | Engine trait with one sherpa-onnx adapter; `model.json` v1 schema; CLI transcribes a WAV and live mic from a model folder (no hard-coded paths); benchmark candidates on ARM and Intel; ADR-003 picks the default model |
| M2 | Capture | Mic + loopback captured together, device changes handled, Me/Others labels in CLI output |
| M3 | Writer | Format 5.1 exactly, append + flush, commit buffer, finalize, recovery; crash test passes |
| M4 | Bleed | Windows AEC control, APM fallback, transcript dedupe; bleed test passes |
| M5 | Main UI | Start/Pause/End, rename, live pane scroll behavior (8.2), status pill |
| M6 | Mini controller + tray | Floating pill with waveform and controls, tray, optional hotkeys |
| M7 | Library + exports | List, search, folders, View in folder, md/txt/docx/json export |
| M8 | Outlook | Classic Outlook COM provider, platform detection, naming, context prompt; silent 'unavailable' on new Outlook |
| M9 | AI handoff | Claude link with file + folder, Copilot clipboard flow, editable template, policy toggles |
| M10 | Model management | `.sprkmodel` packs, `sprkctl`, machine/user/portable stores, incoming import, `active.json`, smoke test, rollback, pruning, **hash allow-list** (16.10 pilot tier), Settings > Models, `Install-SprkModel.ps1`; model update test passes |
| M11 | Audio sources | Session enumeration, metering, title resolution, merge window, exclusions, events + finalize section, naming fallback; audio source test passes |
| M12 | Call context | Provider chain (19.2), participant tracking, labeling modes with hysteresis, events, naming; call context test passes |
| M13 | Packaging | MSIX bundle, MSIs + install script, .intunewin, portable zip, code signing, aligned release (sprkctl included) |
| M14 | Hardening | Checklist 12 complete, accessibility pass (keyboard, screen reader labels), README + admin guide (incl. model and context runbooks) |
| M15 | Model signing (1.0 gate) | Ed25519 SSHSIG signing via the 1Password SSH agent, `sprkctl model sign/verify`, key rotation with two keys, `RequireSignedModels` enforced; 1.0 cannot ship without it |

---

## 15. Open items

- Code-signing source (internal certificate deployed by NinjaOne for the pilot).
- Windows 11 VM licensing (evaluation edition vs. licensed copy) and whether a test Outlook/Teams identity with a synthetic calendar is available for the VM.
- Policy approval for building with a personal Claude account on personal hardware.
- Confirm Claude and Microsoft 365 Copilot are approved for PHI under company agreements before enabling handoff buttons broadly.
- Confirm Documents is OneDrive-redirected (Known Folder Move) on target devices.
- NPU (QNN) acceleration for v1.1.
- Bundled default model: which pack ships inside the app (decide after M1 benchmarks, weigh installer size).
- Model signing (required for 1.0, not for pilot): named key owner and backup holder, dedicated 1Password vault, one-page key SOP (16.10).
- Teams local API: availability unknown; the app probes it silently at each session start (19.2). Confirm in the M12 spike.
- Optional internal model share for `sprkctl model sync` (UNC path and who maintains it).

---

## 16. Transcription models and updates

### 16.1 Goals
- Swap, update or roll back the transcription model **without reinstalling the app**.
- Same mechanism for managed fleets (RMM/Intune), single users, and the portable zip.
- A broken, partial or tampered model must never break transcription; the app falls back to the last good model.
- Never change models in the middle of a session.

### 16.2 Why not prefix renaming (`new.` / `update.` -> `current.`)
The drop-in idea is kept; the rename mechanics are replaced because:

| Problem with prefix renaming | Fix in this design |
|---|---|
| A model is a folder of several files (encoder, decoder, joiner, tokens), not one file | Ship one `.sprkmodel` pack file |
| App may start while RMM is still copying, and load a half-written model | Packs are zips with SHA-256 hashes in a manifest; a partial file fails verification and is skipped until complete |
| Windows locks files in use, so renaming the current model can fail | Never rename in place. Each version gets its own folder; a small `active.json` pointer is switched atomically |
| No rollback once `current.` is overwritten | Previous version kept; `active.json` records it; one-step rollback |
| Users cannot write to `%ProgramData%`, so a user-level app cannot promote machine models | Machine models are imported by `sprkctl` running as SYSTEM (RMM). The app only reads machine scope |
| Nothing says which engine, language or app version a model needs | `model.json` manifest declares engine, features and `min_app_version` |
| No protection against a swapped or tampered model | Pilot: admin-set hash allow-list. 1.0: mandatory Ed25519 signature (16.10) |
| Per-user AppData copies (hundreds of MB per user) | Machine store in `%ProgramData%` shared by all users |

### 16.3 Stores and layout
Three stores share one layout:

| Store | Path | Written by |
|---|---|---|
| Bundled (read-only) | `<app>\models\` | Installer. Holds the default pack so the app works out of the box |
| Machine (managed) | `%ProgramData%\SprkNotes\models\` | `sprkctl` as SYSTEM / admin (RMM, Intune) |
| User | `%LOCALAPPDATA%\SprkNotes\models\` (MSIX: package-virtualized, removed on uninstall) | The app itself (Import, incoming folder), only if `AllowUserModels=1` |
| Portable | `<zip root>\models\` | The app itself |

```
models\
  incoming\                                   # drop .sprkmodel files here
  parakeet-tdt-0.6b-int8@2026.11.0\
    model.json
    model.sig                                 # Ed25519 OpenSSH signature (SSHSIG) of model.json (required from 1.0)
    encoder.int8.onnx  decoder.int8.onnx  joiner.int8.onnx  tokens.txt
  parakeet-tdt-0.6b-int8@2027.01.0\
  active.json                                 # {"active":"parakeet-tdt-0.6b-int8@2027.01.0","previous":"...@2026.11.0","updated":"<iso8601>"}
  import-log.jsonl                            # one line per import attempt: pack, result, reason
```

### 16.4 Which model loads (resolver)
Evaluated at app launch and again at each session start:
1. Policy `PinnedModel` if set and valid (in any store).
2. Machine store `active.json`.
3. User/portable store `active.json` (only if `AllowUserModels=1` or no policy exists).
4. Bundled default.

If the chosen model fails verification or loading, fall back to the next valid candidate (previous version first), log it, and show a quiet notice in Settings. Transcription keeps working.

### 16.5 Manifest (`model.json`, schema 1)
```json
{
  "schema": 1,
  "id": "parakeet-tdt-0.6b-int8",
  "version": "2027.01.0",
  "display_name": "Parakeet TDT 0.6B (int8)",
  "engine": "sherpa-onnx/offline-transducer",
  "languages": ["en"],
  "sample_rate": 16000,
  "features": { "punctuation": true, "casing": true, "streaming": false },
  "accelerators": ["cpu"],
  "min_app_version": "0.4.0",
  "files": {
    "encoder": "encoder.int8.onnx",
    "decoder": "decoder.int8.onnx",
    "joiner": "joiner.int8.onnx",
    "tokens": "tokens.txt"
  },
  "sha256": { "encoder.int8.onnx": "<hex>", "decoder.int8.onnx": "<hex>", "joiner.int8.onnx": "<hex>", "tokens.txt": "<hex>" },
  "smoke_test": { "fixture": "builtin:pangram-en", "expect_contains": ["quick", "fox"] },
  "license": "<SPDX id>",
  "source": "<upstream URL>",
  "notes": "Changelog for this model version"
}
```
- `files` keys are roles the adapter looks up (`sherpa-onnx/offline-transducer`: encoder, decoder, joiner, tokens; `sherpa-onnx/whisper`: encoder, decoder, tokens; `sherpa-onnx/vad-silero`: vad). A voice activity model is a pack of its own with `features` all false.
- `engine` must match an adapter compiled into the app (17.2). New weights for a supported engine family need **no app update**. A new engine family needs an app release with a new adapter.
- Versions use `YYYY.MM.patch` so they sort correctly.

### 16.6 Import and promotion flow
Same steps for every store; only the actor differs.

1. **Detect:** at launch, at session start, and on a folder watch of `incoming\`, look for `*.sprkmodel`. Ignore `*.partial` and files modified in the last 10 s (copy still in progress).
2. **Verify:** open zip (a partial file fails here), block path traversal and oversized entries, parse `model.json`, check schema, engine adapter exists, `min_app_version`, every SHA-256, and signature if required.
3. **Extract** to `models\.staging\<id>@<version>\`, then rename the folder to `models\<id>@<version>\` (atomic, same volume).
4. **Smoke test:** load the model in a separate worker and decode the built-in fixture; check `expect_contains`. Optional benchmark records real-time factor in the import log.
5. **Activate:** write `active.json` via temp file + atomic replace, recording `previous`. Skipped if the pack was imported with `--no-activate` or policy pins another model.
6. **Apply:** the app loads the new model at the **next session start**. A running session keeps its model until it ends.
7. **Clean up:** move the pack to `incoming\processed\` (or delete), prune old versions beyond `KeepModelVersions` (never the active or previous).
8. **Reject:** on any failure, move the pack to `incoming\rejected\` with `<pack>.reason.txt`, log to `import-log.jsonl`, keep the current model.

**Who runs it:**
- **Machine store:** `sprkctl model install --scope machine <pack>` as SYSTEM (wrapped by `Install-SprkModel.ps1` for NinjaOne/Intune). RMM can also just copy into `%ProgramData%\SprkNotes\models\incoming\` and run `sprkctl model import --scope machine`.
- **User and portable stores:** the app does steps 1 to 8 itself, or the user uses Settings > Models > Import.

### 16.7 Admin CLI: `sprkctl`
Shipped with every artifact (arm64 + x64). Exit codes are RMM-friendly (0 ok, 10 verify failed, 11 smoke test failed, 12 incompatible, 20 not elevated for machine scope).

```
sprkctl model pack <folder> -o <file.sprkmodel>
sprkctl model sign <file.sprkmodel> --pubkey <key.pub>   # 1.0: signs model.json via ssh-keygen -Y sign + the 1Password SSH agent
sprkctl model verify <file.sprkmodel> [--require-signature]
sprkctl model hash <file.sprkmodel>               # prints the SHA-256 of model.json for AllowedModelHashes
sprkctl model allow <file.sprkmodel> --scope machine   # pilot tier: adds that hash to HKLM AllowedModelHashes (elevated)
sprkctl model install <file.sprkmodel> --scope machine|user [--no-activate] [--pin]
sprkctl model import --scope machine|user        # process incoming\
sprkctl model list [--scope ...] [--json]
sprkctl model activate <id@version> --scope ...
sprkctl model rollback --scope ...
sprkctl model remove <id@version> --scope ...
sprkctl model bench [<id@version>]               # real-time factor on this device
sprkctl model sync --from <\\server\share\models> --scope machine   # optional: pull newest packs from an internal share
```
`sync` reads only from an internal file share configured by the admin; the app itself never downloads models.

### 16.8 Settings > Models (UI)
- Shows active model, version, engine, source store, last benchmark.
- Installed list with **Switch** and **Roll back**; **Import model pack** button (user/portable scope only).
- When policy pins or manages the model, controls are read-only with "Managed by your organization".

### 16.9 Release process for a new model (runbook outline)
1. Convert/obtain the model, place files in a folder with `model.json`.
2. `sprkctl model pack`, then `sprkctl model sign` (1.0) or record the hash with `sprkctl model hash` (pilot).
3. Test on one Snapdragon and one Intel device: `install --scope machine --no-activate`, `bench`, `activate`, run a test session, `rollback` check.
4. Publish the pack to the internal share / RMM library; deploy `Install-SprkModel.ps1` to a pilot group, then everyone.

### 16.10 Model trust tiers (pilot vs. 1.0)
Signing is **mandatory for the 1.0 release**. Pilot builds (0.x) may run without keys using the hash allow-list.

| Tier | Applies to | Control | Behavior |
|---|---|---|---|
| **Pilot (0.x)** | Managed pilot devices | `AllowUserModels=0` + `AllowedModelHashes` in HKLM, written by `Install-SprkModel.ps1` | Only admin-approved packs load. Bundled default is always trusted. Unknown models show "Unverified" and are refused when any allow-list exists |
| **1.0 and later** | All installs | `RequireSignedModels=1` (default and enforced; policy cannot turn it off) + `ModelPublicKeys` | Every pack must carry a valid Ed25519 signature from a trusted key. Hash allow-list still works as an extra restriction |

**Signing practice (1.0), using 1Password:**
- **Format:** Ed25519 key, OpenSSH signature format (SSHSIG) with namespace `sprknotes-model`, over canonical `model.json` (which already holds every file hash). Admins can also check a pack by hand with `ssh-keygen -Y verify`.
- **Where the key lives:** generated inside 1Password as an SSH key item in a dedicated vault (`SprkNotes Signing`). The 1Password SSH agent signs on request after a Windows Hello / 1Password approval; **the private key never leaves 1Password**. `sprkctl model sign` calls `ssh-keygen -Y sign -n sprknotes-model -f <public key>` against the agent.
- **People:** a named **key owner** (signs releases) and a **backup holder** with their own separate key in a separate vault. Vault access limited to those two plus 1Password account recovery admins. Ownership is assigned to roles (for example "SprkNotes release owner"), recorded in the SOP and the risk register.
- **Two-person check:** the person who builds and tests a pack records its `model.json` hash; the signer confirms the hash matches before approving the signature.
- **Keys in the app:** both public keys are listed in `ModelPublicKeys` (HKLM policy, `allowed_signers` format) and compiled into the app as a fallback trust root.
- **Rotation / revocation:** rotate yearly or when a holder leaves: add the new key by policy, re-sign current packs, remove the old key by policy. No app update needed. If both keys are lost or compromised, ship an app release with new embedded keys.
- **Records:** a signing log (date, pack, hash, signer, approver) kept with the SOP; `import-log.jsonl` on devices records which key verified each pack.
- **Never:** private keys exported, in the repo, in CI variables, or in plain files.
- **Upgrade path:** if the org later adopts an HSM or cloud key service, the signature verifier is behind a trait; add a verifier, re-sign, rotate keys by policy.

---

## 17. Extensibility

### 17.1 Principles
- Every replaceable part sits behind a Rust trait with a small registry. Adding a part means adding an adapter and an ADR, not editing the pipeline.
- Data formats are versioned (`sprknotes-transcript/N`, `model.json` schema N, settings schema N) with forward migrations and tests.
- Config-driven where possible so admins can extend without code.

### 17.2 Extension points
| Extension point | Trait / mechanism | Examples |
|---|---|---|
| Speech engine adapter | `AsrEngine` keyed by `model.json` `engine` | `sherpa-onnx/offline-transducer`, `sherpa-onnx/whisper`, `sherpa-onnx/offline-ctc`, future `qnn` variants, `whisper.cpp` |
| Voice activity detection | `Vad` | Silero, TEN-VAD |
| Echo cancellation | `EchoCanceller` | Windows AEC, WebRTC APM |
| Exporters | `Exporter` (input: parsed transcript) | md, txt, json, docx; later srt/vtt, pdf |
| Calendar providers | `CalendarProvider` | Outlook COM (classic); later other local sources |
| AI handoff targets | Config entries: name, launch template, prompt template, policy key | Claude, M365 Copilot; new targets added by config |
| Post-processing | `SegmentFilter` chain in the commit buffer | dedupe, filler removal, custom vocabulary fixes (e.g. company terms) |
| Audio source resolvers | `AudioSourceResolver` chain (18.3) | media session, browser tab UIA, window title, process name |
| Call context providers | `CallContextProvider` chain (19.2) | calendar (COM), Teams local API, Teams call window title; later Zoom |

### 17.3 Compatibility rules
- Readers accept the current and previous transcript format versions.
- A model pack declaring an unknown `engine` or a higher `min_app_version` is rejected with a clear reason, never half-loaded.
- Removing an extension point or changing a trait needs an ADR and a migration note in CHANGELOG.md.

---

## 18. Audio source detection (context)

### 18.1 Purpose
Record which apps were heard through system audio (a Teams call, a YouTube video in Firefox, a training module), with titles and accurate durations. Used for transcript context, naming fallback (6.2) and the AI summary. On by default; per-user toggle; admin policy can force off.

### 18.2 Detecting sound per app
- Enumerate audio sessions on every active render endpoint (`IAudioSessionManager2`), with session-created notifications so new apps are picked up immediately.
- Poll each session's peak meter (`IAudioMeterInformation`) every 250 ms. **Audible** = peak above about -50 dBFS for at least 300 ms. Muted sessions count as silent.
- Ignore SprkNotes itself and the Windows system-sounds session.
- Map the session's process to the owning app: browsers play audio from helper/content processes, so walk the parent-process chain to the main browser process and its windows.
- Only runs while transcribing. Pause closes open activities at the pause moment; resume re-detects.

### 18.3 Finding the title (resolver chain, first hit wins)
1. **Media session** (Windows Global System Media Transport Controls): title and artist the app publishes (browsers publish the video/page media title; Spotify and others publish track info).
2. **Browser tab with the audio indicator** via UI Automation on the tab strip (Chrome, Edge, Firefox). Confirm each browser's accessible tab names in the M11 spike.
3. **Top-level window title** of the owning app (for browsers this is the active tab, so it ranks below 1 and 2).
4. **App name** from the executable's file description.
- Strip browser suffixes (" - Mozilla Firefox", " - Google Chrome", " - Microsoft Edge"). Matching rules live in a config file so they can be tuned without a release.

### 18.4 Timing rules
An **activity** = one app + one title, from first audible moment to last audible moment.

| Setting | Default | Range | Why |
|---|---|---|---|
| Merge window (media and general apps) | **30 s** | 10 to 300 s | Covers buffering, short pauses, ad breaks |
| Merge window (call apps: Teams, Zoom, Webex, Phone Link) | **120 s** | 30 to 600 s | Calls have natural silences; avoids splitting one call into many |
| Minimum activity length | 3 s | 1 to 30 s | Ignores notification chimes |

Rules:
- Silence shorter than the merge window followed by sound again = **same activity; the silent gap counts toward duration** (plays to 1:30, silent 20 s, resumes -> continuous, gap included).
- Silence that reaches the merge window = **activity ends at the last audible moment**, not when the window expired (stops at 1:30, nothing by 2:00 -> duration 1:30).
- A **title change** (new video, new track) ends the current activity at the last audible moment and starts a new one.
- Several apps can be active at once; each is tracked separately.
- Durations are measured on the session clock, so they line up with transcript timestamps.

### 18.5 Output
- **Live:** an event line once an activity passes the minimum length (timestamped at its real start), and another when it ends (timestamped at the last audible moment, with duration). End events can be written after later-timestamped statement lines; the timestamp is authoritative and parsers sort by it.
- **Finalize:** `audio_sources` in front matter and the **Audio sources** table (5.1). JSON export includes `audio_sources[]`.
- **Crash:** everything needed is already in the event lines; recovery rebuilds the table from them.

### 18.6 Privacy
- Window and tab titles can contain PHI (for example a patient record tab). They are written only into the transcript, never to logs.
- `AudioSourceExcludePatterns` (policy + user setting): process names or title patterns that are never captured. Excluded activities are omitted entirely, not masked.
- Detection reads metadata only; it never captures or stores another app's audio separately.

---

## 19. Call context and speaker naming

### 19.1 Purpose
Know who the user is actually in a call with (scheduled or not), keep that list current during the call, and label remote speech with the other person's name on 1:1 calls.

### 19.2 What is possible on-device, and the provider chain
Everything here is local: no Graph, no Entra, no cloud. No supported local interface exposes a Teams call's full participant list, and the participants-pane reader is out of scope, so SprkNotes reliably knows **whether** the user is in a call, **whether it is 1:1 or a group**, and **the other person's name on 1:1 calls**. Group-call participants are not individually identified.

| # | Provider | Gives | Confidence | Notes |
|---|---|---|---|---|
| 1 | Calendar (section 7, classic Outlook only) | Invitees, platform, subject | Low for attendance | Invitees are not proof of who joined |
| 2 | Teams local API (optional, on by default) | In a call/meeting: yes/no, start/end | High for state, no names | Local WebSocket on `localhost`. **Probed silently at each session start**: if Teams is not running, the API is disabled in Teams privacy settings, or pairing is not approved, the provider is marked unavailable for that session with no prompt or error. Status shown only in Settings. |
| 3 | Teams call window title | 1:1: the other person's display name. Group: meeting/chat title | Medium | Read with standard window enumeration (no UI Automation). Parsing rules in a config file so Teams UI changes can be patched without a release |

**Combining signals:**
- **In a call:** provider 2, or a Teams call window exists (provider 3).
- **1:1 vs group:** a call window title that parses as a single person's name -> 1:1. Teams turns a 1:1 call into a new group call when a third person is added, which replaces the call window/title; that change is the group signal. Anything that does not parse cleanly -> `unknown`.
- **Name on 1:1:** the parsed window title. If unavailable, the calendar invitee only when the matched meeting has exactly one other invitee **and** the call parses as 1:1.
- If every provider is unavailable, labeling stays `unknown` (Others) and nothing is shown to the user during the session.

### 19.3 Polling
- While transcribing: window title check every **15 s** (5 to 60 s, `CallPollSeconds`); cheap and local. Teams local API events are used when available.
- Nothing runs when not transcribing.
- The 1:1 participant and group periods are tracked with session-clock times.

### 19.4 Labeling modes (speaker attribution)
| Mode | When | Remote (system audio) speech labeled as |
|---|---|---|
| `named` | Confirmed 1:1 call with a known name | That person's display name |
| `group` | Call became a group call | `Others` |
| `unknown` | No reliable signal, Teams closed, or providers unavailable | `Others` |

- Mic speech is always **Me**.
- **Switching needs 2 consecutive polls that agree** (about 30 s at default) to avoid flapping. Between first sign of change and confirmation, remote lines are labeled `Others` (when unsure, never guess a name). The commit delay (4.4) lets most lines get the right label before they are written.
- Every mode change writes an event line, e.g. `> \`00:20:05\` Call became a group call: remote speakers labeled Others` and `> \`00:31:40\` Back to 1:1 with Sam Sample: remote speaker labeled Sam Sample`.
- Labels only apply to lines committed after the change; existing lines are never rewritten.
- **Limitations:** group-call participants are not named; someone in the same room as the user is labeled Me; dial-in and room-device participants show the name Teams displays; non-Teams calls (Zoom, phone) stay `unknown`.

### 19.5 Naming
- 1:1 call: `Call with <Name>`. Unscheduled group call: the Teams call window title. (Position 2 in the 6.2 chain.)

### 19.6 Zoom and others
- v1: Zoom meetings come from the calendar (classic Outlook), and Zoom audio is detected as an audio source (18). No Zoom call context.
- Later: a `CallContextProvider` for Zoom window titles if needed.

### 19.7 Privacy and security
- Participant names go only into the transcript; never into logs.
- The Teams local API pairing token is stored in Windows Credential Manager (DPAPI), never in settings files.
- No UI Automation of Teams. Window titles are read with standard Windows calls only.

### 19.8 Out of scope
- Reading the Teams participants pane (UI Automation): removed; unsupported and brittle.
- Cloud roster data (Graph meeting events, webhooks): conflicts with the on-device rule.

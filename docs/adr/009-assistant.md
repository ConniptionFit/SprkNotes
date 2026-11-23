# ADR-009: On-device assistant (runtime, isolation, grounding)

Status: **accepted** (owner request and decisions, 2026-11-23). Amends SPEC 1 (non-goal "Built-in AI summarization"). Plan and research: the assistant plan document in the project files.

## Context
Users want to ask about the meeting in progress ("what did I miss?", "what are the action items?", "when is X due?") and get dynamic note titles. The cloud handoff (SPEC 9) cannot do this live and sends data off the device. The feature must run on a PC that is in use, beside transcription (about 1.8 GB resident), without slowing it.

## Decision
1. **Name in the code and docs: "assistant" / "assistant model".** "Transcription model" keeps meaning speech recognition. User-facing words say "transcript", never "recording".
2. **Runtime: llama.cpp** through the `llama-cpp-2` crate, **CPU first**, behind the `AssistantEngine` trait (`sprk-assistant`). No NPU or GPU path ships in v1 (the Hexagon backend needs Windows test signing; Adreno OpenCL is a later opt-in). Choice of the first real models waits for the benchmark spike on the laptops.
3. **Separate helper process** (`sprk-assistant`), spoken to over a local pipe with JSON lines (`protocol`). A crash or out-of-memory in the helper cannot touch transcription. The helper has no network code: CI fails if `sprk-assistant` depends on a network crate (`scripts/check-assistant-deps.sh`).
4. **Polite guest rules** (`governor`): below-normal priority, a third of the cores (at most 4, admin cap `AssistantMaxThreads`), one request at a time, short outputs, load on demand, unload after 5 idle minutes (`AssistantIdleUnloadMinutes`), pick the biggest installed tier that leaves headroom free (1, 2 and 3 GB for titles, light and standard) and otherwise drop a tier or refuse, and **pause between tokens when transcription lag stays above 2.5 s for 3 s** (resume after 5 s healthy, cancel after 20 s held). Transcription always wins.
5. **Tiers:** `titles`, `light`, `standard`. **Light is the default size** (owner decision). **The assistant is off by default**; the user or `AssistantEnabled` turns it on. `AssistantTier` caps the size.
6. **Grounding** (applies to every feature, as it lands): the model sees a line-numbered view (`[L12 00:03:41] Others: ...`) wrapped as data; answers cite line ids; plain code verifies citations, and numbers, dates and names in an answer must appear in the cited lines; "not stated" is a valid answer; the assistant has no tools and produces text only; transcript text cannot close its own data block. Titles are cleaned and must share a significant word with the transcript.
7. **Never edits the transcript.** Output is shown to the user or, only with `AssistantSaveOutputs` allowed and the user asking, written to a separate notes file beside the transcript. A title is a suggestion; it never renames a file by itself.
8. **Privacy** (hard rules 4 and 11): prompts, questions, answers and titles are PHI-grade. They are never logged and never put in errors (errors carry a kind only). **Question and answer history lives in memory only** (owner decision) and ends with the app or session.

## Consequences
- New crate `sprk-assistant` (portable, testable on Linux with `FakeEngine`), new policy keys (`AssistantEnabled`, `AssistantTier`, `AssistantPinnedModel`, `AssistantIdleUnloadMinutes`, `AssistantMaxThreads`, `AssistantSaveOutputs`, `AssistantBackground`).
- SPEC 1 non-goal changes: built-in summarization is now a goal, on-device only; the Claude/Copilot handoff stays for higher-quality cloud summaries.
- Model packs for the assistant are a new pack kind (ADR-010) and use the existing download, trust and signing rules.
- Needs real-laptop numbers (SPEC 13.3) before any size is declared supported.

## Addendum (2026-11-23): features, runtime and first model picks
- **Features and policy.** Four core features (title suggestion, "What did I miss?", action items, Ask a question) are on once the assistant is on. Seven extras are each their own setting and **off until the user turns them on**: agenda check, my commitments, pin a moment, parking lot, explain this, say it again short, next-meeting prep. `AssistantFeatures` (comma list) is an allow-list: policy can only narrow what a user may enable. Pins and the parking lot are plain code and never start a model.
- **Same grounding for every extra.** Explain and say-it-short cite lines and pass the same number, date and name checks; agenda coverage needs a real cited line or it reads "Not yet"; "my commitments" is the action list filtered to items owned by Me (no guessing of anyone's name); next-meeting prep only copies already checked parts (action items, set-aside topics, pins).
- **Runtime.** `sprk_assistant::runtime` owns the session workspace, starts the helper only when a request needs it, applies the memory gate at load, allows one request at a time, and unloads after idle. The app glue (`app/src-tauri/src/assistant.rs`) only feeds lines and maps results. A failed helper is dropped and restarted on the next request. Errors sent to the UI are short codes, never text from the meeting.
- **Prompt formats.** `chatml`, `chatml-nothink` and `gemma4` (Gemma 4 turn tokens, stop at `<turn|>`). A pack still only names a format; it never carries a template.
- **First models (dev-host check, 2026-11-23; laptop numbers still pending).** The bundled llama.cpp loads Qwen3.5 0.8B, 2B, 4B and Gemma 4 E2B. Titles: 0.8B. Light (default): 2B, which adds small inventions to recaps, so the code checks are required. Standard: 4B (about 4.2 GB resident, so its headroom needs review on the laptops). Gemma 4 E2B is as accurate as 4B and faster, at about 4.3 GB.

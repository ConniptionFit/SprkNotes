# ADR-006: Live captions

Status: accepted (2026-11-19, John's request)

## Context
Committed transcript lines appear after the commit delay and after a finished utterance, so a speaker is heard well before they are read. John wants real-time captions in the overlay: Others by default, Me optional.

## Decision
- A caption is a transient view of speech in progress. It is never committed, written to the transcript, logged or kept after it is replaced (hard rules 1, 4, 6, 13 unchanged). The committed transcript still comes only from finished VAD utterances.
- `sprk_pipeline::caption::Captioner` keeps the last 10 s of audio per wanted source in memory and re-decodes it about every 0.7 s with the session's engine. A finished utterance starts the caption over, and the committed line is the final for that speaker.
- Only wanted sources are buffered or decoded: Others (system audio) when captions are on, and the microphone only when "Me" is also on. Off means no buffering at all.
- Cost control: silence and under 0.6 s of audio are not decoded. The wait before the next decode is twice the last decode time (between 0.7 s and 5 s), so captions use at most about a third of the session thread. Three decodes in a row slower than 3 s switch captions off for the rest of the session; transcription is unaffected.
- The host polls `Events::captions_wanted()` and emits `Events::caption(speaker, text)`; the Tauri layer emits the `caption` event `{ speaker: "Others" | "Me", text, final: false }`. Named 1:1 labels are not used in captions.
- Default off in the engine; the overlay's captions toggle turns it on, and "Me" is a Settings option.

## Consequences
Partial decodes share the single engine with committed decoding, so captions can delay committed lines slightly on slow machines; the backoff and auto-off bound that. The Snapdragon laptop must confirm the cost (SPEC 13.3).

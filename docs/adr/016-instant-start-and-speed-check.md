# ADR-016: Instant Start and the desktop speed check

Status: **accepted** (2026-11-26). John asked for both ("Add all 5 proposals").
Related: SPEC 4.6, 16.4, 16.8a; ADR-003 (default model), ADR-015 Decision 2 (phone tuning).

## Context
Start used to load the speech model (hash the files, build the recognizer) and free it at End. That took seconds on
every Start, and anything said while it loaded was lost because capture only began afterwards. Decode threads were
fixed at `min(4, logical cores)`, which is a guess: on a 12-core Snapdragon X more may help, on an Intel hybrid chip
more can be slower once efficiency cores join in.

## Decision 1: keep one warm engine and share it
The app loads the active model in the background (launch, main window shown, "Start a transcript?" shown, after a
download) and keeps it. `sprk_asr::SharedEngine` wraps it in `Arc<Mutex<Box<dyn Engine>>>` and each session gets a clone,
so no ownership has to be handed back at End and the `Engine` trait is unchanged. Only one session runs at a time, so the
lock is never contended. A panic in a decode poisons the lock: that engine refuses further decodes and is reloaded.
It is reloaded only when its key changes: the resolver's choice (id@version and folder, read without hashing) plus the
speed check's thread count.

## Decision 2: capture first, engine later
`Host::start` takes `EngineSource::Ready(engine)` or `EngineSource::Loading(receiver)`. With `Loading`, the session thread
starts capture at once and keeps everything (frames, device notices, Pause, Resume, source toggles) in an in-memory list
until the engine arrives, then replays it into a new `Session` in order. Because `Session` is clock-free and times come
from sample counts, replayed lines carry their real times. The file is created at Start as before; if the load falls back
to another model its label is written at finalize. A load that fails or takes over 5 minutes closes the file with an
event line and a `model_failed` notice. Audio never touches disk (hard rule 1).

Alternatives: a lazy engine that blocks the first decode (stalls Pause and End behind the load); queueing inside
`Session` (spreads the special case through the dedupe and commit timing). The host-level replay keeps `Session` as is.

## Decision 3: time the model once per machine
`sprk_asr::tune` gets the desktop pieces: candidates (half, equal and double `min(4, cores)`, at most 8), a built-in
synthetic clip, a timed decode after a warm-up, and the pick (fewest threads within 10% of the fastest). The app runs it
once per model and machine, stores `{threads, rtf}` in `%APPDATA%\SprkNotes\speed.json`, and passes the threads to the
engine through `EngineOptions` (a new argument to `EngineFactory`). `SPRK_ASR_THREADS` still wins. Any failure keeps the
old default.

## Consequences
- RAM: the model stays resident while SprkNotes runs (about 1 GB for Parakeet), also when idle. Accepted for an instant
  Start; documented in SPEC 4.6. The speed check loads a second copy for a few seconds.
- First launch after install runs the check (a few seconds in the background) before warming; a Start during it calls
  the check off and loads at once.
- The synthetic clip measures mostly the encoder; a Whisper decoder may emit fewer tokens than for real speech. Good
  enough to rank thread counts on one machine, not a benchmark to compare models.

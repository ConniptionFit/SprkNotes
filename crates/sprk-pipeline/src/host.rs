//! Session host: owns one live session on a dedicated thread (SPEC 3.2, 4.5, 6.3).
//! Echo removal, VAD, ASR, commit buffer and the crash-safe writer run here; the devices are
//! read on a capture thread of their own (`capture.rs`) so a decode never stalls them. The UI side only
//! sends commands and receives events. Everything platform-specific is injected, so the whole
//! flow runs on the Linux dev host with fakes.
//!
//! Hard rules: audio never leaves memory; committed lines are appended and flushed at once
//! (the writer does that), so a crash or a dropped `Host` loses nothing already committed.
use crate::capture::{CaptureThread, Captured};
use crate::commit::Config;
use crate::session::{Session, SessionError, Sink, VadFactory};
use crate::{Segment, Speaker};
use sprk_asr::Engine;
use sprk_audio::live::{Backend, CaptureEvent};
use sprk_audio::Source;
use sprk_context::activity::{ActivityTracker, Exclusions, Timing};
use sprk_context::labeling::{parse_call_title, LabelTracker, Observation, TitleRules};
use sprk_context::sources::{AudioSession, NoTabs, SourcePoller};
use sprk_context::Window;
use sprk_transcript::format::{timestamp, Meta, NameSource, Status};
use sprk_transcript::naming::{file_name, unique_path};
use sprk_transcript::writer::TranscriptWriter;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Local time pieces the host needs. The app supplies a real clock; tests supply a fixed one.
#[derive(Debug, Clone)]
pub struct Stamp {
    /// `YYYY-MM-DD`
    pub date: String,
    /// `HHmm`
    pub hhmm: String,
    /// `2026-11-18T13:00:04-06:00`
    pub iso: String,
}

pub trait Clock: Send {
    fn now(&self) -> Stamp;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiLine {
    Statement {
        id: u64,
        t: String,
        speaker: String,
        text: String,
    },
    Event {
        id: u64,
        t: String,
        text: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiStatus {
    Ready,
    Transcribing,
    Paused,
}

/// Events for the UI. Implemented by the Tauri layer; tests use a collector.
pub trait Events: Send + Sync {
    fn line(&self, l: UiLine);
    fn status(&self, s: UiStatus);
    /// A short, text-free reason (`write_failed`). Never transcript text or names.
    fn problem(&self, kind: &str);
    /// Live caption for speech in progress or just finished (ADR-006, ADR-007). `speaker` is
    /// `Me` or `Others`. Never stored or logged by the host. Default: ignore.
    fn caption(&self, _c: &UiCaption) {}
    /// Input levels, 0 to 1, for Me and Others, about ten times a second while a session runs.
    /// A number only; never audio. Default: ignore.
    fn levels(&self, _me: f32, _others: f32) {}
    /// A Me line already shown live (a final caption) was dropped as microphone bleed, so the
    /// live view can take its row back. Transient like captions: never stored or logged.
    /// Default: ignore.
    fn dropped(&self, _speaker: &'static str, _text: &str) {}
    /// What the UI wants now: `(captions on, include Me)`. Polled by the session thread.
    fn captions_wanted(&self) -> (bool, bool) {
        (false, false)
    }
}

/// A live caption for the UI. Each one replaces the previous caption of the same speaker.
#[derive(Debug, Clone, PartialEq)]
pub struct UiCaption {
    /// `Me` or `Others`.
    pub speaker: &'static str,
    /// The whole line so far.
    pub text: String,
    /// Leading words that are settled; the rest may still change.
    pub stable: usize,
    /// Utterance finished: `text` is the committed wording.
    pub is_final: bool,
    /// Rows that locked and left the live row since the last caption (ADR-008).
    pub sealed: Vec<String>,
    /// Which utterance this belongs to.
    pub utt: u64,
    /// `HH:MM:SS` where the finished utterance began (final captions only).
    pub t: Option<String>,
}

/// How often the source buttons' meters are updated.
const LEVELS_EVERY_MS: u64 = 50;

fn emit_captions(events: &dyn Events, session: &mut Session, now_ms: u64, offset_ms: u64) {
    for c in session.captions(now_ms) {
        events.caption(&UiCaption {
            speaker: if c.speaker == Speaker::Me {
                "Me"
            } else {
                "Others"
            },
            stable: c.stable,
            is_final: c.is_final,
            sealed: c.sealed,
            utt: c.utt,
            t: c.start_ms.map(|m| timestamp(m + offset_ms)),
            text: c.text,
        });
    }
}

pub struct StartParams {
    /// Transcripts folder.
    pub root: PathBuf,
    /// Name typed by the user; empty means the default title.
    pub name: String,
    pub app_version: String,
    /// `id@version` of the active transcription model.
    pub engine_label: String,
    pub commit: Config,
    /// Continue this existing transcript instead of creating a new one (SPEC 6.5).
    pub continue_from: Option<PathBuf>,
}

/// A transcription engine that finished loading, and the model it loaded (`id@version`; a
/// failed model falls back to another, so this can differ from [`StartParams::engine_label`]).
pub struct LoadedEngine {
    pub label: String,
    pub engine: Box<dyn Engine>,
}

/// The engine a session decodes with (Instant Start, SPEC 4.6).
pub enum EngineSource {
    /// Loaded and waiting (the warm model): decoding starts at once.
    Ready(Box<dyn Engine>),
    /// Still loading. Capture starts anyway and everything it hears waits in memory, in order,
    /// until the engine arrives; then it is decoded as if it had just been heard, so no words
    /// are lost. `Err` (a reason with no model or transcript text) means no engine will come.
    Loading(Receiver<Result<LoadedEngine, String>>),
}

/// The longest the session waits for a loading engine before giving up on it. Loads take
/// seconds; this only stops a load that never returns from holding audio in memory for ever.
const LOAD_WAIT_MAX: Duration = Duration::from_secs(300);

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("could not start the session: {0}")]
    Start(#[from] sprk_transcript::TranscriptError),
    #[error("the session thread stopped")]
    Gone,
}

/// Platform inputs for context detection. Built on the session thread (COM must be
/// initialized per thread), so the closure that makes them is what crosses threads.
pub struct ContextInputs {
    pub windows: Box<dyn Fn() -> Vec<Window>>,
    pub sessions: Box<dyn Fn() -> Vec<AudioSession>>,
}

/// SPEC 18 and 19 settings that reach the host. `None` disables context entirely.
pub struct ContextConfig {
    pub make_inputs: Box<dyn FnOnce() -> ContextInputs + Send>,
    pub audio_sources: bool,
    pub call_context: bool,
    pub name_one_to_one: bool,
    pub timing: Timing,
    pub exclusions: Vec<String>,
    /// Window title poll interval for call context (SPEC 19.3: 15 s, 5 to 60 s in the app).
    pub call_poll_ms: u64,
}

enum Cmd {
    Pause,
    Resume,
    End,
    Rename(String),
    SetSource(Source, bool),
    /// Event text the user added: a mark or a note (SPEC 5.1).
    UserEvent(String),
}

pub struct Host {
    tx: Sender<Cmd>,
    join: Option<JoinHandle<()>>,
    writer: SharedWriter,
}

type SharedWriter = Arc<Mutex<Option<TranscriptWriter>>>;

/// Writes to the file and mirrors each committed line to the UI.
struct UiSink {
    writer: SharedWriter,
    events: Arc<dyn Events>,
    next_id: u64,
    /// Added to every time: a continued transcript keeps counting from its last line.
    offset_ms: u64,
}

impl UiSink {
    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
}

fn lock(w: &SharedWriter) -> std::sync::MutexGuard<'_, Option<TranscriptWriter>> {
    w.lock().unwrap_or_else(|p| p.into_inner())
}

impl Sink for UiSink {
    fn statement(&mut self, seg: &Segment) -> Result<(), SessionError> {
        let source = if seg.speaker.is_remote() {
            "system"
        } else {
            "mic"
        };
        let (start_ms, end_ms) = (seg.start_ms + self.offset_ms, seg.end_ms + self.offset_ms);
        if let Some(w) = lock(&self.writer).as_mut() {
            w.append_statement(start_ms, end_ms, seg.speaker.label(), source, &seg.text)?;
        }
        let id = self.id();
        self.events.line(UiLine::Statement {
            id,
            t: timestamp(start_ms),
            speaker: seg.speaker.label().to_string(),
            text: seg.text.clone(),
        });
        Ok(())
    }

    fn event(&mut self, at_ms: u64, text: &str) -> Result<(), SessionError> {
        let at_ms = at_ms + self.offset_ms;
        if let Some(w) = lock(&self.writer).as_mut() {
            w.append_event(at_ms, text)?;
        }
        let id = self.id();
        self.events.line(UiLine::Event {
            id,
            t: timestamp(at_ms),
            text: text.to_string(),
        });
        Ok(())
    }
}

fn duration_text(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

impl Host {
    /// Create the file, then run the session on its own thread. Capture starts at once; the
    /// engine is either ready or still loading (`EngineSource`), and the VAD packs are
    /// verified already. `make_backend` runs on the capture thread because platform streams
    /// must stay on the thread that opened them.
    pub fn start(
        p: StartParams,
        engine: EngineSource,
        make_vad: VadFactory,
        make_backend: Box<dyn FnOnce() -> Box<dyn Backend> + Send>,
        clock: Box<dyn Clock>,
        events: Arc<dyn Events>,
        context: Option<ContextConfig>,
    ) -> Result<Host, HostError> {
        let stamp = clock.now();
        let title = if p.name.trim().is_empty() {
            "Transcript".to_string()
        } else {
            p.name.clone()
        };
        let source = if p.name.trim().is_empty() {
            NameSource::Default
        } else {
            NameSource::User
        };
        std::fs::create_dir_all(&p.root).map_err(sprk_transcript::TranscriptError::from)?;
        let sidecar_for = |path: &std::path::Path| {
            p.root.join(".sprknotes").join(format!(
                "{}.jsonl",
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("session")
            ))
        };
        let (writer, offset_ms) = match &p.continue_from {
            Some(existing) => TranscriptWriter::reopen(
                existing.clone(),
                Some(sidecar_for(existing)),
                &p.app_version,
                &p.engine_label,
            )?,
            None => {
                let path = unique_path(&p.root, &file_name(&stamp.date, &stamp.hhmm, &title));
                let sidecar = sidecar_for(&path);
                let meta = Meta {
                    title: title.clone(),
                    date: stamp.date.clone(),
                    start: stamp.iso.clone(),
                    end: None,
                    duration: None,
                    status: Status::InProgress,
                    sources: vec!["mic".into(), "system".into()],
                    speakers: vec!["Me".into(), "Others".into()],
                    name_source: source,
                    engine: p.engine_label.clone(),
                    app: p.app_version.clone(),
                };
                (
                    TranscriptWriter::create(path, Some(sidecar), meta, None)?,
                    0,
                )
            }
        };
        let shared: SharedWriter = Arc::new(Mutex::new(Some(writer)));
        let writer = shared.clone();
        let (tx, rx) = channel();
        let root = p.root.clone();
        let commit = p.commit.clone();
        let join = std::thread::Builder::new()
            .name("sprk-session".into())
            .spawn(move || {
                let guard = (shared.clone(), events.clone());
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(
                        rx,
                        shared,
                        engine,
                        make_vad,
                        make_backend,
                        clock,
                        events,
                        commit,
                        root,
                        context,
                        offset_ms,
                    );
                }));
                if outcome.is_err() {
                    interrupted(&guard.0, guard.1.as_ref());
                }
            })
            .map_err(|_| HostError::Gone)?;
        Ok(Host {
            tx,
            join: Some(join),
            writer,
        })
    }

    /// The file this session writes to right now (automatic renames move it), so the Library
    /// can leave it alone. None once the file is closed.
    pub fn path(&self) -> Option<std::path::PathBuf> {
        lock(&self.writer).as_ref().map(|w| w.path().to_path_buf())
    }

    /// The session thread stopped on its own (a panic, or the engine never loaded). `end`
    /// returns at once then.
    pub fn is_finished(&self) -> bool {
        self.join.as_ref().is_none_or(JoinHandle::is_finished)
    }

    fn send(&self, c: Cmd) -> Result<(), HostError> {
        self.tx.send(c).map_err(|_| HostError::Gone)
    }
    pub fn pause(&self) -> Result<(), HostError> {
        self.send(Cmd::Pause)
    }
    pub fn resume(&self) -> Result<(), HostError> {
        self.send(Cmd::Resume)
    }
    pub fn rename(&self, name: &str) -> Result<(), HostError> {
        self.send(Cmd::Rename(name.to_string()))
    }
    pub fn set_source(&self, s: Source, on: bool) -> Result<(), HostError> {
        self.send(Cmd::SetSource(s, on))
    }
    /// Drop a mark at this moment (SPEC 5.1). Works while transcribing or paused.
    pub fn mark(&self) -> Result<(), HostError> {
        self.send(Cmd::UserEvent(sprk_transcript::format::MARK.to_string()))
    }
    /// Save a short note at this moment. Returns false (and writes nothing) when the note
    /// is empty once trimmed.
    pub fn note(&self, text: &str) -> Result<bool, HostError> {
        match sprk_transcript::format::note_event(text) {
            Some(ev) => self.send(Cmd::UserEvent(ev)).map(|_| true),
            None => Ok(false),
        }
    }
    /// Flush, finalize the file and stop. Blocks until the thread is done.
    pub fn end(mut self) -> Result<(), HostError> {
        let _ = self.tx.send(Cmd::End);
        match self.join.take() {
            Some(j) => j.join().map_err(|_| HostError::Gone),
            None => Ok(()),
        }
    }
}

/// The session thread panicked: every committed line is already on disk, so close the file as
/// interrupted (SPEC 12, CLAUDE.md rule 6) and tell the UI. Nothing from the panic is logged.
fn interrupted(shared: &SharedWriter, events: &dyn Events) {
    if let Some(w) = lock(shared).take() {
        let path = w.path().to_path_buf();
        drop(w);
        if sprk_transcript::recovery::mark_interrupted(&path).is_err() {
            events.problem("finalize_failed");
        }
    }
    events.problem("session_panicked");
    events.status(UiStatus::Ready);
}

/// What happened before the engine was ready, kept in memory in order and replayed once it
/// is. Frames heard while paused are not kept: a paused session ignores them anyway.
enum Early {
    Got(Vec<Captured>, u64),
    Pause(Vec<Captured>, u64),
    Resume(Vec<Captured>, u64),
    Source(Source, bool, u64),
    /// A mark or note, after what was heard up to it.
    Event(Vec<Captured>, u64, String),
}

/// How waiting for a loading engine ended.
struct Waited {
    engine: Option<LoadedEngine>,
    early: Vec<Early>,
    /// End (or the UI went away) came while waiting.
    ended: bool,
}

/// Capture runs and commands are answered while the engine loads; nothing is decoded yet.
/// The source buttons' meters still move, so the person can see they are being heard.
fn wait_for_engine(
    loading: &Receiver<Result<LoadedEngine, String>>,
    rx: &Receiver<Cmd>,
    capture: &mut CaptureThread,
    start: &Instant,
    events: &dyn Events,
    title: &mut Option<(String, NameSource)>,
) -> Waited {
    let now_ms = |s: &Instant| s.elapsed().as_millis() as u64;
    let mut early = Vec::new();
    let mut paused = false;
    let mut enabled = [true, true];
    let mut meters: [crate::meter::Meter; 2] = Default::default();
    let mut last_levels = 0u64;
    let keep = |got: Vec<Captured>, paused: bool| -> Vec<Captured> {
        if paused {
            got.into_iter()
                .filter(|c| !matches!(c, Captured::Frame(_)))
                .collect()
        } else {
            got
        }
    };
    loop {
        match loading.try_recv() {
            Ok(Ok(e)) => {
                return Waited {
                    engine: Some(e),
                    early,
                    ended: false,
                }
            }
            Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Waited {
                    engine: None,
                    early,
                    ended: false,
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
        if start.elapsed() > LOAD_WAIT_MAX {
            return Waited {
                engine: None,
                early,
                ended: false,
            };
        }
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(Cmd::Pause) if !paused => {
                let got = keep(capture.sync(), paused);
                early.push(Early::Pause(got, now_ms(start)));
                paused = true;
                events.status(UiStatus::Paused);
            }
            Ok(Cmd::Resume) if paused => {
                let got = capture.restart();
                early.push(Early::Resume(got, now_ms(start)));
                paused = false;
                events.status(UiStatus::Transcribing);
            }
            Ok(Cmd::Pause | Cmd::Resume) => {}
            Ok(Cmd::SetSource(s, on)) => {
                capture.set_enabled(s, on);
                enabled[usize::from(s == Source::System)] = on;
                early.push(Early::Source(s, on, now_ms(start)));
            }
            Ok(Cmd::UserEvent(text)) => {
                let got = keep(capture.sync(), paused);
                early.push(Early::Event(got, now_ms(start), text));
            }
            Ok(Cmd::Rename(n)) => {
                if !n.trim().is_empty() {
                    *title = Some((n, NameSource::User));
                }
            }
            Ok(Cmd::End) | Err(RecvTimeoutError::Disconnected) => {
                // Keep everything heard up to now, then wait for the engine so it is decoded.
                let got = keep(capture.sync(), paused);
                early.push(Early::Got(got, now_ms(start)));
                let left = LOAD_WAIT_MAX.saturating_sub(start.elapsed());
                let engine = loading.recv_timeout(left).ok().and_then(Result::ok);
                return Waited {
                    engine,
                    early,
                    ended: true,
                };
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        capture.rethrow_if_failed();
        let got = keep(capture.take(), paused);
        for c in &got {
            if let Captured::Frame(f) = c {
                let i = usize::from(f.source == Source::System);
                if enabled[i] {
                    meters[i].push(&f.samples);
                }
            }
        }
        if !got.is_empty() {
            early.push(Early::Got(got, now_ms(start)));
        }
        let t = now_ms(start);
        if t.saturating_sub(last_levels) >= LEVELS_EVERY_MS {
            last_levels = t;
            events.levels(meters[0].take(), meters[1].take());
        }
    }
}

/// The engine never loaded: nothing could be transcribed. Close the file with a line that
/// says why (no model name: the reason stays generic), and tell the UI.
fn no_engine(
    shared: &SharedWriter,
    events: &dyn Events,
    clock: &dyn Clock,
    t: u64,
    offset_ms: u64,
) {
    events.problem("model_failed");
    if let Some(mut w) = lock(shared).take() {
        let _ = w.append_event(
            t + offset_ms,
            "Transcription stopped: the transcription model did not load",
        );
        if w.finalize(
            &clock.now().iso,
            &duration_text(t + offset_ms),
            Status::Complete,
        )
        .is_err()
        {
            events.problem("finalize_failed");
        }
    }
    events.status(UiStatus::Ready);
}

#[allow(clippy::too_many_arguments)]
fn run(
    rx: Receiver<Cmd>,
    shared: SharedWriter,
    engine: EngineSource,
    make_vad: VadFactory,
    make_backend: Box<dyn FnOnce() -> Box<dyn Backend> + Send>,
    clock: Box<dyn Clock>,
    events: Arc<dyn Events>,
    commit: Config,
    root: PathBuf,
    context: Option<ContextConfig>,
    offset_ms: u64,
) {
    let start = Instant::now();
    let mut capture = match CaptureThread::spawn(make_backend, start) {
        Ok(c) => c,
        // No thread to read the devices with: close the file as interrupted, as for a panic.
        Err(_) => std::panic::resume_unwind(Box::new("capture thread did not start")),
    };
    let now_ms = |s: &Instant| s.elapsed().as_millis() as u64;
    let mut title: Option<(String, NameSource)> = None;
    events.status(UiStatus::Transcribing);

    let (engine, early, ended) = match engine {
        EngineSource::Ready(e) => (e, Vec::new(), false),
        EngineSource::Loading(loading) => {
            let w = wait_for_engine(
                &loading,
                &rx,
                &mut capture,
                &start,
                events.as_ref(),
                &mut title,
            );
            let Some(loaded) = w.engine else {
                capture.stop();
                no_engine(
                    &shared,
                    events.as_ref(),
                    clock.as_ref(),
                    now_ms(&start),
                    offset_ms,
                );
                return;
            };
            if let Some(wr) = lock(&shared).as_mut() {
                wr.set_engine(&loaded.label);
            }
            (loaded.engine, w.early, w.ended)
        }
    };

    let mut ctx = context.map(Ctx::new);
    let sink = UiSink {
        writer: shared.clone(),
        events: events.clone(),
        next_id: 0,
        offset_ms,
    };
    let mut session =
        Session::new(engine, make_vad, Box::new(sink), commit).with_echo_cancellation(true);
    let mut last_tick = 0u64;
    let mut last_levels = 0u64;
    let mut last_device: [Option<String>; 2] = [None, None];

    let problem = |e: SessionError| {
        let _ = e;
        events.problem("write_failed");
    };

    // Everything heard while the engine loaded, decoded now in the order it happened.
    for e in early {
        match e {
            Early::Got(got, t) => apply(got, &mut session, &mut last_device, t, &problem),
            Early::Pause(got, t) => {
                apply(got, &mut session, &mut last_device, t, &problem);
                if let Some(c) = ctx.as_mut() {
                    c.pause(t, &mut session);
                }
                if let Err(e) = session.pause(t) {
                    problem(e);
                }
            }
            Early::Resume(got, t) => {
                apply(got, &mut session, &mut last_device, t, &problem);
                if let Err(e) = session.resume(t) {
                    problem(e);
                }
            }
            Early::Source(s, on, t) => session.set_source_enabled(s, on, t),
            Early::Event(got, t, text) => {
                apply(got, &mut session, &mut last_device, t, &problem);
                if let Err(e) = session.event(t, &text) {
                    problem(e);
                }
            }
        }
    }

    // End came while the engine loaded: the replay above was everything.
    if !ended {
        'main: loop {
            // Commands first so Pause/End act on a consistent state.
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(Cmd::Pause) => {
                    let got = capture.sync();
                    apply(
                        got,
                        &mut session,
                        &mut last_device,
                        now_ms(&start),
                        &problem,
                    );
                    if let Some(c) = ctx.as_mut() {
                        c.pause(now_ms(&start), &mut session);
                    }
                    if let Err(e) = session.pause(now_ms(&start)) {
                        problem(e);
                    }
                    emit_captions(events.as_ref(), &mut session, now_ms(&start), offset_ms);
                    events.status(UiStatus::Paused);
                }
                Ok(Cmd::Resume) => {
                    // Fresh streams: audio buffered through the pause must not be fed as new.
                    let got = capture.restart();
                    apply(
                        got,
                        &mut session,
                        &mut last_device,
                        now_ms(&start),
                        &problem,
                    );
                    if let Err(e) = session.resume(now_ms(&start)) {
                        problem(e);
                    }
                    events.status(UiStatus::Transcribing);
                }
                Ok(Cmd::SetSource(s, on)) => {
                    capture.set_enabled(s, on);
                    session.set_source_enabled(s, on, now_ms(&start));
                }
                Ok(Cmd::UserEvent(text)) => {
                    // Lines already committed land first, so the mark follows what was on screen.
                    if let Err(e) = session.event(now_ms(&start), &text) {
                        problem(e);
                    }
                }
                Ok(Cmd::Rename(n)) => {
                    if !n.trim().is_empty() {
                        title = Some((n, NameSource::User));
                    }
                }
                Ok(Cmd::End) | Err(RecvTimeoutError::Disconnected) => break 'main,
                Err(RecvTimeoutError::Timeout) => {}
            }
            let t = now_ms(&start);
            // A capture panic carries on here, so the file closes as interrupted as it did when
            // capture ran on this thread.
            capture.rethrow_if_failed();
            let got = capture.take();
            apply(got, &mut session, &mut last_device, t, &problem);
            let (cap_on, cap_me) = events.captions_wanted();
            session.set_captions(cap_on, cap_me);
            emit_captions(events.as_ref(), &mut session, t, offset_ms);
            if t.saturating_sub(last_levels) >= LEVELS_EVERY_MS {
                last_levels = t;
                let [me, others] = session.levels();
                events.levels(me, others);
            }
            if let Some(c) = ctx.as_mut() {
                if session.state() == crate::session::State::Running {
                    c.poll(t, &mut session);
                }
            }
            if t.saturating_sub(last_tick) >= 1_000 {
                last_tick = t;
                if let Err(e) = session.tick(t) {
                    problem(e);
                }
                for d in session.take_dropped() {
                    if d.speaker == Speaker::Me {
                        events.dropped("Me", &d.text);
                    }
                }
            }
        }
    }

    // End (or the UI went away): flush, then finalize the file.
    let t = now_ms(&start);
    let got = capture.sync();
    apply(got, &mut session, &mut last_device, t, &problem);
    capture.stop();
    if let Some(c) = ctx.as_mut() {
        if session.state() == crate::session::State::Running {
            c.pause(t, &mut session);
        }
    }
    if let Err(e) = session.end(t) {
        problem(e);
    }
    emit_captions(events.as_ref(), &mut session, t, offset_ms);
    drop(session); // releases the sink's handle on the writer
    if let Some(mut w) = lock(&shared).take() {
        if let Some((name, src)) = title.take() {
            let stamp = clock.now();
            // Stay in the transcript's own folder (a continued one may live in a subfolder).
            let dir = w.path().parent().map_or(root.clone(), |d| d.to_path_buf());
            let new_path = unique_path(&dir, &file_name(&stamp.date, &stamp.hhmm, &name));
            let _ = w.rename(new_path, &name, src);
        }
        if w.finalize(
            &clock.now().iso,
            &duration_text(t + offset_ms),
            Status::Complete,
        )
        .is_err()
        {
            events.problem("finalize_failed");
        }
    }
    events.status(UiStatus::Ready);
}

/// Context detection running on the session thread: audio sources (SPEC 18) and call context
/// (SPEC 19). Titles and names only ever go into transcript event lines (CLAUDE.md rule 11).
struct Ctx {
    cfg: ContextConfig,
    inputs: Option<ContextInputs>,
    poller: SourcePoller,
    labels: LabelTracker,
    rules: TitleRules,
    last_sources_ms: Option<u64>,
    last_call_ms: Option<u64>,
    windows: Vec<Window>,
    last_label: String,
}

impl Ctx {
    fn new(mut cfg: ContextConfig) -> Self {
        let make = std::mem::replace(&mut cfg.make_inputs, Box::new(unreachable_inputs));
        let inputs = Some(make());
        let poller = SourcePoller::new(ActivityTracker::new(
            cfg.timing.clone(),
            Exclusions(cfg.exclusions.clone()),
        ));
        Ctx {
            cfg,
            inputs,
            poller,
            labels: LabelTracker::new(),
            rules: TitleRules::default(),
            last_sources_ms: None,
            last_call_ms: None,
            windows: Vec::new(),
            last_label: "Others".into(),
        }
    }

    fn write(session: &mut Session, at_ms: u64, text: &str) {
        let _ = session.event(at_ms, text);
    }

    fn poll(&mut self, t: u64, session: &mut Session) {
        let Some(inputs) = self.inputs.as_ref() else {
            return;
        };
        if self.cfg.audio_sources
            && self
                .last_sources_ms
                .is_none_or(|l| t.saturating_sub(l) >= 250)
        {
            self.last_sources_ms = Some(t);
            // Titles come from a window list refreshed at most every 2 s.
            if self.windows.is_empty() || t % 2_000 < 250 {
                self.windows = (inputs.windows)();
            }
            let sessions = (inputs.sessions)();

            // Audio source activity is tracked (it can name an unnamed transcript) but is not
            // written into the transcript: start and end lines were noise for the reader.
            let _ = self.poller.poll(t, &sessions, &[], &NoTabs, &self.windows);
        }
        if self.cfg.call_context
            && self
                .last_call_ms
                .is_none_or(|l| t.saturating_sub(l) >= self.cfg.call_poll_ms)
        {
            self.last_call_ms = Some(t);
            let wins = (inputs.windows)();
            let obs = self.observe(&wins);
            if let Some(change) = self.labels.observe(&obs, t) {
                Self::write(session, change.at_ms, &change.text);
            }
            // Never guess: a name is only used once the mode is confirmed.
            let label = self.labels.remote_label().to_string();
            if label != self.last_label {
                session.set_remote_speaker(if label == "Others" {
                    Speaker::Others
                } else {
                    Speaker::Named(label.clone())
                });
                self.last_label = label;
            }
        }
    }

    /// Strongest signal among the Teams windows: a parsed call beats Unknown.
    fn observe(&self, wins: &[Window]) -> Observation {
        let mut best = Observation::Unknown;
        for w in wins {
            if !self
                .rules
                .strip_suffixes
                .iter()
                .any(|s| w.title.ends_with(s.as_str()))
            {
                continue;
            }
            match parse_call_title(&w.title, &self.rules) {
                Observation::Unknown => {}
                Observation::OneToOne(n) => {
                    if !self.cfg.name_one_to_one {
                        return Observation::Unknown;
                    }
                    if best == Observation::Unknown {
                        best = Observation::OneToOne(n);
                    }
                }
                Observation::Group => return Observation::Group,
            }
        }
        best
    }

    fn pause(&mut self, t: u64, _session: &mut Session) {
        let _ = self.poller.pause(t);
    }
}

fn unreachable_inputs() -> ContextInputs {
    ContextInputs {
        windows: Box::new(Vec::new),
        sessions: Box::new(Vec::new),
    }
}

fn label(s: Source) -> &'static str {
    match s {
        Source::Mic => "microphone",
        Source::System => "system audio",
    }
}

/// A device name as shown in the transcript: the text before any platform tag, no control characters.
fn device_label(name: &str) -> String {
    name.split('\u{1f}')
        .next()
        .unwrap_or(name)
        .chars()
        .filter(|c| !c.is_control())
        .collect()
}

/// Hand what capture produced to the session, in order: device notices become event lines,
/// the device in use is named once each time it changes (the transcript is the only place names
/// may go; a wrong headset or speaker is otherwise invisible), and audio is fed.
fn apply(
    got: Vec<Captured>,
    session: &mut Session,
    last_device: &mut [Option<String>; 2],
    t: u64,
    problem: &dyn Fn(SessionError),
) {
    for c in got {
        match c {
            Captured::Frame(f) => session.feed(f.source, &f.samples),
            Captured::Event(ev) => {
                session.capture_changed();
                let text = match ev {
                    CaptureEvent::DeviceChanged(s) => format!("Source changed: {}", label(s)),
                    CaptureEvent::Unavailable(s) => format!("Source unavailable: {}", label(s)),
                    CaptureEvent::Restored(s) => format!("Source restored: {}", label(s)),
                };
                if let Err(e) = session.event(t, &text) {
                    problem(e);
                }
            }
            Captured::Device(s, Some(name)) => {
                let shown = match s {
                    Source::Mic => "Microphone",
                    Source::System => "Others audio",
                };
                let now = Some(device_label(&name));
                let slot = &mut last_device[usize::from(s == Source::System)];
                if now != *slot {
                    if let Some(n) = &now {
                        if let Err(e) = session.event(t, &format!("{shown}: {n}")) {
                            problem(e);
                        }
                    }
                    *slot = now;
                }
            }
            Captured::Device(_, None) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprk_asr::{AsrError, Segment as AsrSeg, Utterance, Vad};
    use sprk_audio::live::PcmStream;
    use sprk_audio::AudioError;
    use sprk_context::activity::Timing;

    struct ChunkVad(u64);
    impl Vad for ChunkVad {
        fn accept(&mut self, s: &[f32]) -> Vec<Utterance> {
            let u = Utterance {
                start_sample: self.0,
                samples: s.to_vec(),
            };
            self.0 += s.len() as u64;
            vec![u]
        }
        fn flush(&mut self) -> Vec<Utterance> {
            vec![]
        }
    }

    struct QueueEngine(Arc<Mutex<Vec<String>>>);
    impl Engine for QueueEngine {
        fn transcribe(&mut self, s: &[f32]) -> Result<Vec<AsrSeg>, AsrError> {
            let mut q = self.0.lock().unwrap();
            if q.is_empty() {
                return Err(AsrError::Rejected);
            }
            Ok(vec![AsrSeg {
                start_ms: 0,
                end_ms: s.len() as u64 * 1000 / 16_000,
                text: q.remove(0),
            }])
        }
    }

    /// One second of audio for the mic, delivered once.
    struct OnceStream(Arc<Mutex<Option<Vec<f32>>>>);
    impl PcmStream for OnceStream {
        fn read(&mut self, out: &mut Vec<f32>) {
            if let Some(v) = self.0.lock().unwrap().take() {
                out.extend(v);
            }
        }
    }
    struct OneMicBackend(Arc<Mutex<Option<Vec<f32>>>>);
    impl Backend for OneMicBackend {
        fn default_device(&self, s: Source) -> Option<String> {
            (s == Source::Mic).then(|| "mic".to_string())
        }
        fn open(&mut self, _: Source, _: &str) -> Result<Box<dyn PcmStream>, AudioError> {
            Ok(Box::new(OnceStream(self.0.clone())))
        }
    }

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> Stamp {
            Stamp {
                date: "2026-11-19".into(),
                hhmm: "0900".into(),
                iso: "2026-11-19T09:00:00-06:00".into(),
            }
        }
    }

    #[derive(Default)]
    struct Collect {
        lines: Mutex<Vec<UiLine>>,
        statuses: Mutex<Vec<UiStatus>>,
        problems: Mutex<Vec<String>>,
        levels: Mutex<Vec<(f32, f32)>>,
    }
    impl Events for Collect {
        fn levels(&self, me: f32, others: f32) {
            self.levels.lock().unwrap().push((me, others));
        }
        fn line(&self, l: UiLine) {
            self.lines.lock().unwrap().push(l);
        }
        fn status(&self, s: UiStatus) {
            self.statuses.lock().unwrap().push(s);
        }
        fn problem(&self, k: &str) {
            self.problems.lock().unwrap().push(k.into());
        }
    }

    fn launch(name: &str, answers: &[&str]) -> (Host, Arc<Collect>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let ev = Arc::new(Collect::default());
        let audio = Arc::new(Mutex::new(Some(vec![0.1f32; 16_000])));
        let q = Arc::new(Mutex::new(
            answers.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        ));
        let host = Host::start(
            StartParams {
                root: dir.path().to_path_buf(),
                name: name.into(),
                app_version: "0.0.1".into(),
                engine_label: "test@2026.11.0".into(),
                commit: Config::default(),
                continue_from: None,
            },
            EngineSource::Ready(Box::new(QueueEngine(q))),
            Box::new(|| Box::new(ChunkVad(0))),
            Box::new(move || Box::new(OneMicBackend(audio))),
            Box::new(FixedClock),
            ev.clone(),
            None,
        )
        .unwrap();
        (host, ev, dir)
    }

    fn md_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "md"))
            .collect()
    }

    #[test]
    fn continuing_a_transcript_appends_to_the_same_file_with_later_times() {
        let (host, _ev, dir) = launch("Standup", &["first part"]);
        std::thread::sleep(Duration::from_millis(300));
        host.end().unwrap();
        let files = md_files(dir.path());
        assert_eq!(files.len(), 1);
        let before = std::fs::read_to_string(&files[0]).unwrap();

        let ev = Arc::new(Collect::default());
        let audio = Arc::new(Mutex::new(Some(vec![0.1f32; 16_000])));
        let q = Arc::new(Mutex::new(vec!["second part".to_string()]));
        let host = Host::start(
            StartParams {
                root: dir.path().to_path_buf(),
                name: String::new(),
                app_version: "0.0.2".into(),
                engine_label: "test@2026.11.0".into(),
                commit: Config::default(),
                continue_from: Some(files[0].clone()),
            },
            EngineSource::Ready(Box::new(QueueEngine(q))),
            Box::new(|| Box::new(ChunkVad(0))),
            Box::new(move || Box::new(OneMicBackend(audio))),
            Box::new(FixedClock),
            ev.clone(),
            None,
        )
        .unwrap();
        assert_eq!(host.path(), Some(files[0].clone()));
        std::thread::sleep(Duration::from_millis(300));
        host.end().unwrap();
        assert_eq!(md_files(dir.path()), files, "no second file");
        let text = std::fs::read_to_string(&files[0]).unwrap();
        assert!(text.contains("status: complete"));
        assert!(text.contains("title: \"Standup\""), "{text}");
        assert!(text.contains("**Me:** first part"));
        assert!(text.contains("Transcription continued"));
        assert!(text.contains("**Me:** second part"));
        let p = sprk_transcript::parse::Parsed::parse(&text, None);
        let first = p
            .segments
            .iter()
            .find(|s| s.text == "first part")
            .unwrap()
            .start_ms;
        let second = p
            .segments
            .iter()
            .find(|s| s.text == "second part")
            .unwrap()
            .start_ms;
        assert!(second > first, "{first} then {second}");
        assert!(before.len() < text.len());
        // The UI got the continued times too.
        assert!(ev
            .lines
            .lock()
            .unwrap()
            .iter()
            .any(|l| matches!(l, UiLine::Statement { t, .. } if t != "00:00:00")));
    }

    #[test]
    fn levels_reach_the_ui_for_the_source_that_has_sound() {
        let (host, ev, _dir) = launch("Levels", &["hello"]);
        // Wait for the levels rather than a fixed time: a busy CI runner can take
        // longer than a few hundred milliseconds just to start the session thread.
        let heard = |l: &[(f32, f32)]| {
            l.len() >= 2 && l.iter().any(|&(me, _)| (me - 2.0 / 3.0).abs() < 0.01)
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !heard(&ev.levels.lock().unwrap()) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        host.end().unwrap();
        let levels = ev.levels.lock().unwrap().clone();
        assert!(levels.len() >= 2, "about ten a second: {levels:?}");
        // The fake mic delivers one second at 0.1 (-20 dBFS); there is no system device.
        assert!(
            levels.iter().any(|&(me, _)| (me - 2.0 / 3.0).abs() < 0.01),
            "{levels:?}"
        );
        assert!(levels.iter().all(|&(_, others)| others == 0.0));
    }

    #[test]
    fn full_session_writes_a_complete_transcript() {
        let (host, ev, dir) = launch("Weekly sync", &["hello everyone welcome"]);
        std::thread::sleep(Duration::from_millis(300));
        host.pause().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        host.resume().unwrap();
        host.end().unwrap();
        let files = md_files(dir.path());
        assert_eq!(files.len(), 1);
        assert!(files[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("2026-11-19 0900 Weekly sync"));
        let text = std::fs::read_to_string(&files[0]).unwrap();
        assert!(text.contains("status: complete"));
        assert!(text.contains("name_source: user"));
        assert!(text.contains("**Me:** hello everyone welcome"));
        assert!(text.contains("Transcription paused"));
        assert!(text.contains("Transcription resumed"));
        // The device in use is named once, so a wrong headset is visible in the transcript.
        assert_eq!(text.matches("Microphone: mic").count(), 1);
        assert!(!text.contains("Others audio:"), "no device, no line");
        let st = ev.statuses.lock().unwrap().clone();
        assert_eq!(st.first(), Some(&UiStatus::Transcribing));
        assert!(st.contains(&UiStatus::Paused));
        assert_eq!(st.last(), Some(&UiStatus::Ready));
        assert!(ev
            .lines
            .lock()
            .unwrap()
            .iter()
            .any(|l| matches!(l, UiLine::Statement { speaker, .. } if speaker == "Me")));
        assert!(ev.problems.lock().unwrap().is_empty());
    }

    #[test]
    fn marks_and_notes_land_in_the_file_as_timestamped_event_lines() {
        let (host, ev, dir) = launch("Visit", &["blood pressure is fine"]);
        std::thread::sleep(Duration::from_millis(300));
        host.mark().unwrap();
        assert!(host.note("  recheck in two weeks\n").unwrap());
        assert!(!host.note(" \n ").unwrap(), "an empty note is not saved");
        host.pause().unwrap();
        // Paused still takes notes.
        assert!(host.note("asked about diet").unwrap());
        host.end().unwrap();
        let text = std::fs::read_to_string(&md_files(dir.path())[0]).unwrap();
        let p = sprk_transcript::parse::Parsed::parse(&text, None);
        let kinds: Vec<_> = p
            .events
            .iter()
            .map(|e| sprk_transcript::format::event_kind(&e.text))
            .filter(|k| *k != sprk_transcript::format::EventKind::Other)
            .collect();
        use sprk_transcript::format::EventKind;
        assert_eq!(
            kinds,
            vec![
                EventKind::Mark,
                EventKind::Note("recheck in two weeks"),
                EventKind::Note("asked about diet"),
            ]
        );
        // The line spoken before the mark is written before it.
        let said = text.find("**Me:** blood pressure is fine").unwrap();
        let mark = text.find("` Mark\n").unwrap();
        assert!(said < mark, "{text}");
        assert!(text.contains("status: complete"));
        // The live view gets the same lines.
        let lines = ev.lines.lock().unwrap();
        assert!(lines
            .iter()
            .any(|l| matches!(l, UiLine::Event { text, .. } if text == "Mark")));
        assert!(lines.iter().any(
            |l| matches!(l, UiLine::Event { text, .. } if text == "Note: recheck in two weeks")
        ));
    }

    #[test]
    fn rename_during_the_session_wins_at_the_end() {
        let (host, _ev, dir) = launch("", &["some words to commit here"]);
        let first = host.path().unwrap();
        assert!(first.to_string_lossy().contains("Transcript"));
        host.rename("Budget review").unwrap();
        std::thread::sleep(Duration::from_millis(200));
        // The live file keeps its name until End; path() still points at it.
        let live = host.path().unwrap();
        assert_eq!(live, first);
        assert!(live.exists());
        host.end().unwrap();
        let files = md_files(dir.path());
        assert_eq!(files.len(), 1);
        assert!(files[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("Budget review"));
        let text = std::fs::read_to_string(&files[0]).unwrap();
        assert!(text.contains("title: \"Budget review\""));
        assert!(text.contains("name_source: user"));
    }

    #[test]
    fn a_dropped_host_still_leaves_every_committed_line_on_disk() {
        let (host, _ev, dir) = launch("Crash test", &["this line is committed"]);
        std::thread::sleep(Duration::from_millis(300));
        // Simulate the UI going away without End: the thread finalizes on disconnect.
        drop(host);
        std::thread::sleep(Duration::from_millis(300));
        let files = md_files(dir.path());
        let text = std::fs::read_to_string(&files[0]).unwrap();
        assert!(text.contains("this line is committed"));
    }

    #[test]
    fn default_name_uses_transcript_and_default_source() {
        let (host, _ev, dir) = launch("  ", &[]);
        host.end().unwrap();
        let files = md_files(dir.path());
        assert!(files[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("Transcript"));
        let text = std::fs::read_to_string(&files[0]).unwrap();
        assert!(text.contains("name_source: default"));
    }

    fn launch_with_context(cfg: ContextConfig) -> (Host, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let host = Host::start(
            StartParams {
                root: dir.path().to_path_buf(),
                name: "Ctx".into(),
                app_version: "0.0.1".into(),
                engine_label: "test@2026.11.0".into(),
                commit: Config::default(),
                continue_from: None,
            },
            EngineSource::Ready(Box::new(QueueEngine(Arc::new(Mutex::new(vec![]))))),
            Box::new(|| Box::new(ChunkVad(0))),
            Box::new(|| Box::new(OneMicBackend(Arc::new(Mutex::new(None))))),
            Box::new(FixedClock),
            Arc::new(Collect::default()),
            Some(cfg),
        )
        .unwrap();
        (host, dir)
    }

    fn ctx_cfg(windows: Vec<Window>, sessions: Vec<AudioSession>) -> ContextConfig {
        ContextConfig {
            make_inputs: Box::new(move || ContextInputs {
                windows: Box::new(move || windows.clone()),
                sessions: Box::new(move || sessions.clone()),
            }),
            audio_sources: true,
            call_context: true,
            name_one_to_one: true,
            timing: Timing {
                merge_general_ms: 30_000,
                merge_call_ms: 120_000,
                min_activity_ms: 1_000, // the tracker clamps to SPEC 18.4's 1 s floor
            },
            exclusions: vec![],
            call_poll_ms: 40,
        }
    }

    fn only_md(dir: &std::path::Path) -> String {
        std::fs::read_to_string(&md_files(dir)[0]).unwrap()
    }

    #[test]
    fn one_to_one_call_is_confirmed_by_two_polls_and_written_as_an_event() {
        let win = vec![Window {
            pid: 1,
            title: "Sam Sample | Microsoft Teams".into(),
        }];
        let (host, dir) = launch_with_context(ctx_cfg(win, vec![]));
        std::thread::sleep(Duration::from_millis(400));
        host.end().unwrap();
        let text = only_md(dir.path());
        assert!(
            text.contains("Call with Sam Sample: remote speaker labeled Sam Sample"),
            "{text}"
        );
        // Finalize rebuilt the participants section from that event.
        assert!(text.contains("## Participants"));
    }

    #[test]
    fn group_titles_label_others_and_names_can_be_switched_off() {
        let group = vec![Window {
            pid: 1,
            title: "Weekly meeting | Microsoft Teams".into(),
        }];
        let (host, dir) = launch_with_context(ctx_cfg(group, vec![]));
        std::thread::sleep(Duration::from_millis(400));
        host.end().unwrap();
        assert!(only_md(dir.path())
            .contains("Call became a group call: remote speakers labeled Others"));

        let one = vec![Window {
            pid: 1,
            title: "Sam Sample | Microsoft Teams".into(),
        }];
        let mut cfg = ctx_cfg(one, vec![]);
        cfg.name_one_to_one = false;
        let (host, dir) = launch_with_context(cfg);
        std::thread::sleep(Duration::from_millis(300));
        host.end().unwrap();
        assert!(!only_md(dir.path()).contains("Sam Sample"));
    }

    #[test]
    fn audible_apps_write_no_audio_source_lines() {
        let sess = vec![AudioSession {
            owner_pid: 7,
            exe: "firefox".into(),
            peak: 0.2,
            muted: false,
            ignorable: false,
        }];
        let win = vec![Window {
            pid: 7,
            title: "Safety training - Mozilla Firefox".into(),
        }];
        let (host, dir) = launch_with_context(ctx_cfg(win, sess));
        std::thread::sleep(Duration::from_millis(2_400));
        host.end().unwrap();
        let text = only_md(dir.path());
        assert!(!text.contains("Audio source"), "{text}");
        assert!(!text.contains("Safety training"), "{text}");
    }

    struct PanicEngine;
    impl Engine for PanicEngine {
        fn transcribe(&mut self, _: &[f32]) -> Result<Vec<AsrSeg>, AsrError> {
            panic!("engine blew up");
        }
    }

    #[test]
    fn a_panic_closes_the_file_as_interrupted_and_keeps_committed_lines() {
        let dir = tempfile::tempdir().unwrap();
        let ev = Arc::new(Collect::default());
        let audio = Arc::new(Mutex::new(Some(vec![0.1f32; 16_000])));
        let host = Host::start(
            StartParams {
                root: dir.path().to_path_buf(),
                name: "Crash".into(),
                app_version: "0.0.1".into(),
                engine_label: "test@2026.11.0".into(),
                commit: Config::default(),
                continue_from: None,
            },
            EngineSource::Ready(Box::new(PanicEngine)),
            Box::new(|| Box::new(ChunkVad(0))),
            Box::new(move || Box::new(OneMicBackend(audio))),
            Box::new(FixedClock),
            ev.clone(),
            None,
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(400));
        host.end().unwrap();
        let text = only_md(dir.path());
        assert!(text.contains("status: interrupted"), "{text}");
        assert!(text.contains("Transcription interrupted"));
        assert!(ev
            .problems
            .lock()
            .unwrap()
            .contains(&"session_panicked".to_string()));
    }

    struct PanicBackend;
    impl Backend for PanicBackend {
        fn default_device(&self, _: Source) -> Option<String> {
            panic!("device call failed");
        }
        fn open(&mut self, _: Source, _: &str) -> Result<Box<dyn PcmStream>, AudioError> {
            Err(AudioError::Unavailable)
        }
    }

    #[test]
    fn a_capture_panic_still_closes_the_file_as_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let ev = Arc::new(Collect::default());
        let host = Host::start(
            StartParams {
                root: dir.path().to_path_buf(),
                name: "Capture crash".into(),
                app_version: "0.0.1".into(),
                engine_label: "test@2026.11.0".into(),
                commit: Config::default(),
                continue_from: None,
            },
            EngineSource::Ready(Box::new(QueueEngine(Arc::new(Mutex::new(vec![]))))),
            Box::new(|| Box::new(ChunkVad(0))),
            Box::new(|| Box::new(PanicBackend)),
            Box::new(FixedClock),
            ev.clone(),
            None,
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(400));
        host.end().unwrap();
        let text = only_md(dir.path());
        assert!(text.contains("status: interrupted"), "{text}");
        assert!(ev
            .problems
            .lock()
            .unwrap()
            .contains(&"session_panicked".to_string()));
    }

    /// Steady 20 ms reads; logs when each read happened.
    struct Steady(Arc<Mutex<Vec<Instant>>>, Option<Instant>);
    impl PcmStream for Steady {
        fn read(&mut self, out: &mut Vec<f32>) {
            let now = Instant::now();
            self.0.lock().unwrap().push(now);
            let ms = self
                .1
                .map_or(20, |l| now.duration_since(l).as_millis() as usize);
            self.1 = Some(now);
            out.extend(std::iter::repeat_n(0.1f32, ms * 16));
        }
    }
    struct SteadyMic(Arc<Mutex<Vec<Instant>>>);
    impl Backend for SteadyMic {
        fn default_device(&self, s: Source) -> Option<String> {
            (s == Source::Mic).then(|| "mic".to_string())
        }
        fn open(&mut self, _: Source, _: &str) -> Result<Box<dyn PcmStream>, AudioError> {
            Ok(Box::new(Steady(self.0.clone(), None)))
        }
    }
    /// A decode that takes 300 ms, like a long utterance on a slow laptop.
    struct SlowEngine;
    impl Engine for SlowEngine {
        fn transcribe(&mut self, s: &[f32]) -> Result<Vec<AsrSeg>, AsrError> {
            std::thread::sleep(Duration::from_millis(300));
            Ok(vec![AsrSeg {
                start_ms: 0,
                end_ms: s.len() as u64 * 1000 / 16_000,
                text: "words".into(),
            }])
        }
    }
    /// One utterance per 200 ms of audio, so the session thread is nearly always decoding.
    struct EveryFifthVad(u64, usize);
    impl Vad for EveryFifthVad {
        fn accept(&mut self, s: &[f32]) -> Vec<Utterance> {
            self.1 += s.len();
            if self.1 < 3_200 {
                return vec![];
            }
            let u = Utterance {
                start_sample: self.0,
                samples: vec![0.1; self.1],
            };
            self.0 += self.1 as u64;
            self.1 = 0;
            vec![u]
        }
        fn flush(&mut self) -> Vec<Utterance> {
            vec![]
        }
    }

    #[test]
    fn slow_decodes_do_not_hold_up_reading_the_devices() {
        let dir = tempfile::tempdir().unwrap();
        let reads = Arc::new(Mutex::new(Vec::new()));
        let r = reads.clone();
        let host = Host::start(
            StartParams {
                root: dir.path().to_path_buf(),
                name: "Slow".into(),
                app_version: "0.0.1".into(),
                engine_label: "test@2026.11.0".into(),
                commit: Config::default(),
                continue_from: None,
            },
            EngineSource::Ready(Box::new(SlowEngine)),
            Box::new(|| Box::new(EveryFifthVad(0, 0))),
            Box::new(move || Box::new(SteadyMic(r))),
            Box::new(FixedClock),
            Arc::new(Collect::default()),
            None,
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(1_500));
        host.end().unwrap();
        let r = reads.lock().unwrap().clone();
        let worst = r.windows(2).map(|w| w[1] - w[0]).max().unwrap_or_default();
        assert!(r.len() >= 40, "only {} reads in 1.5 s", r.len());
        assert!(
            worst < Duration::from_millis(150),
            "a read waited {worst:?} behind a decode"
        );
        assert!(only_md(dir.path()).contains("**Me:** words"));
    }

    /// Mic audio whose every sample says where it was in the stream: sample k is
    /// 0.5 + k * 2^-24 (exact in f32), so lost, repeated or reordered audio shows.
    struct Ramp {
        next: Arc<Mutex<u64>>,
        last: Option<Instant>,
    }
    fn ramp_value(k: u64) -> f32 {
        0.5 + k as f32 / 16_777_216.0
    }
    impl PcmStream for Ramp {
        fn read(&mut self, out: &mut Vec<f32>) {
            let now = Instant::now();
            let ms = self
                .last
                .map_or(20, |l| now.duration_since(l).as_millis() as u64);
            self.last = Some(now);
            let mut next = self.next.lock().unwrap();
            for _ in 0..ms * 16 {
                out.push(ramp_value(*next));
                *next += 1;
            }
        }
    }
    struct RampMic(Arc<Mutex<u64>>);
    impl Backend for RampMic {
        fn default_device(&self, s: Source) -> Option<String> {
            (s == Source::Mic).then(|| "mic".to_string())
        }
        fn open(&mut self, _: Source, _: &str) -> Result<Box<dyn PcmStream>, AudioError> {
            Ok(Box::new(Ramp {
                next: self.0.clone(),
                last: None,
            }))
        }
    }
    /// Keeps every sample it is asked to decode, in order, and says nothing.
    struct Tally(Arc<Mutex<Vec<f32>>>);
    impl Engine for Tally {
        fn transcribe(&mut self, s: &[f32]) -> Result<Vec<AsrSeg>, AsrError> {
            self.0.lock().unwrap().extend_from_slice(s);
            Ok(vec![AsrSeg {
                start_ms: 0,
                end_ms: s.len() as u64 * 1000 / 16_000,
                text: "heard".into(),
            }])
        }
    }

    /// Start with the engine still loading; it arrives after `late` (or fails to, with `None`).
    fn launch_loading(
        late: Duration,
        engine: Option<Box<dyn Engine>>,
    ) -> (Host, Arc<Collect>, Arc<Mutex<u64>>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let ev = Arc::new(Collect::default());
        let delivered = Arc::new(Mutex::new(0u64));
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            std::thread::sleep(late);
            let _ = tx.send(match engine {
                Some(engine) => Ok(LoadedEngine {
                    label: "late@2026.11.1".into(),
                    engine,
                }),
                None => Err("did not load".into()),
            });
        });
        let d = delivered.clone();
        let host = Host::start(
            StartParams {
                root: dir.path().to_path_buf(),
                name: "Instant".into(),
                app_version: "0.0.1".into(),
                engine_label: "test@2026.11.0".into(),
                commit: Config::default(),
                continue_from: None,
            },
            EngineSource::Loading(rx),
            Box::new(|| Box::new(ChunkVad(0))),
            Box::new(move || Box::new(RampMic(d))),
            Box::new(FixedClock),
            ev.clone(),
            None,
        )
        .unwrap();
        // As the app does for a source switched off before Start; also keeps the echo
        // canceller (which needs both streams) out of the way of the exact sample check.
        host.set_source(Source::System, false).unwrap();
        (host, ev, delivered, dir)
    }

    /// The decoded audio is the stream from its first sample, in order, with nothing missing.
    fn assert_whole_stream_in_order(heard: &[f32], at_least: u64) {
        let real: Vec<f32> = heard.iter().copied().filter(|v| *v != 0.0).collect();
        assert!(
            real.len() as u64 >= at_least,
            "decoded {} of {at_least} samples",
            real.len()
        );
        for (k, v) in real.iter().enumerate() {
            assert_eq!(*v, ramp_value(k as u64), "sample {k} lost or out of order");
        }
    }

    #[test]
    fn audio_heard_while_the_engine_loads_is_decoded_in_order_once_it_arrives() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let (host, ev, delivered, dir) = launch_loading(
            Duration::from_millis(500),
            Some(Box::new(Tally(heard.clone()))),
        );
        // Transcribing from the first moment, before the engine exists.
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            ev.statuses.lock().unwrap().first(),
            Some(&UiStatus::Transcribing)
        );
        assert!(heard.lock().unwrap().is_empty(), "nothing decoded yet");
        std::thread::sleep(Duration::from_millis(900));
        let before_end = *delivered.lock().unwrap();
        host.end().unwrap();
        assert!(before_end > 8_000, "the fake mic ran: {before_end}");
        assert_whole_stream_in_order(&heard.lock().unwrap(), before_end);
        let text = only_md(dir.path());
        assert!(text.contains("**Me:** heard"), "{text}");
        assert!(
            text.contains("engine: \"late@2026.11.1\"") || text.contains("engine: late@2026.11.1"),
            "{text}"
        );
        assert!(text.contains("status: complete"));
        // The meters moved while the engine loaded.
        assert!(ev.levels.lock().unwrap().iter().any(|&(me, _)| me > 0.0));
        assert!(ev.problems.lock().unwrap().is_empty());
    }

    #[test]
    fn end_while_the_engine_loads_waits_for_it_and_keeps_every_word() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let (host, ev, delivered, dir) = launch_loading(
            Duration::from_millis(700),
            Some(Box::new(Tally(heard.clone()))),
        );
        std::thread::sleep(Duration::from_millis(300));
        let before_end = *delivered.lock().unwrap();
        host.end().unwrap();
        assert!(before_end > 0);
        assert_whole_stream_in_order(&heard.lock().unwrap(), before_end);
        let text = only_md(dir.path());
        assert!(text.contains("**Me:** heard"), "{text}");
        assert!(text.contains("status: complete"));
        assert_eq!(ev.statuses.lock().unwrap().last(), Some(&UiStatus::Ready));
    }

    #[test]
    fn pause_and_resume_while_the_engine_loads_land_in_the_transcript_in_order() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let (host, ev, _delivered, dir) = launch_loading(
            Duration::from_millis(600),
            Some(Box::new(Tally(heard.clone()))),
        );
        std::thread::sleep(Duration::from_millis(150));
        host.pause().unwrap();
        std::thread::sleep(Duration::from_millis(200));
        host.resume().unwrap();
        std::thread::sleep(Duration::from_millis(700));
        host.end().unwrap();
        let st = ev.statuses.lock().unwrap().clone();
        assert_eq!(
            &st[..3],
            &[
                UiStatus::Transcribing,
                UiStatus::Paused,
                UiStatus::Transcribing
            ]
        );
        let text = only_md(dir.path());
        let paused = text.find("Transcription paused").expect("paused line");
        let resumed = text.find("Transcription resumed").expect("resumed line");
        let first = text.find("**Me:** heard").expect("a line before the pause");
        assert!(first < paused && paused < resumed, "{text}");
        assert!(text[resumed..].contains("**Me:** heard"), "{text}");
    }

    #[test]
    fn a_mark_and_a_note_made_while_the_engine_loads_land_after_what_was_heard() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let (host, ev, _delivered, dir) =
            launch_loading(Duration::from_millis(600), Some(Box::new(Tally(heard))));
        std::thread::sleep(Duration::from_millis(250));
        host.mark().unwrap();
        assert!(host.note("before the model was ready").unwrap());
        std::thread::sleep(Duration::from_millis(700));
        host.end().unwrap();
        let text = only_md(dir.path());
        let first = text.find("**Me:** heard").expect("a line before the mark");
        let mark = text.find("` Mark\n").expect("the mark");
        let note = text.find("before the model was ready").expect("the note");
        assert!(first < mark && mark < note, "{text}");
        assert!(text.contains("status: complete"));
        assert!(ev.problems.lock().unwrap().is_empty());
    }

    #[test]
    fn an_engine_that_never_loads_closes_the_file_and_says_so() {
        let (host, ev, _delivered, dir) = launch_loading(Duration::from_millis(200), None);
        std::thread::sleep(Duration::from_millis(500));
        assert!(host.is_finished(), "the session stopped by itself");
        host.end().unwrap();
        let text = only_md(dir.path());
        assert!(
            text.contains("Transcription stopped: the transcription model did not load"),
            "{text}"
        );
        assert!(text.contains("status: complete"));
        assert!(ev
            .problems
            .lock()
            .unwrap()
            .contains(&"model_failed".to_string()));
        assert_eq!(ev.statuses.lock().unwrap().last(), Some(&UiStatus::Ready));
    }

    #[test]
    fn a_shared_engine_serves_one_session_after_another() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let shared = sprk_asr::SharedEngine::new(Box::new(Tally(heard.clone())));
        for name in ["First", "Second"] {
            let dir = tempfile::tempdir().unwrap();
            let delivered = Arc::new(Mutex::new(0u64));
            let d = delivered.clone();
            let host = Host::start(
                StartParams {
                    root: dir.path().to_path_buf(),
                    name: name.into(),
                    app_version: "0.0.1".into(),
                    engine_label: "test@2026.11.0".into(),
                    commit: Config::default(),
                    continue_from: None,
                },
                EngineSource::Ready(Box::new(shared.clone())),
                Box::new(|| Box::new(ChunkVad(0))),
                Box::new(move || Box::new(RampMic(d))),
                Box::new(FixedClock),
                Arc::new(Collect::default()),
                None,
            )
            .unwrap();
            host.set_source(Source::System, false).unwrap();
            std::thread::sleep(Duration::from_millis(300));
            let before_end = *delivered.lock().unwrap();
            host.end().unwrap();
            assert_whole_stream_in_order(&std::mem::take(&mut *heard.lock().unwrap()), before_end);
            assert!(only_md(dir.path()).contains("**Me:** heard"));
        }
    }
}

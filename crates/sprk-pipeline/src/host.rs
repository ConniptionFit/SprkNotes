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
}

fn emit_captions(events: &dyn Events, session: &mut Session, now_ms: u64) {
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
}

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
        if let Some(w) = lock(&self.writer).as_mut() {
            w.append_statement(
                seg.start_ms,
                seg.end_ms,
                seg.speaker.label(),
                source,
                &seg.text,
            )?;
        }
        let id = self.id();
        self.events.line(UiLine::Statement {
            id,
            t: timestamp(seg.start_ms),
            speaker: seg.speaker.label().to_string(),
            text: seg.text.clone(),
        });
        Ok(())
    }

    fn event(&mut self, at_ms: u64, text: &str) -> Result<(), SessionError> {
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
    /// Create the file, then run the session on its own thread. The engine and VAD are already
    /// loaded (the caller resolved and validated the model); `make_backend` runs on the capture
    /// thread because platform streams must stay on the thread that opened them.
    pub fn start(
        p: StartParams,
        engine: Box<dyn Engine>,
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
        let path = unique_path(&p.root, &file_name(&stamp.date, &stamp.hhmm, &title));
        let sidecar = p.root.join(".sprknotes").join(format!(
            "{}.jsonl",
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("session")
        ));
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
        let writer = TranscriptWriter::create(path, Some(sidecar), meta, None)?;
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

#[allow(clippy::too_many_arguments)]
fn run(
    rx: Receiver<Cmd>,
    shared: SharedWriter,
    engine: Box<dyn Engine>,
    make_vad: VadFactory,
    make_backend: Box<dyn FnOnce() -> Box<dyn Backend> + Send>,
    clock: Box<dyn Clock>,
    events: Arc<dyn Events>,
    commit: Config,
    root: PathBuf,
    context: Option<ContextConfig>,
) {
    let mut ctx = context.map(Ctx::new);
    let start = Instant::now();
    let mut capture = match CaptureThread::spawn(make_backend, start) {
        Ok(c) => c,
        // No thread to read the devices with: close the file as interrupted, as for a panic.
        Err(_) => std::panic::resume_unwind(Box::new("capture thread did not start")),
    };
    let sink = UiSink {
        writer: shared.clone(),
        events: events.clone(),
        next_id: 0,
    };
    let mut session =
        Session::new(engine, make_vad, Box::new(sink), commit).with_echo_cancellation(true);
    let now_ms = |s: &Instant| s.elapsed().as_millis() as u64;
    let mut last_tick = 0u64;
    let mut title: Option<(String, NameSource)> = None;
    let mut last_device: [Option<String>; 2] = [None, None];
    events.status(UiStatus::Transcribing);

    let problem = |e: SessionError| {
        let _ = e;
        events.problem("write_failed");
    };

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
                emit_captions(events.as_ref(), &mut session, now_ms(&start));
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
        emit_captions(events.as_ref(), &mut session, t);
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
    emit_captions(events.as_ref(), &mut session, t);
    drop(session); // releases the sink's handle on the writer
    if let Some(mut w) = lock(&shared).take() {
        if let Some((name, src)) = title.take() {
            let stamp = clock.now();
            let new_path = unique_path(&root, &file_name(&stamp.date, &stamp.hhmm, &name));
            let _ = w.rename(new_path, &name, src);
        }
        if w.finalize(&clock.now().iso, &duration_text(t), Status::Complete)
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
    }
    impl Events for Collect {
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
            },
            Box::new(QueueEngine(q)),
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
            },
            Box::new(QueueEngine(Arc::new(Mutex::new(vec![])))),
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
            },
            Box::new(PanicEngine),
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
            },
            Box::new(QueueEngine(Arc::new(Mutex::new(vec![])))),
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
            },
            Box::new(SlowEngine),
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
}

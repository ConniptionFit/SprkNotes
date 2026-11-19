//! Session host: owns one live session on a dedicated thread (SPEC 3.2, 4.5, 6.3).
//! Capture, VAD, ASR, commit buffer and the crash-safe writer all run here; the UI side only
//! sends commands and receives events. Everything platform-specific is injected, so the whole
//! flow runs on the Linux dev host with fakes.
//!
//! Hard rules: audio never leaves memory; committed lines are appended and flushed at once
//! (the writer does that), so a crash or a dropped `Host` loses nothing already committed.
use crate::commit::Config;
use crate::session::{Session, SessionError, Sink, VadFactory};
use crate::Segment;
use sprk_asr::Engine;
use sprk_audio::live::{Backend, CaptureEvent, LiveCapture};
use sprk_audio::Source;
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
        let (tx, rx) = channel();
        let root = p.root.clone();
        let commit = p.commit.clone();
        let join = std::thread::Builder::new()
            .name("sprk-session".into())
            .spawn(move || {
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
                );
            })
            .map_err(|_| HostError::Gone)?;
        Ok(Host {
            tx,
            join: Some(join),
        })
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
) {
    let mut capture = LiveCapture::new(make_backend());
    let sink = UiSink {
        writer: shared.clone(),
        events: events.clone(),
        next_id: 0,
    };
    let mut session = Session::new(engine, make_vad, Box::new(sink), commit);
    let start = Instant::now();
    let now_ms = |s: &Instant| s.elapsed().as_millis() as u64;
    let mut last_tick = 0u64;
    let mut title: Option<(String, NameSource)> = None;
    events.status(UiStatus::Transcribing);

    let problem = |e: SessionError| {
        let _ = e;
        events.problem("write_failed");
    };

    'main: loop {
        // Commands first so Pause/End act on a consistent state.
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(Cmd::Pause) => {
                capture.poll(now_ms(&start));
                drain(&mut capture, &mut session);
                if let Err(e) = session.pause(now_ms(&start)) {
                    problem(e);
                }
                events.status(UiStatus::Paused);
            }
            Ok(Cmd::Resume) => {
                if let Err(e) = session.resume(now_ms(&start)) {
                    problem(e);
                }
                events.status(UiStatus::Transcribing);
            }
            Ok(Cmd::SetSource(s, on)) => {
                capture.set_enabled(s, on);
                session.set_source_enabled(s, on);
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
        capture.poll(t);
        for ev in capture.take_events() {
            let text = match ev {
                CaptureEvent::DeviceChanged(s) => format!("Source changed: {}", label(s)),
                CaptureEvent::Unavailable(s) => format!("Source unavailable: {}", label(s)),
                CaptureEvent::Restored(s) => format!("Source restored: {}", label(s)),
            };
            if let Err(e) = session.event(t, &text) {
                problem(e);
            }
        }
        drain(&mut capture, &mut session);
        if t.saturating_sub(last_tick) >= 1_000 {
            last_tick = t;
            if let Err(e) = session.tick(t) {
                problem(e);
            }
        }
    }

    // End (or the UI went away): flush, then finalize the file.
    let t = now_ms(&start);
    capture.poll(t);
    drain(&mut capture, &mut session);
    if let Err(e) = session.end(t) {
        problem(e);
    }
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

fn label(s: Source) -> &'static str {
    match s {
        Source::Mic => "microphone",
        Source::System => "system audio",
    }
}

fn drain(capture: &mut LiveCapture, session: &mut Session) {
    while let Some(f) = capture.next_frame() {
        session.feed(f.source, &f.samples);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprk_asr::{AsrError, Segment as AsrSeg, Utterance, Vad};
    use sprk_audio::live::PcmStream;
    use sprk_audio::AudioError;

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
        host.rename("Budget review").unwrap();
        std::thread::sleep(Duration::from_millis(200));
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
}

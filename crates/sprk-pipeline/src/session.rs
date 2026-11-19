//! Session orchestration (SPEC 3.2, 4.5): frames in, committed transcript lines out.
//! Deterministic and clock-free: the caller passes `now_ms` (elapsed since Start), so the
//! whole flow runs on the Linux host with fakes. The app wraps this in capture and ASR threads.
//!
//! Hard rules honored here: audio stays in memory (nothing is written but transcript text),
//! committed lines are never rewritten (rule 13), errors never carry transcript text.
use crate::caption::{Caption, Captioner};
use crate::commit::{CommitBuffer, Config};
use crate::{Segment, Speaker};
use sprk_asr::{transcribe_utterance, Engine, Vad};
use sprk_audio::Source;
use sprk_transcript::writer::TranscriptWriter;
use sprk_transcript::TranscriptError;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("transcript write failed: {0}")]
    Transcript(#[from] TranscriptError),
}

/// Where committed lines go. [`TranscriptWriter`] is the real one; tests use a vector.
pub trait Sink: Send {
    fn statement(&mut self, seg: &Segment) -> Result<(), SessionError>;
    fn event(&mut self, at_ms: u64, text: &str) -> Result<(), SessionError>;
}

impl Sink for TranscriptWriter {
    fn statement(&mut self, seg: &Segment) -> Result<(), SessionError> {
        let source = if seg.speaker.is_remote() {
            "system"
        } else {
            "mic"
        };
        self.append_statement(
            seg.start_ms,
            seg.end_ms,
            seg.speaker.label(),
            source,
            &seg.text,
        )?;
        Ok(())
    }
    fn event(&mut self, at_ms: u64, text: &str) -> Result<(), SessionError> {
        self.append_event(at_ms, text)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Running,
    Paused,
    Ended,
}

/// Counters only (CLAUDE.md rule 4: IDs and counts, never text).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub utterances: u64,
    pub decode_errors: u64,
    pub committed: u64,
}

pub type VadFactory = Box<dyn Fn() -> Box<dyn Vad> + Send>;

struct Stream {
    vad: Box<dyn Vad>,
    enabled: bool,
    /// Elapsed ms at which this run of the stream began (shifts after each resume).
    base_ms: u64,
}

pub struct Session {
    engine: Box<dyn Engine>,
    make_vad: VadFactory,
    mic: Stream,
    system: Stream,
    commit: CommitBuffer,
    sink: Box<dyn Sink>,
    remote: Speaker,
    state: State,
    stats: Stats,
    captions: Captioner,
}

impl Session {
    pub fn new(
        engine: Box<dyn Engine>,
        make_vad: VadFactory,
        sink: Box<dyn Sink>,
        cfg: Config,
    ) -> Self {
        let mic = Stream {
            vad: make_vad(),
            enabled: true,
            base_ms: 0,
        };
        let system = Stream {
            vad: make_vad(),
            enabled: true,
            base_ms: 0,
        };
        Session {
            engine,
            make_vad,
            mic,
            system,
            commit: CommitBuffer::new(cfg),
            sink,
            remote: Speaker::Others,
            state: State::Running,
            stats: Stats::default(),
            captions: Captioner::new(),
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Source toggles (SPEC 4.1). A disabled source's frames are ignored.
    pub fn set_source_enabled(&mut self, source: Source, on: bool) {
        match source {
            Source::Mic => self.mic.enabled = on,
            Source::System => self.system.enabled = on,
        }
    }

    /// Label for the remote stream from now on. Already-decoded segments keep their label and
    /// committed lines are never rewritten (CLAUDE.md rule 13); unsure means `Others`.
    pub fn set_remote_speaker(&mut self, s: Speaker) {
        self.remote = if s == Speaker::Me { Speaker::Others } else { s };
    }

    /// Feed 16 kHz mono samples that arrived for `source`. Ignored unless running.
    pub fn feed(&mut self, source: Source, samples: &[f32]) {
        if self.state != State::Running {
            return;
        }
        let (stream, speaker) = match source {
            Source::Mic => (&mut self.mic, Speaker::Me),
            Source::System => (&mut self.system, self.remote.clone()),
        };
        if !stream.enabled {
            return;
        }
        let utterances = stream.vad.accept(samples);
        let base = stream.base_ms;
        self.captions.push(source, samples, !utterances.is_empty());
        self.decode(utterances, base, &speaker);
    }

    fn decode(&mut self, utterances: Vec<sprk_asr::Utterance>, base_ms: u64, speaker: &Speaker) {
        for u in utterances {
            self.stats.utterances += 1;
            match transcribe_utterance(self.engine.as_mut(), &u) {
                Ok(segs) => {
                    for s in segs {
                        if s.text.trim().is_empty() {
                            continue;
                        }
                        self.commit.push(Segment {
                            speaker: speaker.clone(),
                            start_ms: base_ms + s.start_ms,
                            end_ms: base_ms + s.end_ms,
                            text: s.text,
                        });
                    }
                }
                Err(_) => self.stats.decode_errors += 1,
            }
        }
    }

    /// Live captions (ADR-006): what the UI wants right now. Captions are a transient view of
    /// speech in progress; they are never committed, written or logged.
    pub fn set_captions(&mut self, on: bool, me: bool) {
        self.captions.set_wanted(on, me);
    }

    /// In-progress captions that are due. Empty unless running and wanted.
    pub fn captions(&mut self, now_ms: u64) -> Vec<Caption> {
        if self.state != State::Running {
            return Vec::new();
        }
        let remote = self.remote.clone();
        self.captions.poll(now_ms, self.engine.as_mut(), &remote)
    }

    /// Commit whatever has aged past the delay. Call about once a second.
    pub fn tick(&mut self, now_ms: u64) -> Result<Vec<Segment>, SessionError> {
        let ready = self.commit.drain_ready(now_ms);
        self.write(&ready)?;
        Ok(ready)
    }

    fn write(&mut self, segs: &[Segment]) -> Result<(), SessionError> {
        for s in segs {
            self.sink.statement(s)?;
            self.stats.committed += 1;
        }
        Ok(())
    }

    /// Release speech still held by the VADs and commit everything (pause, end, panic).
    fn drain_all(&mut self, now_ms: u64) -> Result<Vec<Segment>, SessionError> {
        let u = self.mic.vad.flush();
        let b = self.mic.base_ms;
        self.decode(u, b, &Speaker::Me);
        let u = self.system.vad.flush();
        let b = self.system.base_ms;
        let remote = self.remote.clone();
        self.decode(u, b, &remote);
        let all = self.commit.flush(now_ms);
        self.write(&all)?;
        Ok(all)
    }

    pub fn pause(&mut self, now_ms: u64) -> Result<(), SessionError> {
        if self.state != State::Running {
            return Ok(());
        }
        self.captions.clear();
        self.drain_all(now_ms)?;
        self.sink.event(now_ms, "Transcription paused")?;
        self.state = State::Paused;
        Ok(())
    }

    /// Timestamps stay wall-clock elapsed, so the gap is visible (SPEC 4.5): fresh VADs
    /// start counting from the resume moment.
    pub fn resume(&mut self, now_ms: u64) -> Result<(), SessionError> {
        if self.state != State::Paused {
            return Ok(());
        }
        self.mic.vad = (self.make_vad)();
        self.system.vad = (self.make_vad)();
        self.mic.base_ms = now_ms;
        self.system.base_ms = now_ms;
        self.sink.event(now_ms, "Transcription resumed")?;
        self.state = State::Running;
        Ok(())
    }

    /// Flush and stop. The caller finalizes the file (SPEC 6.3).
    pub fn end(&mut self, now_ms: u64) -> Result<(), SessionError> {
        if self.state == State::Ended {
            return Ok(());
        }
        if self.state == State::Running {
            self.drain_all(now_ms)?;
        }
        self.state = State::Ended;
        Ok(())
    }

    /// Write an event line (source changed, call started, labeling mode changed, ...).
    /// Buffered statements are flushed first so the event lands in true order.
    pub fn event(&mut self, now_ms: u64, text: &str) -> Result<(), SessionError> {
        let ready = self.commit.flush(now_ms);
        self.write(&ready)?;
        self.sink.event(now_ms, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprk_asr::{AsrError, Segment as AsrSeg, Utterance};
    use std::sync::{Arc, Mutex};

    /// Every fed chunk is one utterance starting at the stream's running sample count.
    struct ChunkVad {
        seen: u64,
    }
    impl Vad for ChunkVad {
        fn accept(&mut self, s: &[f32]) -> Vec<Utterance> {
            let u = Utterance {
                start_sample: self.seen,
                samples: s.to_vec(),
            };
            self.seen += s.len() as u64;
            vec![u]
        }
        fn flush(&mut self) -> Vec<Utterance> {
            vec![]
        }
    }

    /// Answers from a queue; the segment spans the whole utterance.
    struct QueueEngine(Arc<Mutex<Vec<String>>>);
    impl Engine for QueueEngine {
        fn transcribe(&mut self, s: &[f32]) -> Result<Vec<AsrSeg>, AsrError> {
            let mut q = self.0.lock().unwrap();
            if q.is_empty() {
                return Err(AsrError::Rejected);
            }
            Ok(vec![AsrSeg {
                start_ms: 0,
                end_ms: (s.len() as u64) * 1000 / 16_000,
                text: q.remove(0),
            }])
        }
    }

    #[derive(Clone, Default)]
    struct Log(Arc<Mutex<Vec<String>>>);
    impl Sink for Log {
        fn statement(&mut self, s: &Segment) -> Result<(), SessionError> {
            self.0.lock().unwrap().push(format!(
                "{} {}: {}",
                s.start_ms,
                s.speaker.label(),
                s.text
            ));
            Ok(())
        }
        fn event(&mut self, at: u64, t: &str) -> Result<(), SessionError> {
            self.0.lock().unwrap().push(format!("{at} > {t}"));
            Ok(())
        }
    }

    fn make(answers: &[&str]) -> (Session, Log) {
        let q = Arc::new(Mutex::new(
            answers.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        ));
        let log = Log::default();
        let s = Session::new(
            Box::new(QueueEngine(q)),
            Box::new(|| Box::new(ChunkVad { seen: 0 })),
            Box::new(log.clone()),
            Config::default(),
        );
        (s, log)
    }

    static ONE_SEC: [f32; 16_000] = [0.1; 16_000];

    #[test]
    fn mic_and_system_get_me_and_others_in_time_order() {
        let (mut s, log) = make(&["hello team", "thanks for joining everyone"]);
        s.feed(Source::Mic, &ONE_SEC);
        s.feed(Source::System, &ONE_SEC);
        assert!(s.tick(5_000).unwrap().is_empty(), "held inside the delay");
        s.end(8_000).unwrap();
        let l = log.0.lock().unwrap().clone();
        assert_eq!(
            l,
            vec!["0 Me: hello team", "0 Others: thanks for joining everyone"]
        );
        assert_eq!(s.stats().committed, 2);
    }

    #[test]
    fn pause_flushes_writes_event_and_ignores_audio_until_resume() {
        let (mut s, log) = make(&["first statement here", "after the break we continue"]);
        s.feed(Source::Mic, &ONE_SEC);
        s.pause(2_000).unwrap();
        assert_eq!(s.state(), State::Paused);
        s.feed(Source::Mic, &ONE_SEC); // dropped
        s.resume(10_000).unwrap();
        s.feed(Source::Mic, &ONE_SEC);
        s.end(12_000).unwrap();
        let l = log.0.lock().unwrap().clone();
        assert_eq!(
            l,
            vec![
                "0 Me: first statement here",
                "2000 > Transcription paused",
                "10000 > Transcription resumed",
                "10000 Me: after the break we continue",
            ]
        );
    }

    #[test]
    fn disabled_source_is_ignored() {
        let (mut s, log) = make(&["only the mic speaks"]);
        s.set_source_enabled(Source::System, false);
        s.feed(Source::System, &ONE_SEC);
        s.feed(Source::Mic, &ONE_SEC);
        s.end(3_000).unwrap();
        assert_eq!(log.0.lock().unwrap().len(), 1);
    }

    #[test]
    fn remote_label_applies_to_new_audio_only_and_me_is_never_remote() {
        let (mut s, log) = make(&["from others before naming", "now it is a named one to one"]);
        s.feed(Source::System, &ONE_SEC);
        s.set_remote_speaker(Speaker::Named("Sam Sample".into()));
        s.feed(Source::System, &ONE_SEC);
        s.set_remote_speaker(Speaker::Me); // refused: falls back to Others
        s.end(5_000).unwrap();
        let l = log.0.lock().unwrap().clone();
        assert!(l[0].contains("Others: from others"));
        assert!(l[1].contains("Sam Sample: now it is"));
    }

    #[test]
    fn decode_errors_are_counted_not_fatal() {
        let (mut s, log) = make(&[]);
        s.feed(Source::Mic, &ONE_SEC);
        s.end(2_000).unwrap();
        assert_eq!(s.stats().decode_errors, 1);
        assert!(log.0.lock().unwrap().is_empty());
    }

    #[test]
    fn events_flush_buffered_lines_first() {
        let (mut s, log) = make(&["spoken before the change"]);
        s.feed(Source::Mic, &ONE_SEC);
        s.event(3_000, "Source changed: Headset").unwrap();
        let l = log.0.lock().unwrap().clone();
        assert_eq!(
            l,
            vec![
                "0 Me: spoken before the change",
                "3000 > Source changed: Headset"
            ]
        );
    }

    #[test]
    fn end_is_idempotent_and_blocks_further_audio() {
        let (mut s, log) = make(&["one line only"]);
        s.end(1_000).unwrap();
        s.end(2_000).unwrap();
        s.feed(Source::Mic, &ONE_SEC);
        assert_eq!(s.state(), State::Ended);
        assert!(log.0.lock().unwrap().is_empty());
    }

    #[test]
    fn writes_a_real_crash_safe_transcript() {
        use sprk_transcript::format::{Meta, NameSource, Status};
        let d = tempfile::tempdir().unwrap();
        let meta = Meta {
            title: "Sync".into(),
            date: "2026-11-19".into(),
            start: "09:00".into(),
            end: None,
            duration: None,
            status: Status::InProgress,
            sources: vec!["mic".into(), "system".into()],
            speakers: vec![],
            name_source: NameSource::Default,
            engine: "fake".into(),
            app: "0.0.1".into(),
        };
        let w = TranscriptWriter::create(d.path().join("t.md"), None, meta, None).unwrap();
        let path = w.path().to_path_buf();
        let q = Arc::new(Mutex::new(vec!["written straight to disk".to_string()]));
        let mut s = Session::new(
            Box::new(QueueEngine(q)),
            Box::new(|| Box::new(ChunkVad { seen: 0 })),
            Box::new(w),
            Config::default(),
        );
        s.feed(Source::Mic, &ONE_SEC);
        s.pause(1_000).unwrap(); // no clean shutdown needed after this
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.contains("**Me:** written straight to disk"));
        assert!(text.contains("Transcription paused"));
    }

    #[test]
    fn captions_are_transient_and_never_reach_the_sink() {
        let (mut s, log) = make(&["first line", "second line", "in progress words"]);
        s.set_captions(true, false);
        // The test VAD closes an utterance per feed, which uses up the first two answers.
        s.feed(Source::System, &ONE_SEC);
        s.feed(Source::System, &ONE_SEC);
        let caps = s.captions(1_000);
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].text, "in progress words");
        assert_eq!(caps[0].speaker, Speaker::Others);
        assert!(log.0.lock().unwrap().is_empty(), "nothing written yet");
        s.pause(2_000).unwrap();
        assert!(s.captions(10_000).is_empty(), "none while paused");
    }
}

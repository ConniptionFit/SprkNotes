//! sherpa-onnx adapters (SPEC 4.3, 17.2). Which engine runs is decided by `model.json`'s
//! `engine` string through the [`Registry`]; file names come from the manifest's `files`
//! roles. Nothing here hard-codes a model name or path (CLAUDE.md hard rule 9).
use crate::{AsrError, Engine, EngineOptions, Registry, Segment, Utterance, Vad};
use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, SileroVadModelConfig, VadModelConfig,
    VoiceActivityDetector,
};
use sprk_models::Manifest;
use std::path::Path;

pub const ENGINE_TRANSDUCER: &str = "sherpa-onnx/offline-transducer";
pub const ENGINE_WHISPER: &str = "sherpa-onnx/whisper";
pub const ENGINE_VAD_SILERO: &str = "sherpa-onnx/vad-silero";

/// Register every sherpa-onnx engine family compiled into this build.
pub fn register(reg: &mut Registry) {
    reg.register(ENGINE_TRANSDUCER, build_transducer);
    reg.register(ENGINE_WHISPER, build_whisper);
}

fn role(m: &Manifest, dir: &Path, role: &str) -> Result<String, AsrError> {
    m.file_path(dir, role)
        .map(|p| p.to_string_lossy().into_owned())
        .ok_or_else(|| AsrError::MissingRole(role.into()))
}

/// Threads for decoding: `SPRK_ASR_THREADS` (benchmarking on dev machines) wins, then the
/// count the speed check picked (`options`), then [`crate::tune::default_threads`].
fn threads(options: &EngineOptions) -> i32 {
    crate::tune::env_threads()
        .or(options.threads)
        .filter(|n| *n > 0)
        .unwrap_or_else(crate::tune::default_threads)
        .min(i32::MAX as u32) as i32
}

/// Boost per hint token (sherpa-onnx's usual value). Modest, so a hint nudges rather than forces.
const HOTWORDS_SCORE: f32 = 1.5;

pub struct SherpaEngine {
    recognizer: OfflineRecognizer,
    /// How to build the recognizer again with word hints. Only for transducer packs that ship
    /// a `bpe_vocab` file: sherpa-onnx needs it to turn a hint into the model's tokens, and
    /// without it the native code stops the process instead of returning an error.
    hint_cfg: Option<OfflineRecognizerConfig>,
    /// Hints for every stream, in sherpa-onnx's form (`one/two words`). Memory only, never a
    /// file and never logged.
    hotwords: Option<String>,
}

impl Engine for SherpaEngine {
    fn transcribe(&mut self, samples_16k_mono: &[f32]) -> Result<Vec<Segment>, AsrError> {
        let stream = match &self.hotwords {
            Some(h) => self.recognizer.create_stream_with_hotwords(h),
            None => self.recognizer.create_stream(),
        };
        stream.accept_waveform(16_000, samples_16k_mono);
        self.recognizer.decode(&stream);
        let text = stream
            .get_result()
            .map(|r| r.text.trim().to_owned())
            .unwrap_or_default();
        if text.is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![Segment {
            start_ms: 0,
            end_ms: samples_16k_mono.len() as u64 * 1000 / 16_000,
            text,
        }])
    }

    /// Rebuilds the recognizer with modified beam search and per-stream hotwords. Any failure
    /// keeps the greedy recognizer already loaded, so transcription goes on exactly as before.
    fn bias_toward(&mut self, phrases: &[String]) -> bool {
        let Some(base) = &self.hint_cfg else {
            return false;
        };
        let hotwords = crate::custom_words::hint_phrases(phrases);
        if hotwords.is_empty() {
            return false;
        }
        let mut cfg = base.clone();
        cfg.decoding_method = Some("modified_beam_search".into());
        cfg.max_active_paths = 4;
        cfg.hotwords_score = HOTWORDS_SCORE;
        match OfflineRecognizer::create(&cfg) {
            Some(r) => {
                self.recognizer = r;
                self.hotwords = Some(hotwords);
                true
            }
            None => false,
        }
    }
}

fn build_transducer(
    m: &Manifest,
    dir: &Path,
    options: &EngineOptions,
) -> Result<Box<dyn Engine>, AsrError> {
    let mut cfg = OfflineRecognizerConfig::default();
    cfg.model_config.transducer.encoder = Some(role(m, dir, "encoder")?);
    cfg.model_config.transducer.decoder = Some(role(m, dir, "decoder")?);
    cfg.model_config.transducer.joiner = Some(role(m, dir, "joiner")?);
    cfg.model_config.tokens = Some(role(m, dir, "tokens")?);
    cfg.model_config.model_type = Some("nemo_transducer".into());
    cfg.model_config.num_threads = threads(options);
    // Optional role: a pack that ships the tokenizer's vocabulary can take custom-word hints.
    let hint_cfg = m.file_path(dir, "bpe_vocab").map(|p| {
        let mut h = cfg.clone();
        h.model_config.modeling_unit = Some("bpe".into());
        h.model_config.bpe_vocab = Some(p.to_string_lossy().into_owned());
        h
    });
    create(cfg, hint_cfg)
}

fn build_whisper(
    m: &Manifest,
    dir: &Path,
    options: &EngineOptions,
) -> Result<Box<dyn Engine>, AsrError> {
    let mut cfg = OfflineRecognizerConfig::default();
    cfg.model_config.whisper.encoder = Some(role(m, dir, "encoder")?);
    cfg.model_config.whisper.decoder = Some(role(m, dir, "decoder")?);
    cfg.model_config.whisper.language = m.languages.first().cloned();
    cfg.model_config.whisper.task = Some("transcribe".into());
    cfg.model_config.whisper.tail_paddings = -1;
    cfg.model_config.tokens = Some(role(m, dir, "tokens")?);
    cfg.model_config.num_threads = threads(options);
    create(cfg, None)
}

fn create(
    cfg: OfflineRecognizerConfig,
    hint_cfg: Option<OfflineRecognizerConfig>,
) -> Result<Box<dyn Engine>, AsrError> {
    let recognizer = OfflineRecognizer::create(&cfg).ok_or(AsrError::Rejected)?;
    Ok(Box::new(SherpaEngine {
        recognizer,
        hint_cfg,
        hotwords: None,
    }))
}

/// Silero VAD through sherpa-onnx. `max_speech_s` caps utterance length so offline
/// models with a hard input limit (Whisper: 30 s) never see more than they can take.
pub struct SileroVad {
    vad: VoiceActivityDetector,
    /// Absolute index (in the current stream) of `history[0]`.
    hist_base: u64,
    /// Recent audio, memory only, so each utterance can start a little before the point
    /// where the detector noticed speech (its onset lags the real start by a few words' worth
    /// of milliseconds). Capped well above the longest utterance plus the VAD buffer.
    history: std::collections::VecDeque<f32>,
    /// Added to the detector's own sample positions so they stay absolute across forced cuts.
    offset: u64,
    /// Absolute sample index at which the current unbroken run of speech began.
    speech_since: Option<u64>,
    /// Longest unbroken run (samples) before the speech is cut even without a pause.
    force_cut: u64,
    /// Absolute sample index where the last released utterance ended.
    prev_end: u64,
}

/// Samples kept ahead of a detected utterance (1 s at 16 kHz). The detector reports speech
/// about 0.65 s after it begins, so less than this clipped a sentence's first words. Never
/// reaches back into the previous utterance, so no word is decoded twice.
const PRE_ROLL: u64 = 16_000;
/// Samples kept after a detected utterance (0.3 s). The detector ends speech where its
/// probability first drops, often inside a word's quiet tail (a final s, t or k), and only
/// releases it after 0.5 s of silence, so this much is always heard already and is silence by
/// the detector's own measure: it never reaches into the next utterance.
const POST_ROLL: u64 = 4_800;
/// History cap: longer than the VAD's 60 s internal buffer.
const HISTORY_MAX: usize = 70 * 16_000;
/// The detector only honors its maximum speech length at a pause. Speech with no pause for this
/// many seconds past that maximum is cut anyway, so non-stop talking still commits steadily
/// instead of waiting for the detector's 60 s buffer (or for End).
const FORCE_CUT_EXTRA_S: f32 = 5.0;

impl SileroVad {
    pub fn load(m: &Manifest, dir: &Path, max_speech_s: f32) -> Result<Self, AsrError> {
        m.check_compat(&[ENGINE_VAD_SILERO], env!("CARGO_PKG_VERSION"))?;
        m.verify_files(dir)?;
        let cfg = VadModelConfig {
            silero_vad: SileroVadModelConfig {
                model: Some(role(m, dir, "vad")?),
                threshold: 0.5,
                min_silence_duration: 0.5,
                min_speech_duration: 0.25,
                window_size: 512,
                max_speech_duration: max_speech_s,
            },
            sample_rate: 16_000,
            num_threads: 1,
            ..VadModelConfig::default()
        };
        let vad = VoiceActivityDetector::create(&cfg, 60.0).ok_or(AsrError::Rejected)?;
        Ok(Self {
            vad,
            hist_base: 0,
            history: std::collections::VecDeque::new(),
            offset: 0,
            speech_since: None,
            force_cut: ((max_speech_s + FORCE_CUT_EXTRA_S) * 16_000.0) as u64,
            prev_end: 0,
        })
    }

    fn drain(&mut self) -> Vec<Utterance> {
        let mut out = Vec::new();
        while let Some(seg) = self.vad.front() {
            let start = self.offset + seg.start().max(0) as u64;
            let len = seg.samples().len() as u64;
            let heard = self.hist_base + self.history.len() as u64;
            let floor = self.hist_base.max(self.prev_end.min(start));
            let (from, to) = crate::padded_span(start, len, floor, heard, PRE_ROLL, POST_ROLL);
            let span = |a: u64, b: u64| -> Vec<f32> {
                let skip = a.saturating_sub(self.hist_base) as usize;
                self.history
                    .iter()
                    .skip(skip)
                    .take(b.saturating_sub(a) as usize)
                    .copied()
                    .collect()
            };
            let mut samples = span(from, start);
            samples.extend_from_slice(seg.samples());
            samples.extend(span(start + len, to));
            out.push(Utterance {
                start_sample: from,
                samples,
            });
            self.prev_end = to;
            self.vad.pop();
        }
        out
    }
}

impl Vad for SileroVad {
    fn accept(&mut self, samples_16k_mono: &[f32]) -> Vec<Utterance> {
        self.history.extend(samples_16k_mono.iter().copied());
        while self.history.len() > HISTORY_MAX {
            self.history.pop_front();
            self.hist_base += 1;
        }
        self.vad.accept_waveform(samples_16k_mono);
        let mut out = self.drain();
        let now = self.hist_base + self.history.len() as u64;
        if self.vad.detected() {
            let since = *self.speech_since.get_or_insert(now);
            if now.saturating_sub(since) > self.force_cut {
                // Release the unbroken speech, then start the detector clean at this point.
                self.vad.flush();
                out.extend(self.drain());
                self.vad.reset();
                self.offset = now;
                self.speech_since = None;
            }
        } else {
            self.speech_since = None;
        }
        out
    }
    fn pending_since(&self) -> Option<u64> {
        self.speech_since
    }
    fn flush(&mut self) -> Vec<Utterance> {
        self.vad.flush();
        let out = self.drain();
        // The next stream starts clean; without this the detector's internal buffer offsets
        // go out of range and sherpa aborts.
        self.vad.reset();
        self.history.clear();
        self.hist_base = 0;
        self.offset = 0;
        self.prev_end = 0;
        self.speech_since = None;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_both_engine_families() {
        let mut r = Registry::default();
        register(&mut r);
        let e = r.engines();
        assert!(e.contains(&ENGINE_TRANSDUCER) && e.contains(&ENGINE_WHISPER));
    }
}

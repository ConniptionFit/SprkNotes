//! Engine trait. The pipeline never branches on a specific engine or model.
pub mod wav;
pub mod wer;

#[cfg(any(feature = "sherpa-static", feature = "sherpa-shared"))]
pub mod sherpa;
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AsrError {
    #[error("engine failed to load")]
    Load,
    #[error("model rejected: {0}")]
    Model(#[from] sprk_models::ManifestError),
    #[error("model.json does not declare the '{0}' file this engine needs")]
    MissingRole(String),
    #[error("engine rejected the model files")]
    Rejected,
}

pub trait Engine: Send {
    fn transcribe(&mut self, samples_16k_mono: &[f32]) -> Result<Vec<Segment>, AsrError>;
}

/// One stretch of speech cut out of a stream by a [`Vad`].
#[derive(Debug, Clone, PartialEq)]
pub struct Utterance {
    /// Index of the first sample, relative to the start of the stream.
    pub start_sample: u64,
    pub samples: Vec<f32>,
}

/// Voice activity detection (SPEC 17.2). Feed 16 kHz mono audio; get finished utterances.
pub trait Vad: Send {
    fn accept(&mut self, samples_16k_mono: &[f32]) -> Vec<Utterance>;
    /// End of stream: release any speech still buffered.
    fn flush(&mut self) -> Vec<Utterance>;
}

const SAMPLE_RATE: u64 = 16_000;

fn to_ms(samples: u64) -> u64 {
    samples * 1000 / SAMPLE_RATE
}

/// Transcribe one utterance and place its segments on the stream timeline.
pub fn transcribe_utterance(
    engine: &mut dyn Engine,
    u: &Utterance,
) -> Result<Vec<Segment>, AsrError> {
    let base = to_ms(u.start_sample);
    let len = to_ms(u.samples.len() as u64);
    let mut out = engine.transcribe(&u.samples)?;
    for s in &mut out {
        // Engines report times relative to the utterance; clamp so a segment never leaves it.
        s.start_ms = base + s.start_ms.min(len);
        s.end_ms = base + s.end_ms.min(len);
    }
    Ok(out)
}

/// Run a whole 16 kHz buffer through a VAD and decode each utterance in order.
/// Chunks stay small so the VAD sees the audio the way a live stream would deliver it.
pub fn transcribe_with_vad(
    engine: &mut dyn Engine,
    vad: &mut dyn Vad,
    samples_16k_mono: &[f32],
) -> Result<Vec<Segment>, AsrError> {
    let mut out = Vec::new();
    let mut utterances = Vec::new();
    for chunk in samples_16k_mono.chunks(512) {
        utterances.extend(vad.accept(chunk));
    }
    utterances.extend(vad.flush());
    for u in &utterances {
        out.extend(transcribe_utterance(engine, u)?);
    }
    Ok(out)
}

/// Builds an engine from a verified model folder. Adapters register one of these per
/// `model.json` `engine` string (SPEC 17.2); the pipeline never branches on the string.
pub type EngineFactory =
    fn(&sprk_models::Manifest, &std::path::Path) -> Result<Box<dyn Engine>, AsrError>;

#[derive(Default)]
pub struct Registry {
    entries: Vec<(&'static str, EngineFactory)>,
}

impl Registry {
    pub fn register(&mut self, engine: &'static str, factory: EngineFactory) {
        self.entries.push((engine, factory));
    }

    pub fn engines(&self) -> Vec<&'static str> {
        self.entries.iter().map(|(n, _)| *n).collect()
    }

    /// Validates compatibility and file hashes, then loads. A bad model is rejected
    /// with a reason and never half-loaded (SPEC 17.3).
    pub fn load(
        &self,
        manifest: &sprk_models::Manifest,
        dir: &std::path::Path,
        app_version: &str,
    ) -> Result<Box<dyn Engine>, AsrError> {
        manifest.check_compat(&self.engines(), app_version)?;
        manifest.verify_files(dir)?;
        let factory = self
            .entries
            .iter()
            .find(|(n, _)| *n == manifest.engine)
            .map(|(_, f)| *f)
            .ok_or(AsrError::Load)?;
        factory(manifest, dir)
    }
}

/// Scripted engine for portable tests.
pub struct FakeEngine {
    pub script: Vec<Segment>,
}
impl Engine for FakeEngine {
    fn transcribe(&mut self, _s: &[f32]) -> Result<Vec<Segment>, AsrError> {
        Ok(self.script.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(engine: &str) -> sprk_models::Manifest {
        let json = format!(
            r#"{{"schema":1,"id":"t","version":"2027.01.0","display_name":"T","engine":"{engine}",
"languages":["en"],"sample_rate":16000,
"features":{{"punctuation":true,"casing":true,"streaming":false}},
"accelerators":["cpu"],"min_app_version":"0.0.1","files":{{"tokens":"tokens.txt"}},
"sha256":{{"tokens.txt":"2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"}},
"license":"MIT","source":"synthetic"}}"#
        );
        sprk_models::Manifest::from_json(&json).unwrap()
    }

    fn fake_factory(
        _m: &sprk_models::Manifest,
        _d: &std::path::Path,
    ) -> Result<Box<dyn Engine>, AsrError> {
        Ok(Box::new(FakeEngine { script: vec![] }))
    }

    struct EveryChunkVad {
        seen: u64,
    }
    impl Vad for EveryChunkVad {
        fn accept(&mut self, s: &[f32]) -> Vec<Utterance> {
            let u = Utterance {
                start_sample: self.seen,
                samples: s.to_vec(),
            };
            self.seen += s.len() as u64;
            // Pretend only the second chunk contains speech.
            if u.start_sample == 512 {
                vec![u]
            } else {
                vec![]
            }
        }
        fn flush(&mut self) -> Vec<Utterance> {
            vec![]
        }
    }

    #[test]
    fn vad_pipeline_places_segments_on_the_stream_timeline() {
        let mut engine = FakeEngine {
            script: vec![Segment {
                start_ms: 0,
                end_ms: 999_999, // lies: must be clamped to the utterance
                text: "hi".into(),
            }],
        };
        let mut vad = EveryChunkVad { seen: 0 };
        let audio = vec![0.0f32; 1536];
        let segs = transcribe_with_vad(&mut engine, &mut vad, &audio).unwrap();
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].start_ms, 32); // sample 512 at 16 kHz
        assert_eq!(segs[0].end_ms, 32 + 32); // clamped to the 32 ms utterance
    }

    #[test]
    fn registry_loads_verified_model_and_rejects_others() {
        let mut r = Registry::default();
        r.register("test/fake", fake_factory);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tokens.txt"), "hello").unwrap();
        assert!(r.load(&manifest("test/fake"), dir.path(), "0.0.1").is_ok());
        assert!(r
            .load(&manifest("unknown/engine"), dir.path(), "0.0.1")
            .is_err());
        std::fs::write(dir.path().join("tokens.txt"), "tampered").unwrap();
        assert!(r.load(&manifest("test/fake"), dir.path(), "0.0.1").is_err());
    }

    #[test]
    fn fake_returns_script() {
        let seg = Segment {
            start_ms: 0,
            end_ms: 500,
            text: "hello".into(),
        };
        let mut e = FakeEngine {
            script: vec![seg.clone()],
        };
        assert_eq!(e.transcribe(&[]).ok(), Some(vec![seg]));
    }
}

//! Engine trait. The pipeline never branches on a specific engine or model.
pub mod wav;
pub mod wer;
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
}

pub trait Engine: Send {
    fn transcribe(&mut self, samples_16k_mono: &[f32]) -> Result<Vec<Segment>, AsrError>;
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

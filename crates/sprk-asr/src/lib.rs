//! Engine trait. The pipeline never branches on a specific engine or model.
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
}

pub trait Engine: Send {
    fn transcribe(&mut self, samples_16k_mono: &[f32]) -> Result<Vec<Segment>, AsrError>;
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

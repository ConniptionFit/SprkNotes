//! Audio capture seam. Samples live in memory only; nothing here writes audio to disk.
use std::sync::{Arc, Mutex};

pub mod aec;
#[cfg(feature = "cpal-backend")]
pub mod cpal_backend;
pub mod echo;
#[cfg(windows)]
pub mod endpoints;
pub mod live;
pub mod resample;

pub use resample::Resampler;

/// Which stream a frame came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Mic,
    System,
}

/// A block of 16 kHz mono f32 samples.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub source: Source,
    pub samples: Vec<f32>,
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("capture device unavailable")]
    Unavailable,
}

/// Windows: WASAPI (M2). Tests: [`FakeCapture`].
pub trait AudioCapture: Send {
    fn next_frame(&mut self) -> Result<Option<Frame>, AudioError>;
}

/// Windows: IAcousticEchoCancellationControl (M4), WebRTC APM fallback.
pub trait EchoCanceller: Send {
    fn process(&mut self, mic: &[f32], reference: &[f32]) -> Vec<f32>;
}

/// In-memory fake that replays scripted frames.
pub struct FakeCapture {
    frames: std::collections::VecDeque<Frame>,
}
impl FakeCapture {
    pub fn new(frames: Vec<Frame>) -> Self {
        Self {
            frames: frames.into(),
        }
    }
}
impl AudioCapture for FakeCapture {
    fn next_frame(&mut self) -> Result<Option<Frame>, AudioError> {
        Ok(self.frames.pop_front())
    }
}

/// Pass-through canceller used by tests.
pub struct NoopEchoCanceller;
impl EchoCanceller for NoopEchoCanceller {
    fn process(&mut self, mic: &[f32], _reference: &[f32]) -> Vec<f32> {
        mic.to_vec()
    }
}

#[allow(dead_code)]
type Shared<T> = Arc<Mutex<T>>;

#[cfg(windows)]
pub mod windows_impl {} // WASAPI wrappers arrive in M2.

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fake_replays_then_ends() {
        let f = Frame {
            source: Source::Mic,
            samples: vec![0.1],
        };
        let mut c = FakeCapture::new(vec![f.clone()]);
        assert_eq!(c.next_frame().ok().flatten(), Some(f));
        assert_eq!(c.next_frame().ok().flatten(), None);
    }
    #[test]
    fn noop_aec_passes_mic_through() {
        assert_eq!(
            NoopEchoCanceller.process(&[0.5, 0.25], &[1.0]),
            vec![0.5, 0.25]
        );
    }
}

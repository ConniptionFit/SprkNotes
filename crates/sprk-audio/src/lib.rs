//! Audio capture seam. Samples live in memory only; nothing here writes audio to disk.
use std::sync::{Arc, Mutex};

pub mod aec;
#[cfg(feature = "cpal-backend")]
pub mod cpal_backend;
pub mod echo;
#[cfg(windows)]
pub mod endpoints;
pub mod live;

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

/// Streaming linear resampler to a target rate (mono f32). Keeps one sample of history so
/// chunk boundaries do not click. Linear interpolation is adequate for speech into an ASR
/// front end; the capture path (M2) may swap in a better filter behind the same shape.
pub struct LinearResampler {
    step: f64,
    pos: f64,
    buf: Vec<f32>,
}

impl LinearResampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        let step = if out_rate == 0 {
            1.0
        } else {
            f64::from(in_rate) / f64::from(out_rate)
        };
        Self {
            step,
            pos: 0.0,
            buf: Vec::new(),
        }
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        self.buf.extend_from_slice(input);
        let mut out = Vec::with_capacity((input.len() as f64 / self.step) as usize + 1);
        while self.pos + 1.0 < self.buf.len() as f64 {
            let i = self.pos as usize;
            let frac = (self.pos - i as f64) as f32;
            let (a, b) = (self.buf[i], self.buf[i + 1]);
            out.push(a + (b - a) * frac);
            self.pos += self.step;
        }
        let used = (self.pos as usize).min(self.buf.len());
        self.buf.drain(..used);
        self.pos -= used as f64;
        out
    }
}

#[cfg(test)]
mod resample_tests {
    use super::*;

    #[test]
    fn chunked_matches_one_shot_length() {
        let input: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.01).sin()).collect();
        let one = LinearResampler::new(48_000, 16_000).process(&input);
        let mut r = LinearResampler::new(48_000, 16_000);
        let chunked: Vec<f32> = input.chunks(441).flat_map(|c| r.process(c)).collect();
        assert!(one.len().abs_diff(chunked.len()) <= 1);
        assert!((1598..=1600).contains(&one.len()));
        // Values agree where both exist: no discontinuity at chunk joins.
        for (a, b) in one.iter().zip(&chunked) {
            assert!((a - b).abs() < 1e-5);
        }
    }

    #[test]
    fn same_rate_is_identity() {
        let input = [0.1f32, 0.2, 0.3, 0.4];
        let out = LinearResampler::new(16_000, 16_000).process(&input);
        assert_eq!(out, input[..3].to_vec()); // last sample waits for its neighbour
    }
}

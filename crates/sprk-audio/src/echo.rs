//! Live echo removal for the microphone (SPEC 4.2 layer 2): WebRTC AEC3 (pure Rust port, the same
//! canceller family Chrome, Teams and Zoom use) with the loopback stream as the far-end reference.
//! Only the echo canceller runs. Noise suppression and gain control stay off: they change speech
//! the transcription model should hear untouched. Samples live in memory only.
use sonora::config::EchoCanceller;
use sonora::{AudioProcessing, Config, StreamConfig};
use std::collections::VecDeque;

/// AEC3 works on 10 ms frames; at 16 kHz that is 160 samples.
const FRAME: usize = 160;
/// Mic audio waits at most this long for its reference before it is processed without one.
const MAX_WAIT: usize = 16_000 * 3 / 10;

pub struct EchoPath {
    apm: AudioProcessing,
    mic: VecDeque<f32>,
    reference: VecDeque<f32>,
    out: Vec<f32>,
}

impl Default for EchoPath {
    fn default() -> Self {
        Self::new()
    }
}

impl EchoPath {
    pub fn new() -> Self {
        let cfg = Config {
            echo_canceller: Some(EchoCanceller::default()),
            ..Default::default()
        };
        let apm = AudioProcessing::builder()
            .config(cfg)
            .capture_config(StreamConfig::new(16_000, 1))
            .render_config(StreamConfig::new(16_000, 1))
            .build();
        Self {
            apm,
            mic: VecDeque::new(),
            reference: VecDeque::new(),
            out: Vec::new(),
        }
    }

    /// 16 kHz mono samples that went to the speakers (the loopback stream).
    pub fn push_reference(&mut self, samples: &[f32]) {
        self.reference.extend(samples);
        self.pump();
    }

    /// 16 kHz mono microphone samples.
    pub fn push_mic(&mut self, samples: &[f32]) {
        self.mic.extend(samples);
        self.pump();
    }

    /// Cleaned microphone samples ready so far, oldest first.
    pub fn take(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.out)
    }

    /// Everything still waiting, unprocessed (pause or end). A partial last frame is kept as is.
    pub fn flush(&mut self) -> Vec<f32> {
        self.pump_with(true);
        self.out.extend(self.mic.drain(..));
        self.reference.clear();
        self.take()
    }

    fn pump(&mut self) {
        self.pump_with(false);
    }

    fn pump_with(&mut self, force: bool) {
        let silence = [0.0f32; FRAME];
        while self.mic.len() >= FRAME {
            let have_ref = self.reference.len() >= FRAME;
            if !have_ref && !force && self.mic.len() < MAX_WAIT {
                break;
            }
            let mut r = [0.0f32; FRAME];
            if have_ref {
                for (d, s) in r.iter_mut().zip(self.reference.drain(..FRAME)) {
                    *d = s;
                }
            } else {
                r = silence;
            }
            let mut m = [0.0f32; FRAME];
            for (d, s) in m.iter_mut().zip(self.mic.drain(..FRAME)) {
                *d = s;
            }
            let mut render_out = [0.0f32; FRAME];
            let mut clean = [0.0f32; FRAME];
            let _ = self
                .apm
                .process_render_f32(&[&r[..]], &mut [&mut render_out[..]]);
            if self
                .apm
                .process_capture_f32(&[&m[..]], &mut [&mut clean[..]])
                .is_err()
            {
                clean = m; // never lose the user's speech to a processing error
            }
            self.out.extend_from_slice(&clean);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic speech-like signal: band-limited noise with a ~4 Hz syllable envelope.
    fn speechlike(n: usize, seed: u32) -> Vec<f32> {
        let mut x = seed;
        let mut lp = 0.0f32;
        (0..n)
            .map(|i| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let white = (x >> 8) as f32 / 8_388_608.0 - 1.0;
                lp = 0.6 * lp + 0.4 * white;
                let t = i as f32 / 16_000.0;
                let env = (0.5 + 0.5 * (2.0 * std::f32::consts::PI * 4.0 * t).sin()).powi(2);
                lp * env * 0.6
            })
            .collect()
    }

    /// What a laptop mic hears from its own speakers: delayed, filtered, a little distorted.
    fn through_the_room(refr: &[f32], delay: usize, gain: f32) -> Vec<f32> {
        (0..refr.len())
            .map(|i| {
                let g = |k: usize| if i >= k { refr[i - k] } else { 0.0 };
                let lin = gain * (g(delay) + 0.4 * g(delay + 23) + 0.2 * g(delay + 61));
                (lin * 2.0).tanh() / 2.0
            })
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn run(mic: &[f32], refr: &[f32]) -> Vec<f32> {
        let mut e = EchoPath::new();
        let mut out = Vec::new();
        for (m, r) in mic.chunks(320).zip(refr.chunks(320)) {
            e.push_mic(m);
            e.push_reference(r);
            out.extend(e.take());
        }
        out.extend(e.flush());
        out
    }

    #[test]
    fn speaker_sound_in_the_mic_is_removed() {
        let n = 16_000 * 12;
        let refr = speechlike(n, 7);
        let mic = through_the_room(&refr, 1_600, 0.5); // 100 ms path
        let out = run(&mic, &refr);
        let tail = n - 16_000 * 4..n;
        let db = 20.0 * (rms(&mic[tail.clone()]) / rms(&out[tail]).max(1e-9)).log10();
        eprintln!("echo reduction {db:.1} dB");
        assert!(db > 15.0, "echo reduced by only {db:.1} dB");
    }

    #[test]
    fn near_speech_with_a_silent_reference_is_untouched() {
        let n = 16_000 * 4;
        let mic = speechlike(n, 99);
        let out = run(&mic, &vec![0.0; n]);
        let ratio = rms(&out[8_000..]) / rms(&mic[8_000..]);
        assert!(ratio > 0.8, "kept only {ratio:.2} of the user's speech");
    }

    #[test]
    fn user_talking_over_playback_is_kept() {
        let n = 16_000 * 12;
        let refr = speechlike(n, 7);
        let echo = through_the_room(&refr, 1_600, 0.3);
        let me = speechlike(n, 4242);
        // The user speaks only in the last third.
        let start = n * 2 / 3;
        let mic: Vec<f32> = (0..n)
            .map(|i| echo[i] + if i >= start { me[i] } else { 0.0 })
            .collect();
        let out = run(&mic, &refr);
        let kept = rms(&out[start + 8_000..]) / rms(&me[start + 8_000..]);
        eprintln!("kept {kept:.2}");
        assert!(kept > 0.5, "user speech kept at {kept:.2}");
    }

    #[test]
    fn missing_reference_does_not_stall_the_mic() {
        let mut e = EchoPath::new();
        let mic = speechlike(16_000, 1);
        e.push_mic(&mic);
        // No reference ever arrives: audio is released once the wait limit passes.
        let first = e.take().len();
        assert!(first >= 16_000 - MAX_WAIT - FRAME, "only {first} released");
        assert_eq!(first + e.flush().len(), 16_000);
    }
}

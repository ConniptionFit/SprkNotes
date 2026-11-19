//! Software echo cancellation fallback (SPEC 4.2 layer 2) and the layer-1 decision logic.
//! Pure Rust, portable, no allocation after construction. See ADR-004 for why this is an
//! NLMS adaptive filter first and WebRTC APM can come later as another [`EchoCanceller`].
use crate::EchoCanceller;

/// Which layers are needed for the current devices (SPEC 4.2 headset detection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EchoPlan {
    /// Headphones or a headset: speaker sound does not reach the mic. Transcript dedupe stays on.
    DedupeOnly,
    /// The endpoint supports Windows AEC control: use it, plus dedupe.
    WindowsAec,
    /// Otherwise run the software canceller on the mic against the loopback reference.
    Software,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormFactor {
    Speakers,
    Headphones,
    Headset,
    Other,
}

pub fn plan(render: FormFactor, windows_aec_available: bool) -> EchoPlan {
    match render {
        FormFactor::Headphones | FormFactor::Headset => EchoPlan::DedupeOnly,
        _ if windows_aec_available => EchoPlan::WindowsAec,
        _ => EchoPlan::Software,
    }
}

/// Normalized LMS canceller. `taps` at 16 kHz covers `taps / 16` ms of echo path.
pub struct NlmsCanceller {
    w: Vec<f32>,
    /// Newest reference sample first.
    hist: Vec<f32>,
    mu: f32,
    energy: f32,
}

impl NlmsCanceller {
    pub fn new(taps: usize) -> Self {
        let taps = taps.max(1);
        Self {
            w: vec![0.0; taps],
            hist: vec![0.0; taps],
            mu: 0.5,
            energy: 0.0,
        }
    }

    pub fn with_step(mut self, mu: f32) -> Self {
        self.mu = mu.clamp(0.01, 1.0);
        self
    }
}

impl EchoCanceller for NlmsCanceller {
    /// `mic` and `reference` are time-aligned 16 kHz mono. If `reference` is shorter it is
    /// treated as silence for the missing tail.
    fn process(&mut self, mic: &[f32], reference: &[f32]) -> Vec<f32> {
        let n = self.w.len();
        let mut out = Vec::with_capacity(mic.len());
        for (i, &m) in mic.iter().enumerate() {
            let r = reference.get(i).copied().unwrap_or(0.0);
            // Shift history (newest first) and keep a running energy.
            let dropped = self.hist[n - 1];
            self.hist.copy_within(0..n - 1, 1);
            self.hist[0] = r;
            self.energy += r * r - dropped * dropped;
            self.energy = self.energy.max(0.0);
            let est: f32 = self.w.iter().zip(&self.hist).map(|(a, b)| a * b).sum();
            let e = m - est;
            // Geigel-style double-talk guard: freeze when the mic is much louder than the
            // reference history could explain, so near speech does not corrupt the filter.
            let peak_ref = self.hist.iter().fold(0.0f32, |a, b| a.max(b.abs()));
            let double_talk = m.abs() > 2.0 * peak_ref && peak_ref > 1e-4;
            if !double_talk && self.energy > 1e-6 {
                let g = self.mu * e / (self.energy + 1e-3);
                for (w, h) in self.w.iter_mut().zip(&self.hist) {
                    *w += g * h;
                }
            }
            out.push(e);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic noise in [-1, 1].
    fn noise(n: usize, seed: u32) -> Vec<f32> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                (x >> 8) as f32 / (1u32 << 23) as f32 - 1.0
            })
            .collect()
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt()
    }

    fn echo_of(reference: &[f32], delay: usize, gain: f32) -> Vec<f32> {
        (0..reference.len())
            .map(|i| {
                if i >= delay {
                    gain * reference[i - delay]
                } else {
                    0.0
                }
            })
            .collect()
    }

    #[test]
    fn plan_follows_form_factor_and_capability() {
        assert_eq!(plan(FormFactor::Headset, true), EchoPlan::DedupeOnly);
        assert_eq!(plan(FormFactor::Headphones, false), EchoPlan::DedupeOnly);
        assert_eq!(plan(FormFactor::Speakers, true), EchoPlan::WindowsAec);
        assert_eq!(plan(FormFactor::Speakers, false), EchoPlan::Software);
        assert_eq!(plan(FormFactor::Other, false), EchoPlan::Software);
    }

    #[test]
    fn removes_a_delayed_echo() {
        let reference = noise(32_000, 7);
        let mic = echo_of(&reference, 40, 0.6);
        let mut aec = NlmsCanceller::new(256);
        let out = aec.process(&mic, &reference);
        let before = rms(&mic[24_000..]);
        let after = rms(&out[24_000..]);
        // At least 20 dB of echo removed once converged.
        assert!(after < before / 10.0, "before {before} after {after}");
    }

    #[test]
    fn keeps_near_speech_while_removing_echo() {
        let reference = noise(48_000, 3);
        let echo = echo_of(&reference, 25, 0.5);
        // Near speech: a quiet tone that starts after convergence.
        let near: Vec<f32> = (0..48_000)
            .map(|i| {
                if i >= 24_000 {
                    0.2 * (i as f32 * 0.07).sin()
                } else {
                    0.0
                }
            })
            .collect();
        let mic: Vec<f32> = echo.iter().zip(&near).map(|(a, b)| a + b).collect();
        let mut aec = NlmsCanceller::new(256);
        let out = aec.process(&mic, &reference);
        let kept = rms(&out[40_000..]);
        let want = rms(&near[40_000..]);
        assert!(
            kept > want * 0.6 && kept < want * 1.6,
            "kept {kept} want {want}"
        );
    }

    #[test]
    fn silent_reference_passes_mic_through() {
        let mic = noise(1_000, 1);
        let out = NlmsCanceller::new(64).process(&mic, &[]);
        assert_eq!(out, mic);
    }
}

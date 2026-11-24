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

/// 10 ms frames: the envelope rate used for delay tracking and the bleed gate.
const FRAME_MS: i32 = 10;
/// Echo delay AEC3 is kept at. It only works for roughly 50 to 450 ms (measured), and the loopback
/// and microphone streams reach us with unknown offsets, so the mic is held back this long
/// and the delay is re-centered on this value when it drifts out of range.
const TARGET_DELAY_MS: i32 = 250;
const DELAY_OK_MS: std::ops::RangeInclusive<i32> = 80..=450;
/// Envelope history kept, in frames (6 s).
const HISTORY: usize = 520;
/// Delay search window in frames (-0.8 s to +1.2 s) and correlation window (4 s).
const SEARCH_FRAMES: std::ops::RangeInclusive<i32> = -80..=120;
const DELAY_WINDOW: usize = 300;
const DELAY_EVERY: u32 = 50;
/// Bleed gate: look at the last second, every 100 ms, over delays of 0 to 0.8 s.
const GATE_WINDOW: usize = 250;
const GATE_EVERY: u32 = 10;
const GATE_LAGS: std::ops::RangeInclusive<i32> = 8..=45;
const GATE_CORR: f32 = 0.75;
const GATE_HANGOVER: u32 = 30;
/// Envelope level (sample magnitude) below which a stream counts as silent.
const ACTIVE: f32 = 0.002;
/// Expected-bleed suppressor. The mic hears the speakers at a roughly steady fraction of what
/// they play (the coupling). It is learned from the quietest-but-typical part of the recent
/// mic-to-reference level ratios, which is the speakers alone because the user is usually
/// quiet while a video plays. Mic sound not clearly above that expected level is the speakers.
const SUP_WINDOW: usize = 30; // 300 ms levels
const SUP_EVERY: u32 = 5; // decide every 50 ms
const SUP_HISTORY: usize = 1_500; // coupling samples kept (about 15 s)
const SUP_MIN_SAMPLES: usize = 20;
const SUP_SETTLED: usize = 100; // 5 s of decisions
const SUP_PERCENTILE: f32 = 0.35;
/// Mic level must exceed expected bleed by this factor (about 8 dB) to count as the user.
const SUP_MARGIN: f32 = 2.5;
/// Reference level (mean magnitude) below which there is nothing to bleed.
const SUP_REF_MIN: f32 = 0.003;
const SUP_HANGOVER: u32 = 8;

/// Mean absolute level of a frame.
fn level(x: &[f32]) -> f32 {
    x.iter().map(|v| v.abs()).sum::<f32>() / x.len().max(1) as f32
}

/// Best normalized correlation between `x[t]` and `y[t - lag]` over the last `window` frames,
/// for lags in `lags`. None when the history is too short or either side is flat.
fn best_corr(
    x: &VecDeque<f32>,
    y: &VecDeque<f32>,
    window: usize,
    lags: std::ops::RangeInclusive<i32>,
) -> Option<(i32, f32)> {
    let max_back = (*lags.start()).min(0).unsigned_abs() as usize;
    let max_lag = (*lags.end()).max(0) as usize;
    let n = x.len().min(y.len());
    if n < window + max_lag + max_back {
        return None;
    }
    let xs: Vec<f32> = x.iter().rev().take(window).copied().collect(); // newest first
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    let mx = mean(&xs);
    let vx: f32 = xs.iter().map(|a| (a - mx) * (a - mx)).sum();
    if vx < 1e-9 {
        return None;
    }
    let ys: Vec<f32> = y.iter().rev().copied().collect(); // newest first
    let mut best: Option<(i32, f32)> = None;
    for lag in lags {
        let off = lag as isize;
        let seg: Vec<f32> = (0..window as isize)
            .map(|t| ys.get((t + off).max(0) as usize).copied().unwrap_or(0.0))
            .collect();
        let my = mean(&seg);
        let vy: f32 = seg.iter().map(|a| (a - my) * (a - my)).sum();
        if vy < 1e-9 {
            continue;
        }
        let cov: f32 = xs.iter().zip(&seg).map(|(a, b)| (a - mx) * (b - my)).sum();
        let c = cov / (vx * vy).sqrt();
        if best.is_none_or(|(_, b)| c > b) {
            best = Some((lag, c));
        }
    }
    best
}

fn push_env(q: &mut VecDeque<f32>, v: f32) {
    q.push_back(v);
    while q.len() > HISTORY {
        q.pop_front();
    }
}

pub struct EchoPath {
    apm: AudioProcessing,
    /// Mic samples; the flag marks silence we inserted to hold the mic back (never emitted).
    mic: VecDeque<(f32, bool)>,
    reference: VecDeque<f32>,
    out: Vec<f32>,
    // Envelopes (one value per frame) of what AEC3 was given and what it returned.
    env_mic: VecDeque<f32>,
    env_ref: VecDeque<f32>,
    env_clean: VecDeque<f32>,
    frames: u32,
    off_range: u32,
    gate_hold: u32,
    /// Samples the mic is held back by (the silence inserted into it).
    hold: usize,
    /// Frames by which the mic trails the reference (kept up to date by the delay tracker).
    lag_frames: usize,
    /// Recent mic-to-reference level ratios while the speakers play.
    ratios: VecDeque<f32>,
    sup_hold: u32,
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
        let mut mic = VecDeque::new();
        // Hold the mic back so the reference always leads it.
        mic.extend(std::iter::repeat_n(
            (0.0, true),
            (TARGET_DELAY_MS * 16) as usize,
        ));
        Self {
            apm,
            mic,
            reference: VecDeque::new(),
            out: Vec::new(),
            env_mic: VecDeque::new(),
            env_ref: VecDeque::new(),
            env_clean: VecDeque::new(),
            frames: 0,
            off_range: 0,
            gate_hold: 0,
            hold: (TARGET_DELAY_MS * 16) as usize,
            lag_frames: (TARGET_DELAY_MS / FRAME_MS) as usize,
            ratios: VecDeque::new(),
            sup_hold: 0,
        }
    }

    /// 16 kHz mono samples that went to the speakers (the loopback stream).
    pub fn push_reference(&mut self, samples: &[f32]) {
        self.reference.extend(samples);
        self.pump();
    }

    /// 16 kHz mono microphone samples.
    pub fn push_mic(&mut self, samples: &[f32]) {
        self.mic.extend(samples.iter().map(|s| (*s, false)));
        self.pump();
    }

    /// Cleaned microphone samples ready so far, oldest first.
    pub fn take(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.out)
    }

    /// Everything still waiting, unprocessed (pause or end). A partial last frame is kept as is.
    pub fn flush(&mut self) -> Vec<f32> {
        self.pump_with(true);
        self.out
            .extend(self.mic.drain(..).filter(|(_, inj)| !inj).map(|(v, _)| v));
        self.reference.clear();
        self.take()
    }

    fn pump(&mut self) {
        self.pump_with(false);
    }

    fn pump_with(&mut self, force: bool) {
        while self.mic.len() >= FRAME {
            let have_ref = self.reference.len() >= FRAME;
            // The mic is deliberately held back by `hold`; only waiting beyond that counts.
            if !have_ref && !force && self.mic.len() < self.hold + MAX_WAIT {
                break;
            }
            let mut r = [0.0f32; FRAME];
            if have_ref {
                for (d, s) in r.iter_mut().zip(self.reference.drain(..FRAME)) {
                    *d = s;
                }
            }
            let mut m = [0.0f32; FRAME];
            let mut injected = [false; FRAME];
            for ((d, inj), (s, i)) in m.iter_mut().zip(&mut injected).zip(self.mic.drain(..FRAME)) {
                *d = s;
                *inj = i;
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
            push_env(&mut self.env_mic, level(&m));
            push_env(&mut self.env_ref, level(&r));
            push_env(&mut self.env_clean, level(&clean));
            self.frames = self.frames.wrapping_add(1);
            if self.frames.is_multiple_of(GATE_EVERY) {
                self.update_gate();
            }
            if self.frames.is_multiple_of(SUP_EVERY) {
                self.update_suppressor();
            }
            if self.frames.is_multiple_of(DELAY_EVERY) {
                self.track_delay();
            }
            let gated = self.gate_hold > 0 || self.sup_hold > 0;
            self.gate_hold = self.gate_hold.saturating_sub(1);
            self.sup_hold = self.sup_hold.saturating_sub(1);
            for (v, inj) in clean.iter().zip(injected) {
                if !inj {
                    self.out.push(if gated { 0.0 } else { *v });
                }
            }
        }
    }

    fn ref_active(&self, frames: usize) -> bool {
        let n = self.env_ref.len().min(frames);
        n > 0 && self.env_ref.iter().rev().take(n).sum::<f32>() / n as f32 > ACTIVE
    }

    /// Bleed gate: speech that still correlates with what the speakers played, after the
    /// canceller, is the speakers and not the user. Near speech has its own rhythm and does not.
    fn update_gate(&mut self) {
        if !self.ref_active(GATE_WINDOW) {
            return;
        }
        let clean_active = self.env_clean.iter().rev().take(GATE_WINDOW).sum::<f32>()
            / GATE_WINDOW.min(self.env_clean.len()).max(1) as f32
            > ACTIVE;
        if !clean_active {
            return;
        }
        if let Some((_, c)) = best_corr(&self.env_clean, &self.env_ref, GATE_WINDOW, GATE_LAGS) {
            if c >= GATE_CORR {
                self.gate_hold = GATE_HANGOVER;
            }
        }
    }

    /// Mean level of the last `SUP_WINDOW` frames of the mic, and of the reference that was
    /// playing `lag_frames` earlier.
    fn window_levels(&self) -> Option<(f32, f32)> {
        let (m, r) = (&self.env_mic, &self.env_ref);
        if m.len() < SUP_WINDOW || r.len() < SUP_WINDOW + self.lag_frames {
            return None;
        }
        let mic = m.iter().rev().take(SUP_WINDOW).sum::<f32>() / SUP_WINDOW as f32;
        let refr = r
            .iter()
            .rev()
            .skip(self.lag_frames)
            .take(SUP_WINDOW)
            .sum::<f32>()
            / SUP_WINDOW as f32;
        Some((mic, refr))
    }

    /// Mute the mic while it carries nothing clearly above the bleed expected from the speakers.
    fn update_suppressor(&mut self) {
        let Some((mic, refr)) = self.window_levels() else {
            return;
        };
        if refr < SUP_REF_MIN {
            return;
        }
        self.ratios.push_back(mic / refr);
        while self.ratios.len() > SUP_HISTORY {
            self.ratios.pop_front();
        }
        if self.ratios.len() < SUP_MIN_SAMPLES {
            return;
        }
        let mut sorted: Vec<f32> = self.ratios.iter().copied().collect();
        sorted.sort_by(f32::total_cmp);
        // Early on the lowest ratios are the rising edge of a phrase, not the coupling: be more
        // generous until a few seconds of speaker sound have been seen.
        let pct = if sorted.len() < SUP_SETTLED {
            0.8
        } else {
            SUP_PERCENTILE
        };
        let coupling = sorted[((sorted.len() - 1) as f32 * pct) as usize];
        if mic <= coupling * refr * SUP_MARGIN {
            self.sup_hold = SUP_HANGOVER;
        }
    }

    /// Keep the echo delay AEC3 sees inside the range it can lock to. The measured offset is
    /// between the (held back) mic and the reference; when it leaves the range for two checks
    /// in a row, delay the mic or the reference to put it back on the target.
    fn track_delay(&mut self) {
        if !self.ref_active(DELAY_WINDOW) {
            self.off_range = 0;
            return;
        }
        let Some((lag, c)) = best_corr(&self.env_mic, &self.env_ref, DELAY_WINDOW, SEARCH_FRAMES)
        else {
            return;
        };
        if c < 0.35 {
            return;
        }
        let measured = lag * FRAME_MS;
        if DELAY_OK_MS.contains(&measured) {
            self.off_range = 0;
            self.lag_frames = lag.max(0) as usize;
            return;
        }
        self.off_range += 1;
        if self.off_range < 3 {
            return;
        }
        self.off_range = 0;
        let delta = (TARGET_DELAY_MS - measured) * 16;
        let zeros = std::iter::repeat_n(0.0f32, delta.unsigned_abs() as usize);
        if delta > 0 {
            self.mic.extend(zeros.map(|z| (z, true))); // more delay: hold the mic back further
            self.hold += delta as usize;
        } else {
            self.reference.extend(zeros);
        }
        self.lag_frames = (TARGET_DELAY_MS / FRAME_MS) as usize;
        // Old envelopes describe the old alignment.
        self.env_mic.clear();
        self.env_ref.clear();
        self.env_clean.clear();
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
                // Each talker has their own syllable rate, phrasing and pauses.
                let rate = 3.0 + (seed % 7) as f32 * 0.37;
                let phrase = 0.5
                    + 0.5
                        * (2.0 * std::f32::consts::PI * (0.23 + (seed % 5) as f32 * 0.07) * t
                            + seed as f32)
                            .sin();
                let env = (0.5 + 0.5 * (2.0 * std::f32::consts::PI * rate * t + seed as f32).sin())
                    .powi(2)
                    * phrase.powi(2);
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
        let me = speechlike(n, 4243);
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
        assert!(
            first >= 16_000 - (TARGET_DELAY_MS * 16) as usize - MAX_WAIT - FRAME,
            "only {first} released"
        );
        assert_eq!(first + e.flush().len(), 16_000);
    }

    /// Mic that hears the reference `ms` later (negative: the mic runs ahead of the reference),
    /// through a distorting speaker.
    fn echo_with_offset(refr: &[f32], ms: i32, gain: f32, drive: f32) -> Vec<f32> {
        let n = refr.len() as i32;
        let d = ms * 16;
        (0..n)
            .map(|i| {
                let g = |k: i32| {
                    let j = i - k;
                    if j >= 0 && j < n {
                        refr[j as usize]
                    } else {
                        0.0
                    }
                };
                let lin = gain * (g(d) + 0.4 * g(d + 23) + 0.2 * g(d + 61));
                (lin * drive).tanh() / drive
            })
            .collect()
    }

    #[test]
    fn echo_is_removed_whatever_the_stream_offset() {
        // AEC3 alone only works for echo delays of roughly 50 to 450 ms (measured); the mic
        // and loopback streams arrive with arbitrary offsets, so the path re-centers them.
        let n = 16_000 * 30;
        let refr = speechlike(n, 7);
        for ms in [-400, 20, 700, 1_000] {
            let mic = echo_with_offset(&refr, ms, 0.5, 2.0);
            let out = run(&mic, &refr);
            let t = n - 16_000 * 6..n;
            let db = 20.0 * (rms(&mic[t.clone()]) / rms(&out[t]).max(1e-9)).log10();
            eprintln!("offset {ms:>5} ms: reduction {db:5.1} dB");
            assert!(db > 20.0, "offset {ms} ms: reduced by only {db:.1} dB");
        }
    }

    #[test]
    fn loud_distorted_playback_is_still_removed() {
        // A small laptop speaker driven hard: heavy clipping that a linear canceller cannot model.
        let n = 16_000 * 30;
        let refr = speechlike(n, 7);
        let mic = echo_with_offset(&refr, 120, 1.5, 8.0);
        let out = run(&mic, &refr);
        let t = n - 16_000 * 6..n;
        let db = 20.0 * (rms(&mic[t.clone()]) / rms(&out[t]).max(1e-9)).log10();
        eprintln!("hard clipping: reduction {db:.1} dB");
        assert!(db > 20.0, "reduced by only {db:.1} dB");
    }

    #[test]
    fn speech_that_follows_the_playback_rhythm_but_cannot_be_cancelled_is_gated() {
        // Residual a linear canceller cannot model (different waveform, same loudness pattern).
        let n = 16_000 * 30;
        let refr = speechlike(n, 7);
        let mut x = 12_345u32;
        let mic: Vec<f32> = (0..n)
            .map(|i| {
                let a = i.saturating_sub(80);
                let b = (i + 80).min(n);
                let env = level(&refr[a..b]);
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                env * ((x >> 8) as f32 / 8_388_608.0 - 1.0)
            })
            .collect();
        let out = run(&mic, &refr);
        let t = n - 16_000 * 10..n;
        let db = 20.0 * (rms(&mic[t.clone()]) / rms(&out[t]).max(1e-9)).log10();
        eprintln!("rhythm-only residue: reduction {db:.1} dB");
        assert!(db > 15.0, "reduced by only {db:.1} dB");
    }

    /// A video through a reverberant, clipping laptop speaker with pauses between phrases: the
    /// first seconds (before AEC3 has converged and the coupling is known) are the hard part.
    #[test]
    fn phrases_through_a_reverberant_room_are_kept_out() {
        let n = 16_000 * 22;
        let base = speechlike(n, 7);
        let bursts: [(f32, f32); 3] = [(2.0, 4.5), (7.0, 9.5), (13.0, 18.0)];
        let refr: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / 16_000.0;
                if bursts.iter().any(|(a, b)| t >= *a && t < *b) {
                    base[i]
                } else {
                    0.0
                }
            })
            .collect();
        // Room: decaying noise impulse response (RT60 about 0.4 s), 120 ms direct delay.
        let mut x = 99u32;
        let rir: Vec<f32> = (0..6_400)
            .map(|k| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let w = (x >> 8) as f32 / 8_388_608.0 - 1.0;
                w * (-(k as f32) / 1_000.0).exp() * if k < 40 { 0.5 } else { 0.25 }
            })
            .collect();
        let delay = 1_900usize;
        let mut mic: Vec<f32> = (0..n)
            .map(|i| {
                let acc: f32 = rir
                    .iter()
                    .enumerate()
                    .step_by(4)
                    .filter(|(k, _)| i >= delay + k)
                    .map(|(k, h)| h * refr[i - delay - k])
                    .sum();
                (acc * 18.0).tanh() / 1.5
            })
            .collect();
        let mut y = 5u32;
        for v in &mut mic {
            y = y.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *v += ((y >> 8) as f32 / 8_388_608.0 - 1.0) * 0.002; // room noise
        }
        let out = run(&mic, &refr);
        for (i, (a, b)) in bursts.iter().enumerate() {
            let r = (a * 16_000.0) as usize..(b * 16_000.0) as usize;
            let db = 20.0 * (rms(&mic[r.clone()]) / rms(&out[r]).max(1e-9)).log10();
            eprintln!("phrase {i} ({a}-{b} s): reduction {db:.1} dB");
            // The first phrase gets less: nothing is learned yet.
            let need = if i == 0 { 12.0 } else { 20.0 };
            assert!(db > need, "phrase {i}: reduced by only {db:.1} dB");
        }
    }
}

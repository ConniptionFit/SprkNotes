//! End-to-end bleed simulation: capture (with WASAPI-like loopback that sends nothing during
//! silence and a session thread that sometimes blocks while decoding) feeding the echo path.
//! The unit tests in `echo.rs` feed the canceller in perfect lockstep; this one goes through
//! `LiveCapture`, so stream alignment across silences is part of what is tested.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use sprk_audio::echo::EchoPath;
use sprk_audio::live::{Backend, LiveCapture, Packet, PcmStream};
use sprk_audio::{AudioError, Source};
use std::sync::{Arc, Mutex};

const SR: usize = 16_000;

/// Deterministic speech-like signal (same shape as the unit tests use).
fn speechlike(n: usize, seed: u32) -> Vec<f32> {
    let mut x = seed;
    let mut lp = 0.0f32;
    (0..n)
        .map(|i| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let white = (x >> 8) as f32 / 8_388_608.0 - 1.0;
            lp = 0.6 * lp + 0.4 * white;
            let t = i as f32 / SR as f32;
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

/// A laptop speaker into its own mic: short acoustic delay, reverberation, clipping, room noise.
fn room(refr: &[f32]) -> Vec<f32> {
    let mut x = 99u32;
    let rir: Vec<(usize, f32)> = (0..4_800)
        .step_by(6)
        .map(|k| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let w = (x >> 8) as f32 / 8_388_608.0 - 1.0;
            let h = if k == 0 {
                0.6
            } else {
                w * (-(k as f32) / 1_100.0).exp() * 0.18
            };
            (k, h)
        })
        .collect();
    let delay = 48; // 3 ms of air
    let mut y = 5u32;
    (0..refr.len())
        .map(|i| {
            let acc: f32 = rir
                .iter()
                .filter(|(k, _)| i >= delay + k)
                .map(|(k, h)| h * refr[i - delay - k])
                .sum();
            y = y.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (acc * 6.0).tanh() / 4.0 + ((y >> 8) as f32 / 8_388_608.0 - 1.0) * 0.001
        })
        .collect()
}

/// What the devices have handed over and not yet been read: packets with their arrival time.
#[derive(Default)]
struct Feed {
    now_ms: u64,
    mic: Vec<(u64, Vec<f32>)>,
    sys: Vec<(u64, Vec<f32>)>,
}
type Shared = Arc<Mutex<Feed>>;

struct Stream(Shared, Source);
impl PcmStream for Stream {
    fn read(&mut self, out: &mut Vec<f32>) {
        let mut p = Vec::new();
        self.read_packets(&mut p);
        out.extend(p.into_iter().flat_map(|p| p.samples));
    }
    fn read_packets(&mut self, out: &mut Vec<Packet>) {
        let mut f = self.0.lock().unwrap();
        let now = f.now_ms;
        let q = match self.1 {
            Source::Mic => &mut f.mic,
            Source::System => &mut f.sys,
        };
        out.extend(q.drain(..).map(|(at, samples)| Packet {
            samples,
            age_ms: Some(now - at),
        }));
    }
}
struct Fake(Shared);
impl Backend for Fake {
    fn default_device(&self, s: Source) -> Option<String> {
        Some(format!("{s:?}"))
    }
    fn open(&mut self, s: Source, _d: &str) -> Result<Box<dyn PcmStream>, AudioError> {
        Ok(Box::new(Stream(self.0.clone(), s)))
    }
}

struct Run {
    mic: Vec<f32>,
    out: Vec<f32>,
}

/// Plays `refr` (silent stretches are not delivered at all, like WASAPI loopback), with `user`
/// spoken into the mic on top of the speaker echo. The session thread polls every 20 ms and
/// now and then blocks for a decode.
fn simulate(refr: &[f32], user: &[f32], mic_latency_ms: usize, sys_latency_ms: usize) -> Run {
    let n = refr.len();
    let echo = room(refr);
    let mic: Vec<f32> = echo.iter().zip(user).map(|(a, b)| a + b).collect();
    let playing: Vec<bool> = {
        // A player keeps its stream open for 200 ms after the sound stops.
        let mut p = vec![false; n];
        let mut last = None;
        for i in 0..n {
            if refr[i] != 0.0 {
                last = Some(i);
            }
            p[i] = last.is_some_and(|l| i - l < SR / 5);
        }
        p
    };
    let feed: Shared = Arc::default();
    let mut cap = LiveCapture::new(Box::new(Fake(feed.clone())));
    let mut echo_path = EchoPath::new();
    let mut out = Vec::new();
    let (mut mic_at, mut sys_at) = (0usize, 0usize);
    let mut t_ms = 0usize;
    let mut r = 7u32;
    cap.poll(0);
    while t_ms * SR / 1_000 < n + SR {
        // Session thread step: usually 20 ms, sometimes a decode of 0.3 to 1.5 s.
        r = r.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let step = if r.is_multiple_of(23) {
            300 + (r >> 16) as usize % 1_200
        } else {
            20
        };
        // Devices deliver in 10 ms packets while the thread is busy.
        for _ in 0..step / 10 {
            t_ms += 10;
            let now = t_ms * SR / 1_000;
            let mut f = feed.lock().unwrap();
            let m_end = now.saturating_sub(mic_latency_ms * SR / 1_000).min(n);
            if m_end > mic_at {
                f.mic.push((t_ms as u64, mic[mic_at..m_end].to_vec()));
                mic_at = m_end;
            }
            let s_end = now.saturating_sub(sys_latency_ms * SR / 1_000).min(n);
            while sys_at < s_end {
                // Packet by packet: nothing at all while nothing plays.
                let e = (sys_at + SR / 100).min(s_end);
                if playing[sys_at] {
                    f.sys.push((t_ms as u64, refr[sys_at..e].to_vec()));
                }
                sys_at = e;
            }
        }
        feed.lock().unwrap().now_ms = t_ms as u64;
        cap.poll(t_ms as u64);
        while let Some(fr) = cap.next_frame() {
            match fr.source {
                Source::Mic => echo_path.push_mic(&fr.samples),
                Source::System => echo_path.push_reference(&fr.samples),
            }
        }
        out.extend(echo_path.take());
    }
    out.extend(echo_path.flush());
    Run { mic, out }
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn bursts_signal(n: usize, bursts: &[(f32, f32)], seed: u32) -> Vec<f32> {
    let base = speechlike(n, seed);
    (0..n)
        .map(|i| {
            let t = i as f32 / SR as f32;
            if bursts.iter().any(|(a, b)| t >= *a && t < *b) {
                base[i]
            } else {
                0.0
            }
        })
        .collect()
}

/// Share of 50 ms blocks inside `r` where the echo was cut by less than 20 dB: what is left
/// there is loud enough to be transcribed.
fn leaky_blocks(run: &Run, r: std::ops::Range<usize>) -> (f32, f32) {
    let r = r.start..r.end.min(run.out.len()).min(run.mic.len());
    let db = 20.0 * (rms(&run.mic[r.clone()]) / rms(&run.out[r.clone()]).max(1e-9)).log10();
    let blocks: Vec<(f32, f32)> = run.mic[r.clone()]
        .chunks(800)
        .zip(run.out[r].chunks(800))
        .map(|(a, b)| (rms(a), rms(b)))
        .filter(|(a, _)| *a > 0.01)
        .collect();
    let leaky =
        blocks.iter().filter(|(a, b)| *b > 0.1 * *a).count() as f32 / blocks.len().max(1) as f32;
    (db, leaky)
}

const BURSTS: [(f32, f32); 9] = [
    (1.0, 3.5),
    (5.0, 6.0),
    (8.3, 11.0),
    (12.6, 13.4),
    (15.0, 19.0),
    (21.0, 22.2),
    (25.0, 28.0),
    (30.5, 31.3),
    (33.0, 37.0),
];

#[test]
fn playback_with_pauses_stays_out_of_the_mic_through_capture() {
    let n = SR * 40;
    let refr = bursts_signal(n, &BURSTS, 7);
    for (ml, sl) in [(30, 10), (10, 40), (120, 15)] {
        let run = simulate(&refr, &vec![0.0; n], ml, sl);
        // The mic stops delivering at the end and is then filled to the clock, so only
        // the start of the output is compared sample for sample.
        assert!(run.out.len() >= n, "output length {} vs {n}", run.out.len());
        let mut worst = 0.0f32;
        for (i, (a, b)) in BURSTS.iter().enumerate() {
            let r = (a * SR as f32) as usize..((b + 0.3) * SR as f32) as usize;
            let (db, leaky) = leaky_blocks(&run, r);
            eprintln!("latency mic {ml} sys {sl}: phrase {i} ({a}-{b} s) cut {db:5.1} dB, leaky blocks {leaky:.2}");
            if i > 0 {
                worst = worst.max(leaky);
            }
        }
        // The first phrase of a session is excluded: nothing is learned yet (ADR-013).
        assert!(
            worst <= 0.1,
            "mic {ml} ms, system {sl} ms: up to {worst:.2} of a phrase leaked"
        );
    }
}

#[test]
fn the_user_is_kept_while_playback_pauses_and_resumes() {
    let n = SR * 40;
    let refr = bursts_signal(n, &BURSTS, 7);
    // The user speaks in the gaps and over two of the phrases.
    let talk: [(f32, f32); 4] = [(3.8, 4.8), (11.3, 12.4), (16.0, 18.0), (34.0, 36.0)];
    let user: Vec<f32> = bursts_signal(n, &talk, 4243)
        .iter()
        .map(|v| v * 1.4)
        .collect();
    let run = simulate(&refr, &user, 30, 10);
    for (a, b) in talk {
        let r = ((a + 0.2) * SR as f32) as usize..(b * SR as f32) as usize;
        let kept = rms(&run.out[r.clone()]) / rms(&user[r.clone()]);
        eprintln!("user {a}-{b} s: kept {kept:.2}");
        assert!(kept > 0.6, "user at {a}-{b} s kept only {kept:.2}");
    }
}

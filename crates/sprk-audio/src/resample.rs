//! Streaming band-limited resampler (mono f32) for the capture path and offline files.
//!
//! Devices deliver 44.1 or 48 kHz; the engines and echo removal run at 16 kHz. Dropping to
//! 16 kHz needs a low-pass first: anything between 8 kHz and the device's Nyquist (sibilants,
//! keyboard clicks, fan noise, codec hiss) otherwise folds back into the speech band as
//! noise the model never saw in training. The old linear interpolator removed none of it at
//! 48 kHz (3:1 is a whole step, so it was plain decimation: a 12 kHz tone came out as a 4 kHz
//! tone at full level), so this is a Kaiser-windowed sinc low-pass evaluated as a polyphase
//! filter.
//!
//! Design: pass band to 87.5% of the lower Nyquist (7 kHz for 16 kHz out), stop band from
//! the lower Nyquist (8 kHz), about 70 dB down. At 48 kHz in that is ~210 taps, so ~3.4 M
//! multiply-adds per second per stream, and a fixed delay of ~2.2 ms. The delay is the same
//! for every stream at the same device rate, so mic and speaker stay lined up.
//!
//! Timing is exact: positions are tracked as integers in units of 1/L input samples, where
//! out/in = L/M in lowest terms. Usual device rates give L <= 160 and an exact filter row per
//! phase; unusual ones with L > 1024 interpolate between 1024 precomputed rows.

/// Most filter rows kept. Above this, rows are interpolated (timing stays exact).
const MAX_PHASES: u64 = 1_024;
/// Stop-band attenuation the filter is designed for, in dB.
const ATTEN_DB: f64 = 70.0;
/// Pass-band edge as a fraction of the lower Nyquist.
const PASS_FRACTION: f64 = 0.875;

pub struct Resampler {
    inner: Option<Poly>,
}

struct Poly {
    taps: usize,
    /// (phases + 1) rows of `taps`, each stored reversed so a row lines up with a slice of
    /// input ending at the newest sample.
    rows: Vec<f32>,
    phases: u64,
    l: u64,
    m: u64,
    buf: Vec<f32>,
    /// Index in `buf` of the newest input sample the next output needs.
    i: usize,
    /// Fractional position past `i`, in units of 1/l input samples.
    num: u64,
}

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        if in_rate == out_rate || in_rate == 0 || out_rate == 0 {
            return Self { inner: None };
        }
        Self {
            inner: Some(Poly::new(u64::from(in_rate), u64::from(out_rate))),
        }
    }

    /// Feed the next chunk; returns every output sample it completes.
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        match &mut self.inner {
            None => input.to_vec(),
            Some(p) => p.process(input),
        }
    }

    /// Delay the filter adds, in input samples (0 when the rates match).
    pub fn delay_in_samples(&self) -> f64 {
        self.inner.as_ref().map_or(0.0, |p| p.taps as f64 / 2.0)
    }

    /// Resample a whole clip, lined up with the input: output sample n sits at time n/out,
    /// and the length is floor(len * out / in). For files, where the tail matters.
    pub fn convert(in_rate: u32, out_rate: u32, samples: &[f32]) -> Vec<f32> {
        let mut r = Self::new(in_rate, out_rate);
        let Some(taps) = r.inner.as_ref().map(|p| p.taps) else {
            return samples.to_vec();
        };
        let want = (samples.len() as u64 * u64::from(out_rate) / u64::from(in_rate)) as usize;
        let skip = (taps as f64 / 2.0 * f64::from(out_rate) / f64::from(in_rate)).round() as usize;
        let mut out = r.process(samples);
        out.extend(r.process(&vec![0.0; taps + 1]));
        out.into_iter().skip(skip).take(want).collect()
    }
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Zeroth-order modified Bessel function of the first kind (series; converges fast here).
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, q) = (1.0, 1.0, x * x / 4.0);
    for k in 1..60 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < sum * 1e-12 {
            break;
        }
    }
    sum
}

impl Poly {
    fn new(in_rate: u64, out_rate: u64) -> Self {
        let g = gcd(in_rate, out_rate);
        let (l, m) = (out_rate / g, in_rate / g);
        let phases = l.min(MAX_PHASES);

        let nyq = in_rate.min(out_rate) as f64 / 2.0;
        let pass = nyq * PASS_FRACTION;
        let cutoff = (pass + nyq) / 2.0 / in_rate as f64; // cycles per input sample
        let width = 2.0 * std::f64::consts::PI * (nyq - pass) / in_rate as f64;
        let taps = (((ATTEN_DB - 8.0) / (2.285 * width)).ceil() as usize + 1).max(4);
        let beta = 0.1102 * (ATTEN_DB - 8.7);
        let half = taps as f64 / 2.0;
        let norm = bessel_i0(beta);
        let h = |t: f64| -> f64 {
            let x = t - half;
            let r = (x / half).clamp(-1.0, 1.0);
            let w = bessel_i0(beta * (1.0 - r * r).sqrt()) / norm;
            let s = if x.abs() < 1e-12 {
                1.0
            } else {
                let a = std::f64::consts::PI * 2.0 * cutoff * x;
                a.sin() / a
            };
            2.0 * cutoff * s * w
        };

        let mut rows = Vec::with_capacity((phases as usize + 1) * taps);
        for p in 0..=phases {
            let frac = p as f64 / phases as f64;
            let row: Vec<f64> = (0..taps).map(|j| h(frac + j as f64)).collect();
            // Unity gain at DC on every phase, so a steady level never ripples at the
            // phase rate.
            let sum: f64 = row.iter().sum();
            let sum = if sum.abs() < 1e-9 { 1.0 } else { sum };
            rows.extend(row.iter().rev().map(|v| (v / sum) as f32));
        }

        Self {
            taps,
            rows,
            phases,
            l,
            m,
            buf: vec![0.0; taps - 1],
            i: taps - 1,
            num: 0,
        }
    }

    fn row(&self, p: usize) -> &[f32] {
        &self.rows[p * self.taps..(p + 1) * self.taps]
    }

    fn process(&mut self, input: &[f32]) -> Vec<f32> {
        self.buf.extend_from_slice(input);
        let est = input.len() as u64 * self.l / self.m + 1;
        let mut out = Vec::with_capacity(est as usize);
        while self.i < self.buf.len() {
            let win = &self.buf[self.i + 1 - self.taps..=self.i];
            let y = if self.phases == self.l {
                dot(win, self.row(self.num as usize))
            } else {
                let scaled = self.num * self.phases;
                let p = (scaled / self.l) as usize;
                let a = (scaled % self.l) as f32 / self.l as f32;
                let (y0, y1) = (dot(win, self.row(p)), dot(win, self.row(p + 1)));
                y0 + (y1 - y0) * a
            };
            out.push(y);
            self.num += self.m;
            self.i += (self.num / self.l) as usize;
            self.num %= self.l;
        }
        // Keep only the history the next output needs.
        let drop = (self.i + 1).saturating_sub(self.taps).min(self.buf.len());
        self.buf.drain(..drop);
        self.i -= drop;
        out
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    // Four lanes so the compiler can vectorise without reassociation flags.
    let mut acc = [0.0f32; 4];
    let ((ca, ra), (cb, rb)) = (a.as_chunks::<4>(), b.as_chunks::<4>());
    for (x, y) in ca.iter().zip(cb) {
        for k in 0..4 {
            acc[k] += x[k] * y[k];
        }
    }
    let mut s = acc[0] + acc[1] + acc[2] + acc[3];
    for (x, y) in ra.iter().zip(rb) {
        s += x * y;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, hz: f64, secs: f64) -> Vec<f32> {
        let n = (f64::from(rate) * secs) as usize;
        (0..n)
            .map(|i| {
                (2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(rate)).sin() as f32 * 0.5
            })
            .collect()
    }

    /// Level of `out` relative to a 0.5-amplitude sine, in dB, skipping the warm-up.
    fn level_db(out: &[f32]) -> f64 {
        let body = &out[out.len() / 4..];
        let rms =
            (body.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / body.len() as f64).sqrt();
        20.0 * (rms / (0.5 / 2f64.sqrt())).log10()
    }

    fn through(in_rate: u32, hz: f64) -> f64 {
        let mut r = Resampler::new(in_rate, 16_000);
        level_db(&r.process(&tone(in_rate, hz, 1.0)))
    }

    #[test]
    fn speech_band_passes_at_full_level() {
        for rate in [44_100, 48_000, 96_000] {
            for hz in [200.0, 1_000.0, 3_000.0, 6_500.0] {
                let db = through(rate, hz);
                assert!(db.abs() < 0.2, "{rate} Hz in, {hz} Hz tone: {db:.2} dB");
            }
        }
    }

    #[test]
    fn sound_above_8_khz_does_not_fold_into_the_speech_band() {
        // 12 kHz at 48 kHz would land on 4 kHz, 9 kHz on 7 kHz, 15 kHz on 1 kHz.
        for rate in [44_100, 48_000] {
            for hz in [9_000.0, 12_000.0, 15_000.0, 20_000.0] {
                let db = through(rate, hz);
                assert!(
                    db < -60.0,
                    "{rate} Hz in, {hz} Hz tone came through at {db:.1} dB"
                );
            }
        }
    }

    #[test]
    fn chunked_matches_one_shot() {
        for rate in [44_100, 48_000, 22_050] {
            let input = tone(rate, 440.0, 0.3);
            let one = Resampler::new(rate, 16_000).process(&input);
            let mut r = Resampler::new(rate, 16_000);
            let chunked: Vec<f32> = input.chunks(441).flat_map(|c| r.process(c)).collect();
            assert_eq!(one.len(), chunked.len());
            for (a, b) in one.iter().zip(&chunked) {
                assert!((a - b).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn output_rate_is_exact_over_long_runs() {
        // A minute of 10 ms callbacks lands on the exact count (positions are integers, so
        // longer runs cannot drift either).
        for rate in [44_100u32, 48_000] {
            let mut r = Resampler::new(rate, 16_000);
            let chunk = vec![0.0f32; (rate / 100) as usize];
            let mut n = 0usize;
            for _ in 0..6_000 {
                n += r.process(&chunk).len();
            }
            assert_eq!(n, 16_000 * 60, "{rate} Hz in");
        }
    }

    #[test]
    fn same_rate_is_identity() {
        let input = [0.1f32, 0.2, 0.3, 0.4];
        assert_eq!(
            Resampler::new(16_000, 16_000).process(&input),
            input.to_vec()
        );
    }

    #[test]
    fn upsampling_from_a_narrowband_device_keeps_the_tone() {
        let mut r = Resampler::new(8_000, 16_000);
        let db = level_db(&r.process(&tone(8_000, 1_000.0, 1.0)));
        assert!(db.abs() < 0.2, "{db:.2} dB");
    }

    #[test]
    fn odd_rates_interpolate_rows_and_still_filter() {
        // 47 999 Hz shares no useful factor with 16 kHz: rows are interpolated.
        let mut r = Resampler::new(47_999, 16_000);
        let db = level_db(&r.process(&tone(47_999, 1_000.0, 1.0)));
        assert!(db.abs() < 0.2, "{db:.2} dB");
        let mut r = Resampler::new(47_999, 16_000);
        let db = level_db(&r.process(&tone(47_999, 12_000.0, 1.0)));
        assert!(db < -55.0, "{db:.1} dB");
    }

    #[test]
    fn convert_lines_a_clip_up_with_its_source() {
        // A click at 0.5 s comes out at 0.5 s, and the length is exact.
        let mut clip = vec![0.0f32; 48_000];
        clip[24_000] = 1.0;
        let out = Resampler::convert(48_000, 16_000, &clip);
        assert_eq!(out.len(), 16_000);
        let peak = out
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .map(|(i, _)| i);
        assert_eq!(peak, Some(8_000));
    }
}

//! Runtime tuning (ADR-015 Decision 2): read the device's hardware, time a short self-check,
//! and pick the model tier and decode threads. Portable: the same rules serve Android phones
//! and Windows laptops (Snapdragon X or Intel).
//!
//! Detection is cheap and needs no extra libraries. An AI accelerator (NPU) is *detected* and
//! reported, but v1 always decodes on the CPU: no NPU route runs our English models today.
//! The numbers from the self-check decide, not a table of chip names that goes stale.
//!
//! Desktop speed check (SPEC 16.8a): Windows laptops time a short decode of the built-in clip
//! ([`speed_clip`]) at a couple of thread counts ([`speed_candidates`]) once per model and
//! machine, and keep the fewest threads that are about as fast as the fastest
//! ([`pick_threads`]). Any error keeps [`default_threads`].

/// The two model tiers. One model family per tier, downloaded, never bundled (ADR-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Parakeet TDT 0.6B v2 int8: best accuracy, needs about 1.1 GB of RAM.
    Full,
    /// Whisper base.en int8: under 0.5 GB of RAM, for smaller or slower devices.
    Light,
}

/// An AI accelerator the device reports. Informational only in v1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Accelerator {
    /// Qualcomm Snapdragon (Hexagon NPU).
    QualcommHexagon,
    /// Samsung Exynos NPU.
    SamsungExynos,
    /// Google Tensor TPU.
    GoogleTensor,
    /// MediaTek Dimensity APU.
    MediaTek,
}

/// What the device says about itself. Filled by the platform shell (Android `Build`,
/// Windows `GetSystemInfo`) plus [`parse_cpuinfo`] and [`parse_meminfo_mb`] where available.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeviceProfile {
    /// SoC maker, e.g. `QTI` or `Samsung` (Android 12+ `Build.SOC_MANUFACTURER`).
    pub soc_manufacturer: Option<String>,
    /// SoC model, e.g. `SM8750` or `s5e9945` (Android 12+ `Build.SOC_MODEL`).
    pub soc_model: Option<String>,
    pub total_ram_mb: u64,
    /// Performance ("big") cores. 0 means unknown.
    pub big_cores: u32,
    /// All logical cores.
    pub cores: u32,
    pub cpu: CpuFeatures,
}

/// Arm CPU features that speed up int8 decoding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuFeatures {
    /// `asimddp`: int8 dot product.
    pub dotprod: bool,
    /// `i8mm`: int8 matrix multiply.
    pub i8mm: bool,
}

/// Self-check result: real-time factor of each tier on a bundled synthetic clip
/// (decode time divided by audio length; lower is faster). `None` when not measured.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SelfCheck {
    pub rtf_full: Option<f32>,
    pub rtf_light: Option<f32>,
}

/// The choice the app stores per device and shows in Settings ("Tuned for this phone").
#[derive(Debug, Clone, PartialEq)]
pub struct Tuning {
    pub tier: Tier,
    pub threads: u32,
    pub accelerator: Option<Accelerator>,
}

/// RAM below this never gets the Full tier (Parakeet needs about 1.1 GB while the OS and
/// other apps keep running).
pub const FULL_MIN_RAM_MB: u64 = 7_000;
/// The Full tier must decode at least this much faster than real time, so live text keeps
/// up and a phone does not run hot for an hour-long session.
pub const FULL_MAX_RTF: f32 = 0.35;

impl DeviceProfile {
    /// Which accelerator the SoC names suggest. Detection only; see the module docs.
    pub fn accelerator(&self) -> Option<Accelerator> {
        let maker = self
            .soc_manufacturer
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase();
        let model = self.soc_model.as_deref().unwrap_or("").to_ascii_lowercase();
        if maker.contains("qti") || maker.contains("qualcomm") || model.starts_with("sm") {
            Some(Accelerator::QualcommHexagon)
        } else if maker.contains("samsung") || model.starts_with("s5e") || model.contains("exynos")
        {
            Some(Accelerator::SamsungExynos)
        } else if maker.contains("google") || model.contains("tensor") {
            Some(Accelerator::GoogleTensor)
        } else if maker.contains("mediatek") || model.starts_with("mt") {
            Some(Accelerator::MediaTek)
        } else {
            None
        }
    }

    /// Decode threads: the big cores, capped at 4 (more adds heat, not speed, for one stream).
    /// Falls back to half the cores when big cores are unknown.
    pub fn threads(&self) -> u32 {
        let base = if self.big_cores > 0 {
            self.big_cores
        } else {
            self.cores / 2
        };
        base.clamp(1, 4)
    }
}

/// Pick the tier and threads. Full only when the device has the RAM and the self-check shows
/// Full keeps up; with no self-check yet, RAM and int8 dot product decide provisionally.
pub fn choose(device: &DeviceProfile, check: &SelfCheck) -> Tuning {
    let enough_ram = device.total_ram_mb >= FULL_MIN_RAM_MB;
    let fast_enough = match check.rtf_full {
        Some(rtf) => rtf <= FULL_MAX_RTF,
        None => device.cpu.dotprod,
    };
    Tuning {
        tier: if enough_ram && fast_enough {
            Tier::Full
        } else {
            Tier::Light
        },
        threads: device.threads(),
        accelerator: device.accelerator(),
    }
}

/// Read CPU features and core counts from `/proc/cpuinfo` text (Linux and Android).
/// Returns `(features, logical_cores)`.
pub fn parse_cpuinfo(text: &str) -> (CpuFeatures, u32) {
    let mut f = CpuFeatures::default();
    let mut cores = 0u32;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "processor" => cores += 1,
            "Features" | "flags" => {
                for word in value.split_whitespace() {
                    match word {
                        "asimddp" => f.dotprod = true,
                        "i8mm" => f.i8mm = true,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    (f, cores)
}

/// Total RAM in MB from `/proc/meminfo` text. `None` when the line is missing.
pub fn parse_meminfo_mb(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

/// Count big cores from per-core max frequencies (kHz), as read from
/// `/sys/devices/system/cpu/cpuN/cpufreq/cpuinfo_max_freq`: cores within 25% of the fastest
/// (prime and performance cores; efficiency cores fall well below).
pub fn big_cores_from_max_freqs(freqs_khz: &[u64]) -> u32 {
    let Some(&top) = freqs_khz.iter().max() else {
        return 0;
    };
    let cutoff = top - top / 4;
    freqs_khz.iter().filter(|&&f| f >= cutoff).count() as u32
}

/// Decode threads before any speed check: the logical cores, capped at 4.
pub fn default_threads() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get().min(4) as u32)
        .unwrap_or(2)
}

/// `SPRK_ASR_THREADS` when it is a positive number. It wins over the speed check, for
/// benchmarking on dev machines.
pub fn env_threads() -> Option<u32> {
    std::env::var("SPRK_ASR_THREADS")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|n| *n > 0)
}

/// Most threads the speed check tries. More only adds heat for one stream at a time.
pub const SPEED_MAX_THREADS: u32 = 8;

/// Thread counts the speed check times on a machine with `cores` logical cores: half the
/// default, the default, and twice the default, never above the cores or
/// [`SPEED_MAX_THREADS`]. Ascending, no repeats.
pub fn speed_candidates(cores: u32) -> Vec<u32> {
    let cores = cores.max(1);
    let d = cores.min(4);
    let mut v = vec![(d / 2).max(1), d, (d * 2).min(cores).min(SPEED_MAX_THREADS)];
    v.sort_unstable();
    v.dedup();
    v
}

/// One speed check timing: decode threads and the real-time factor (seconds spent decoding per
/// second of audio; lower is faster).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreadTiming {
    pub threads: u32,
    pub rtf: f32,
}

impl ThreadTiming {
    /// How many times faster than real time, as shown in Settings ("3.2x real time").
    pub fn times_real_time(&self) -> f32 {
        if self.rtf > 0.0 {
            1.0 / self.rtf
        } else {
            0.0
        }
    }
}

/// Within this share of the fastest timing, fewer threads win: the same speed with fewer
/// cores busy leaves room for the rest of the PC and runs cooler.
pub const THREADS_TOLERANCE: f32 = 0.10;

/// The speed check's choice: the fewest threads whose timing is within [`THREADS_TOLERANCE`]
/// of the fastest. Timings that are not a positive finite number are ignored; `None` when none
/// is left (the caller keeps [`default_threads`]).
pub fn pick_threads(timings: &[ThreadTiming]) -> Option<ThreadTiming> {
    let valid: Vec<ThreadTiming> = timings
        .iter()
        .copied()
        .filter(|t| t.threads > 0 && t.rtf.is_finite() && t.rtf > 0.0)
        .collect();
    let best = valid.iter().map(|t| t.rtf).fold(f32::INFINITY, f32::min);
    valid
        .into_iter()
        .filter(|t| t.rtf <= best * (1.0 + THREADS_TOLERANCE))
        .min_by_key(|t| t.threads)
}

/// Run the speed check: `time` decodes the clip with that many threads and returns the
/// real-time factor, or `None` when that try failed (or was called off). Each candidate is
/// tried once, in order; the result is [`pick_threads`] of what was measured.
pub fn speed_check(
    candidates: &[u32],
    mut time: impl FnMut(u32) -> Option<f32>,
) -> Option<ThreadTiming> {
    let timings: Vec<ThreadTiming> = candidates
        .iter()
        .filter_map(|&threads| time(threads).map(|rtf| ThreadTiming { threads, rtf }))
        .collect();
    pick_threads(&timings)
}

/// The built-in speed check clip: `seconds` of speech-like sound at 16 kHz (voiced tones with a
/// moving pitch, pulsed at syllable rate, short pauses between "words"). Made in code, the
/// same every time; no file and no real voice.
pub fn speed_clip(seconds: f32) -> Vec<f32> {
    let n = (seconds.max(0.0) * 16_000.0) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / 16_000.0;
            // Pitch drifts between about 100 and 160 Hz, like a speaking voice.
            let f0 = 130.0 + 30.0 * (std::f32::consts::TAU * 0.7 * t).sin();
            let phase = std::f32::consts::TAU * f0 * t;
            let voice = (1..=8)
                .map(|h| (phase * h as f32).sin() / h as f32)
                .sum::<f32>();
            // About four syllables a second; every fourth beat is a pause.
            let beat = (t * 4.0).fract();
            let gap = ((t * 4.0) as u32) % 4 == 3;
            let env = if gap {
                0.0
            } else {
                (std::f32::consts::PI * beat).sin()
            };
            0.15 * env * voice
        })
        .collect()
}

/// Real-time factor of one decode of `clip`: decode once to warm the engine up, then time a
/// second decode. `None` when a decode fails or the clip is empty.
pub fn time_decode(engine: &mut dyn crate::Engine, clip: &[f32]) -> Option<f32> {
    if clip.is_empty() {
        return None;
    }
    engine.transcribe(clip).ok()?;
    let started = std::time::Instant::now();
    engine.transcribe(clip).ok()?;
    let spent = started.elapsed().as_secs_f32();
    Some(spent / (clip.len() as f32 / 16_000.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn galaxy_s25() -> DeviceProfile {
        DeviceProfile {
            soc_manufacturer: Some("QTI".into()),
            soc_model: Some("SM8750".into()),
            total_ram_mb: 11_500,
            big_cores: 8,
            cores: 8,
            cpu: CpuFeatures {
                dotprod: true,
                i8mm: true,
            },
        }
    }

    #[test]
    fn flagship_that_keeps_up_gets_full() {
        let t = choose(
            &galaxy_s25(),
            &SelfCheck {
                rtf_full: Some(0.2),
                rtf_light: Some(0.08),
            },
        );
        assert_eq!(t.tier, Tier::Full);
        assert_eq!(t.threads, 4);
        assert_eq!(t.accelerator, Some(Accelerator::QualcommHexagon));
    }

    #[test]
    fn slow_self_check_drops_to_light() {
        let t = choose(
            &galaxy_s25(),
            &SelfCheck {
                rtf_full: Some(0.6),
                rtf_light: Some(0.2),
            },
        );
        assert_eq!(t.tier, Tier::Light);
    }

    #[test]
    fn small_ram_is_light_even_when_fast() {
        let mut d = galaxy_s25();
        d.total_ram_mb = 5_800;
        let t = choose(
            &d,
            &SelfCheck {
                rtf_full: Some(0.1),
                rtf_light: None,
            },
        );
        assert_eq!(t.tier, Tier::Light);
    }

    #[test]
    fn no_self_check_uses_dotprod_provisionally() {
        let mut d = galaxy_s25();
        assert_eq!(choose(&d, &SelfCheck::default()).tier, Tier::Full);
        d.cpu.dotprod = false;
        assert_eq!(choose(&d, &SelfCheck::default()).tier, Tier::Light);
    }

    #[test]
    fn exynos_is_detected() {
        let d = DeviceProfile {
            soc_manufacturer: Some("Samsung".into()),
            soc_model: Some("s5e9945".into()),
            ..Default::default()
        };
        assert_eq!(d.accelerator(), Some(Accelerator::SamsungExynos));
    }

    #[test]
    fn threads_fall_back_and_cap() {
        let d = DeviceProfile {
            cores: 8,
            ..Default::default()
        };
        assert_eq!(d.threads(), 4);
        let d = DeviceProfile {
            big_cores: 6,
            cores: 8,
            ..Default::default()
        };
        assert_eq!(d.threads(), 4);
        let d = DeviceProfile::default();
        assert_eq!(d.threads(), 1);
    }

    #[test]
    fn parses_android_cpuinfo() {
        let text = "processor\t: 0\nFeatures\t: fp asimd aes asimddp i8mm\n\nprocessor\t: 1\nFeatures\t: fp asimd asimddp\n";
        let (f, cores) = parse_cpuinfo(text);
        assert_eq!(cores, 2);
        assert!(f.dotprod && f.i8mm);
    }

    #[test]
    fn parses_meminfo() {
        assert_eq!(
            parse_meminfo_mb("MemTotal:       11776000 kB\nMemFree: 1 kB\n"),
            Some(11_500)
        );
        assert_eq!(parse_meminfo_mb("nothing here"), None);
    }

    #[test]
    fn counts_big_cores() {
        // Snapdragon 8 Elite: 2 prime cores at 4.32 GHz, 6 performance cores at 3.53 GHz.
        let f = [
            3_532_800, 3_532_800, 3_532_800, 3_532_800, 3_532_800, 3_532_800, 4_320_000, 4_320_000,
        ];
        assert_eq!(big_cores_from_max_freqs(&f), 8);
        // A big.LITTLE phone: 1 prime, 3 big, 4 little.
        let f = [
            1_950_000, 1_950_000, 1_950_000, 1_950_000, 2_850_000, 2_850_000, 2_850_000, 3_300_000,
        ];
        assert_eq!(big_cores_from_max_freqs(&f), 4);
        assert_eq!(big_cores_from_max_freqs(&[]), 0);
    }

    #[test]
    fn speed_candidates_halve_and_double_the_default_within_the_cores() {
        assert_eq!(speed_candidates(12), vec![2, 4, 8]);
        assert_eq!(speed_candidates(8), vec![2, 4, 8]);
        assert_eq!(speed_candidates(6), vec![2, 4, 6]);
        assert_eq!(speed_candidates(4), vec![2, 4]);
        assert_eq!(speed_candidates(2), vec![1, 2]);
        assert_eq!(speed_candidates(1), vec![1]);
        assert_eq!(speed_candidates(0), vec![1]);
    }

    fn t(threads: u32, rtf: f32) -> ThreadTiming {
        ThreadTiming { threads, rtf }
    }

    #[test]
    fn the_fastest_wins_when_it_is_clearly_faster() {
        let got = pick_threads(&[t(2, 0.5), t(4, 0.3), t(8, 0.2)]);
        assert_eq!(got, Some(t(8, 0.2)));
    }

    #[test]
    fn fewer_threads_win_when_about_as_fast() {
        // 8 threads is only 5% faster than 4: keep 4.
        let got = pick_threads(&[t(2, 0.5), t(4, 0.21), t(8, 0.2)]);
        assert_eq!(got, Some(t(4, 0.21)));
        // More threads were slower (efficiency cores joined in).
        let got = pick_threads(&[t(2, 0.4), t(4, 0.25), t(8, 0.33)]);
        assert_eq!(got, Some(t(4, 0.25)));
    }

    #[test]
    fn bad_timings_are_ignored_and_nothing_left_means_no_choice() {
        let got = pick_threads(&[t(2, f32::NAN), t(4, 0.0), t(8, 0.3), t(0, 0.1)]);
        assert_eq!(got, Some(t(8, 0.3)));
        assert_eq!(pick_threads(&[t(2, f32::INFINITY), t(4, -1.0)]), None);
        assert_eq!(pick_threads(&[]), None);
    }

    #[test]
    fn speed_check_times_each_candidate_once_and_skips_failures() {
        let mut seen = Vec::new();
        let got = speed_check(&[2, 4, 8], |n| {
            seen.push(n);
            match n {
                2 => Some(0.6),
                4 => None, // that load failed
                _ => Some(0.25),
            }
        });
        assert_eq!(seen, vec![2, 4, 8]);
        assert_eq!(got, Some(t(8, 0.25)));
        assert_eq!(speed_check(&[2, 4], |_| None), None);
    }

    #[test]
    fn times_real_time_is_the_inverse_of_the_factor() {
        assert!((t(4, 0.3125).times_real_time() - 3.2).abs() < 1e-4);
        assert_eq!(t(4, 0.0).times_real_time(), 0.0);
    }

    #[test]
    fn the_clip_is_the_same_every_time_and_has_sound_and_pauses() {
        let a = speed_clip(3.0);
        assert_eq!(a.len(), 48_000);
        assert_eq!(a, speed_clip(3.0));
        let peak = a.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.1 && peak < 1.0, "{peak}");
        // The fourth beat of each second (0.75 s to 1.0 s) is silent.
        assert!(a[12_100..15_900].iter().all(|v| *v == 0.0));
    }

    struct Counting(u32);
    impl crate::Engine for Counting {
        fn transcribe(&mut self, _s: &[f32]) -> Result<Vec<crate::Segment>, crate::AsrError> {
            self.0 += 1;
            Ok(vec![])
        }
    }
    struct Failing;
    impl crate::Engine for Failing {
        fn transcribe(&mut self, _s: &[f32]) -> Result<Vec<crate::Segment>, crate::AsrError> {
            Err(crate::AsrError::Rejected)
        }
    }

    #[test]
    fn time_decode_warms_up_first_and_reports_failures() {
        let mut e = Counting(0);
        let rtf = time_decode(&mut e, &speed_clip(0.5));
        assert_eq!(e.0, 2, "one warm-up decode, one timed");
        assert!(rtf.is_some_and(|r| r >= 0.0 && r.is_finite()));
        assert_eq!(time_decode(&mut Failing, &speed_clip(0.5)), None);
        assert_eq!(time_decode(&mut Counting(0), &[]), None);
    }
}

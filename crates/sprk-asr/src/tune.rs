//! Runtime tuning (ADR-015 Decision 2): read the device's hardware, time a short self-check,
//! and pick the model tier and decode threads. Portable: the same rules serve Android phones
//! and Windows laptops (Snapdragon X or Intel).
//!
//! Detection is cheap and needs no extra libraries. An AI accelerator (NPU) is *detected* and
//! reported, but v1 always decodes on the CPU: no NPU route runs our English models today.
//! The numbers from the self-check decide, not a table of chip names that goes stale.

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
}

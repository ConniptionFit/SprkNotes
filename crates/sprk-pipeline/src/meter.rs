//! Input level for the Me and Others buttons (SPEC 8.1). Only a number between 0 and 1 leaves
//! here, a few times a second; no audio is kept, stored or logged.

/// Quietest level shown, in dBFS. Below it the meter reads 0.
const FLOOR_DB: f32 = -60.0;

/// Peak level of the audio fed since the last read.
#[derive(Debug, Default, Clone, Copy)]
pub struct Meter {
    peak: f32,
}

impl Meter {
    pub fn push(&mut self, samples: &[f32]) {
        for s in samples {
            let a = s.abs();
            if a > self.peak {
                self.peak = a;
            }
        }
    }

    /// The level since the last call, on a 0 to 1 scale that follows loudness (dB), then reset.
    /// 0 is -60 dBFS or quieter, 1 is full scale. Speech at a normal distance reads about 0.5
    /// to 0.8, so a muted or wrong device is obvious at a glance.
    pub fn take(&mut self) -> f32 {
        let p = std::mem::take(&mut self.peak);
        level_from_peak(p)
    }
}

/// 0 to 1 from a sample peak, linear in dB between FLOOR_DB and 0 dBFS.
pub fn level_from_peak(peak: f32) -> f32 {
    if !peak.is_finite() || peak <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * peak.min(1.0).log10();
    ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_zero_and_full_scale_is_one() {
        assert_eq!(level_from_peak(0.0), 0.0);
        assert_eq!(level_from_peak(1e-4), 0.0, "-80 dBFS is below the floor");
        assert!((level_from_peak(1.0) - 1.0).abs() < 1e-6);
        assert!(
            (level_from_peak(2.0) - 1.0).abs() < 1e-6,
            "clipped input reads full"
        );
        assert_eq!(level_from_peak(f32::NAN), 0.0);
    }

    #[test]
    fn follows_loudness_in_db() {
        // -20 dBFS (typical speech peak) reads two thirds of the way up.
        assert!((level_from_peak(0.1) - 2.0 / 3.0).abs() < 1e-4);
        assert!(
            (level_from_peak(0.001) - 0.0).abs() < 1e-4,
            "-60 dBFS is the floor"
        );
    }

    #[test]
    fn take_reports_the_peak_since_the_last_read_and_resets() {
        let mut m = Meter::default();
        m.push(&[0.0, -0.1, 0.05]);
        assert!((m.take() - 2.0 / 3.0).abs() < 1e-4);
        assert_eq!(m.take(), 0.0, "nothing fed since");
    }
}

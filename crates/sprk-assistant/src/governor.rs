//! Keeps the assistant a polite guest (ADR-009): which model fits in the memory that is free,
//! when to pause because transcription is behind, when to unload, and how many threads to use.
//! All of it is pure and takes times and numbers from the caller, so it tests on Linux.
use crate::engine::Control;

/// Assistant sizes, smallest first. Order matters: a bigger tier compares greater.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    Off,
    Titles,
    Light,
    Standard,
}

impl Tier {
    /// Parses a pack tier or the `AssistantTier` policy value. `auto` means no cap.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "off" => Some(Self::Off),
            "titles" => Some(Self::Titles),
            "light" => Some(Self::Light),
            "standard" | "auto" => Some(Self::Standard),
            _ => None,
        }
    }

    /// Free memory that must remain after the model loads, so transcription and the user's
    /// other apps are not squeezed.
    pub fn headroom_mb(self) -> u64 {
        match self {
            Self::Off | Self::Titles => 1024,
            Self::Light => 2048,
            Self::Standard => 3072,
        }
    }
}

/// An installed assistant model, as far as the governor cares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackChoice {
    pub id_version: String,
    pub tier: Tier,
    pub ram_estimate_mb: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("the assistant is turned off")]
    Off,
    #[error("no assistant model is installed")]
    NoPack,
    #[error("not enough free memory right now")]
    NotEnoughMemory,
}

/// Picks the biggest installed model that is allowed by policy and the user's size choice and
/// still leaves its headroom free. Falls back to a smaller tier before refusing.
pub fn choose_pack(
    available_mb: u64,
    policy_max: Tier,
    user_tier: Tier,
    packs: &[PackChoice],
) -> Result<&PackChoice, Refusal> {
    let ceiling = policy_max.min(user_tier);
    if ceiling == Tier::Off {
        return Err(Refusal::Off);
    }
    let mut allowed: Vec<&PackChoice> = packs
        .iter()
        .filter(|p| p.tier != Tier::Off && p.tier <= ceiling)
        .collect();
    if allowed.is_empty() {
        return Err(Refusal::NoPack);
    }
    allowed.sort_by(|a, b| b.tier.cmp(&a.tier).then(b.id_version.cmp(&a.id_version)));
    allowed
        .into_iter()
        .find(|p| available_mb >= u64::from(p.ram_estimate_mb) + p.tier.headroom_mb())
        .ok_or(Refusal::NotEnoughMemory)
}

/// Thread count: a third of the cores, at most 4, at least 1, then the admin cap.
pub fn default_threads(cores: usize, policy_max: Option<u64>) -> usize {
    let base = (cores / 3).clamp(1, 4);
    match policy_max {
        Some(m) if m >= 1 => base.min(usize::try_from(m).unwrap_or(usize::MAX)),
        _ => base,
    }
}

/// Lag in milliseconds from "utterance ended" to "line visible", the number transcription
/// already measures (SPEC 13.3).
pub const LAG_LIMIT_MS: u64 = 2_500;
const BAD_FOR_MS: u64 = 3_000;
const GOOD_FOR_MS: u64 = 5_000;
const CANCEL_AFTER_HOLD_MS: u64 = 20_000;

/// Yields to transcription. Feed it the current lag as often as it is measured.
#[derive(Debug, Clone, Default)]
pub struct LagYield {
    bad_since: Option<u64>,
    good_since: Option<u64>,
    held_since: Option<u64>,
}

impl LagYield {
    /// Returns what the engine should do now.
    pub fn update(&mut self, now_ms: u64, lag_ms: u64) -> Control {
        if lag_ms > LAG_LIMIT_MS {
            self.good_since = None;
            self.bad_since.get_or_insert(now_ms);
        } else {
            self.bad_since = None;
            self.good_since.get_or_insert(now_ms);
        }
        match self.held_since {
            None => {
                if self.bad_since.is_some_and(|b| now_ms - b >= BAD_FOR_MS) {
                    self.held_since = Some(now_ms);
                    return Control::Hold;
                }
                Control::Run
            }
            Some(h) => {
                if self.good_since.is_some_and(|g| now_ms - g >= GOOD_FOR_MS) {
                    self.held_since = None;
                    self.bad_since = None;
                    return Control::Run;
                }
                if now_ms - h >= CANCEL_AFTER_HOLD_MS {
                    return Control::Cancel;
                }
                Control::Hold
            }
        }
    }
}

/// Unloads the model after it has sat unused for `minutes` (`None` means never).
#[derive(Debug, Clone, Copy)]
pub struct IdleUnload {
    pub minutes: Option<u64>,
    last_used_ms: u64,
}

impl IdleUnload {
    pub fn new(minutes: Option<u64>, now_ms: u64) -> Self {
        Self {
            minutes,
            last_used_ms: now_ms,
        }
    }

    pub fn touch(&mut self, now_ms: u64) {
        self.last_used_ms = now_ms;
    }

    pub fn should_unload(&self, now_ms: u64, busy: bool) -> bool {
        match self.minutes {
            Some(m) if !busy => now_ms.saturating_sub(self.last_used_ms) >= m * 60_000,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(id: &str, tier: Tier, ram: u32) -> PackChoice {
        PackChoice {
            id_version: id.into(),
            tier,
            ram_estimate_mb: ram,
        }
    }

    fn installed() -> Vec<PackChoice> {
        vec![
            pack("tiny@2027.01.0", Tier::Titles, 700),
            pack("two@2027.01.0", Tier::Light, 1800),
            pack("four@2027.01.0", Tier::Standard, 3300),
        ]
    }

    #[test]
    fn light_is_the_pick_when_the_user_chose_light() {
        let p = installed();
        let c = choose_pack(8000, Tier::Standard, Tier::Light, &p).unwrap();
        assert_eq!(c.id_version, "two@2027.01.0");
    }

    #[test]
    fn standard_when_allowed_and_memory_is_plentiful() {
        let p = installed();
        let c = choose_pack(8000, Tier::Standard, Tier::Standard, &p).unwrap();
        assert_eq!(c.tier, Tier::Standard);
    }

    #[test]
    fn drops_a_tier_under_memory_pressure_then_refuses() {
        let p = installed();
        // Standard needs 3300 + 3072, Light needs 1800 + 2048, Titles needs 700 + 1024.
        let c = choose_pack(4000, Tier::Standard, Tier::Standard, &p).unwrap();
        assert_eq!(c.tier, Tier::Light);
        let c = choose_pack(2000, Tier::Standard, Tier::Standard, &p).unwrap();
        assert_eq!(c.tier, Tier::Titles);
        assert_eq!(
            choose_pack(1500, Tier::Standard, Tier::Standard, &p),
            Err(Refusal::NotEnoughMemory)
        );
    }

    #[test]
    fn policy_cap_off_and_missing_packs() {
        let p = installed();
        let c = choose_pack(9000, Tier::Titles, Tier::Standard, &p).unwrap();
        assert_eq!(c.tier, Tier::Titles);
        assert_eq!(
            choose_pack(9000, Tier::Off, Tier::Light, &p),
            Err(Refusal::Off)
        );
        assert_eq!(
            choose_pack(9000, Tier::Standard, Tier::Light, &[]),
            Err(Refusal::NoPack)
        );
        // Only a Standard pack is installed but the cap is Light: nothing allowed.
        let only = [pack("four@2027.01.0", Tier::Standard, 3300)];
        assert_eq!(
            choose_pack(9000, Tier::Light, Tier::Light, &only),
            Err(Refusal::NoPack)
        );
    }

    #[test]
    fn tier_parsing() {
        assert_eq!(Tier::parse("auto"), Some(Tier::Standard));
        assert_eq!(Tier::parse("light"), Some(Tier::Light));
        assert_eq!(Tier::parse("huge"), None);
        assert!(Tier::Light < Tier::Standard);
    }

    #[test]
    fn threads_are_a_third_of_cores_capped() {
        assert_eq!(default_threads(12, None), 4);
        assert_eq!(default_threads(8, None), 2);
        assert_eq!(default_threads(2, None), 1);
        assert_eq!(default_threads(24, None), 4);
        assert_eq!(default_threads(12, Some(2)), 2);
        assert_eq!(default_threads(12, Some(0)), 4);
    }

    #[test]
    fn lag_yield_holds_after_sustained_lag_and_resumes_when_healthy() {
        let mut y = LagYield::default();
        assert_eq!(y.update(0, 500), Control::Run);
        assert_eq!(y.update(1_000, 3_000), Control::Run);
        assert_eq!(y.update(3_999, 3_000), Control::Run);
        assert_eq!(y.update(4_000, 3_000), Control::Hold);
        assert_eq!(y.update(5_000, 3_000), Control::Hold);
        // Healthy again, but not for long enough yet.
        assert_eq!(y.update(6_000, 400), Control::Hold);
        assert_eq!(y.update(10_999, 400), Control::Hold);
        assert_eq!(y.update(11_000, 400), Control::Run);
    }

    #[test]
    fn a_short_spike_is_ignored() {
        let mut y = LagYield::default();
        assert_eq!(y.update(0, 4_000), Control::Run);
        assert_eq!(y.update(2_000, 4_000), Control::Run);
        assert_eq!(y.update(2_500, 300), Control::Run);
        assert_eq!(y.update(6_000, 4_000), Control::Run);
    }

    #[test]
    fn a_long_hold_ends_in_cancel() {
        let mut y = LagYield::default();
        y.update(0, 4_000);
        assert_eq!(y.update(3_000, 4_000), Control::Hold);
        assert_eq!(y.update(22_999, 4_000), Control::Hold);
        assert_eq!(y.update(23_000, 4_000), Control::Cancel);
    }

    #[test]
    fn idle_unload_waits_and_never_unloads_while_busy() {
        let mut i = IdleUnload::new(Some(5), 0);
        assert!(!i.should_unload(299_999, false));
        assert!(i.should_unload(300_000, false));
        assert!(!i.should_unload(400_000, true));
        i.touch(400_000);
        assert!(!i.should_unload(500_000, false));
        assert!(!IdleUnload::new(None, 0).should_unload(u64::MAX, false));
    }
}

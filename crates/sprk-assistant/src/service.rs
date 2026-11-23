//! What the app shows and allows (ADR-009): combines admin policy, the user's choice, the
//! installed assistant packs and free memory into one answer. Pure, so it tests on Linux.
use crate::features::FeatureSet;
use crate::governor::{choose_pack, PackChoice, Refusal, Tier};
use sprk_config::AppPolicy;

/// The user's own choices (the Settings section). Off and Light until they change them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserSettings {
    pub enabled: bool,
    pub tier: Tier,
    /// What the user turned on. The extras start off.
    pub features: FeatureSet,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            tier: Tier::Light,
            features: FeatureSet::default(),
        }
    }
}

/// Policy and user choice merged. `*_locked` means policy decides and the control is read-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effective {
    pub enabled: bool,
    pub enabled_locked: bool,
    /// The size the app will try: the user's choice, capped by policy.
    pub tier: Tier,
    pub tier_locked: bool,
    pub max_tier: Tier,
    pub background: bool,
    pub save_outputs: bool,
    /// Minutes before an idle model unloads. `None` means never.
    pub idle_unload_minutes: Option<u64>,
    pub max_threads: Option<u64>,
    /// The user's choice, narrowed by the `AssistantFeatures` policy list.
    pub features: FeatureSet,
}

pub const DEFAULT_IDLE_UNLOAD_MINUTES: u64 = 5;

pub fn effective(policy: &AppPolicy, user: &UserSettings) -> Effective {
    let policy_cap = policy
        .assistant_tier
        .as_deref()
        .and_then(Tier::parse)
        .unwrap_or(Tier::Standard);
    // `AssistantTier=off` is a hard off, same as AssistantEnabled=0.
    let off_by_tier = policy_cap == Tier::Off;
    let enabled = match policy.assistant_enabled {
        Some(v) => v && !off_by_tier,
        None => user.enabled && !off_by_tier,
    };
    let idle = match policy.assistant_idle_unload_minutes {
        Some(0) => None,
        Some(m) => Some(m),
        None => Some(DEFAULT_IDLE_UNLOAD_MINUTES),
    };
    Effective {
        enabled,
        enabled_locked: policy.assistant_enabled.is_some() || off_by_tier,
        tier: user.tier.min(policy_cap),
        tier_locked: policy.assistant_tier.is_some(),
        max_tier: policy_cap,
        background: policy.assistant_background.unwrap_or(true),
        save_outputs: policy.assistant_save_outputs.unwrap_or(true),
        idle_unload_minutes: idle,
        max_threads: policy.assistant_max_threads,
        features: match policy.assistant_features.as_deref() {
            Some(list) => user.features.intersect(FeatureSet::parse_list(list)),
            None => user.features,
        },
    }
}

/// What the Settings row and the Ask button show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Turned off by the user or by policy.
    Off {
        locked: bool,
    },
    /// On, but no assistant model is installed. `can_download` follows `ModelDownload`.
    NoModel {
        can_download: bool,
    },
    /// Installed, but there is not enough free memory for any allowed size right now.
    NotEnoughMemory,
    Ready {
        id_version: String,
        tier: Tier,
    },
}

pub fn status(
    eff: &Effective,
    packs: &[PackChoice],
    available_mb: u64,
    can_download: bool,
) -> Status {
    if !eff.enabled {
        return Status::Off {
            locked: eff.enabled_locked,
        };
    }
    match choose_pack(available_mb, eff.max_tier, eff.tier, packs) {
        Ok(p) => Status::Ready {
            id_version: p.id_version.clone(),
            tier: p.tier,
        },
        Err(Refusal::NotEnoughMemory) => Status::NotEnoughMemory,
        Err(Refusal::NoPack) => Status::NoModel { can_download },
        Err(Refusal::Off) => Status::Off { locked: true },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprk_config::FakePolicy;

    fn policy(pairs: &[(&str, &str)]) -> AppPolicy {
        AppPolicy::load(&FakePolicy(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ))
    }

    fn pack(tier: Tier, ram: u32) -> PackChoice {
        PackChoice {
            id_version: format!("m-{tier:?}@2027.01.0"),
            tier,
            ram_estimate_mb: ram,
        }
    }

    #[test]
    fn off_and_light_by_default() {
        let e = effective(&policy(&[]), &UserSettings::default());
        assert!(!e.enabled && !e.enabled_locked);
        assert_eq!(e.tier, Tier::Light);
        assert_eq!(e.idle_unload_minutes, Some(5));
        assert!(e.background);
    }

    #[test]
    fn user_can_turn_it_on_when_policy_is_silent() {
        let u = UserSettings {
            enabled: true,
            ..UserSettings::default()
        };
        assert!(effective(&policy(&[]), &u).enabled);
    }

    #[test]
    fn policy_wins_and_locks() {
        let u = UserSettings {
            enabled: true,
            tier: Tier::Standard,
            ..UserSettings::default()
        };
        let off = effective(&policy(&[("AssistantEnabled", "0")]), &u);
        assert!(!off.enabled && off.enabled_locked);
        let on = effective(
            &policy(&[("AssistantEnabled", "1")]),
            &UserSettings::default(),
        );
        assert!(on.enabled && on.enabled_locked);
        let capped = effective(&policy(&[("AssistantTier", "titles")]), &u);
        assert_eq!(capped.tier, Tier::Titles);
        assert!(capped.tier_locked);
        let hard_off = effective(&policy(&[("AssistantTier", "off")]), &u);
        assert!(!hard_off.enabled && hard_off.enabled_locked);
    }

    #[test]
    fn idle_unload_zero_means_never() {
        let e = effective(
            &policy(&[("AssistantIdleUnloadMinutes", "0")]),
            &UserSettings::default(),
        );
        assert_eq!(e.idle_unload_minutes, None);
        let e = effective(
            &policy(&[("AssistantIdleUnloadMinutes", "15")]),
            &UserSettings::default(),
        );
        assert_eq!(e.idle_unload_minutes, Some(15));
    }

    #[test]
    fn status_walks_through_the_states() {
        let on = UserSettings {
            enabled: true,
            ..UserSettings::default()
        };
        let e = effective(&policy(&[]), &on);
        let packs = [pack(Tier::Light, 1800), pack(Tier::Standard, 3300)];
        assert_eq!(
            status(
                &effective(&policy(&[]), &UserSettings::default()),
                &packs,
                9000,
                true
            ),
            Status::Off { locked: false }
        );
        assert_eq!(
            status(&e, &[], 9000, true),
            Status::NoModel { can_download: true }
        );
        assert_eq!(
            status(&e, &[], 9000, false),
            Status::NoModel {
                can_download: false
            }
        );
        assert_eq!(status(&e, &packs, 1000, true), Status::NotEnoughMemory);
        // Light is the user's size, so Light is picked even with plenty of memory.
        assert_eq!(
            status(&e, &packs, 9000, true),
            Status::Ready {
                id_version: "m-Light@2027.01.0".into(),
                tier: Tier::Light
            }
        );
    }

    #[test]
    fn extras_are_off_and_policy_only_narrows() {
        let e = effective(&policy(&[]), &UserSettings::default());
        assert!(e.features.has(crate::features::Feature::Recap));
        assert!(!e.features.has(crate::features::Feature::Agenda));
        let user = UserSettings {
            features: FeatureSet::all(),
            ..UserSettings::default()
        };
        let e = effective(&policy(&[("AssistantFeatures", "recap,agenda")]), &user);
        assert_eq!(e.features.keys(), vec!["recap", "agenda"]);
    }
}

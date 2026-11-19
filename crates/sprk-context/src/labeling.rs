//! Call context: window-title parsing and speaker labeling modes with hysteresis
//! (SPEC 19.2 to 19.4). Never guesses a name: when unsure, remote speech is `Others`.

/// What one poll saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observation {
    /// No reliable signal, Teams closed, providers unavailable.
    Unknown,
    /// Confirmed 1:1 call, with the other person's display name.
    OneToOne(String),
    /// Three or more people.
    Group,
}

/// Confirmed labeling mode (SPEC 19.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Unknown,
    Named(String),
    Group,
}

/// An event line to write when the mode changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeChange {
    pub at_ms: u64,
    pub text: String,
}

/// Number of consecutive agreeing polls needed to switch modes.
pub const CONFIRM_POLLS: u32 = 2;

pub struct LabelTracker {
    mode: Mode,
    /// (candidate, polls seen, session time of the first sign of change)
    pending: Option<(Mode, u32, u64)>,
}

impl Default for LabelTracker {
    fn default() -> Self {
        Self::new()
    }
}

fn to_mode(o: &Observation) -> Mode {
    match o {
        Observation::Unknown => Mode::Unknown,
        Observation::OneToOne(n) => Mode::Named(n.clone()),
        Observation::Group => Mode::Group,
    }
}

impl LabelTracker {
    pub fn new() -> Self {
        Self {
            mode: Mode::Unknown,
            pending: None,
        }
    }

    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    /// Label for remote (system audio) speech right now. Between the first sign of a
    /// change and its confirmation a named label is withheld: never guess a name.
    pub fn remote_label(&self) -> &str {
        match (&self.mode, &self.pending) {
            (Mode::Named(n), None) => n,
            _ => "Others",
        }
    }

    /// Feed one poll. Returns the mode change event when a switch is confirmed. The event is
    /// stamped with the time of the first sign of change (SPEC 19.4 example).
    pub fn observe(&mut self, obs: &Observation, t_ms: u64) -> Option<ModeChange> {
        let cand = to_mode(obs);
        if cand == self.mode {
            self.pending = None;
            return None;
        }
        let (count, first_ms) = match &self.pending {
            Some((m, n, t0)) if *m == cand => (n + 1, *t0),
            _ => (1, t_ms),
        };
        if count < CONFIRM_POLLS {
            self.pending = Some((cand, count, first_ms));
            return None;
        }
        let text = match (&self.mode, &cand) {
            (_, Mode::Group) => {
                "Call became a group call: remote speakers labeled Others".to_string()
            }
            (Mode::Group, Mode::Named(n)) => {
                format!("Back to 1:1 with {n}: remote speaker labeled {n}")
            }
            (_, Mode::Named(n)) => format!("Call with {n}: remote speaker labeled {n}"),
            (_, Mode::Unknown) => {
                "Call context unavailable: remote speakers labeled Others".to_string()
            }
        };
        self.mode = cand;
        self.pending = None;
        Some(ModeChange {
            at_ms: first_ms,
            text,
        })
    }
}

/// Parsing rules for a Teams call window title. Kept as data so they can be patched
/// without a release (SPEC 19.2 provider 3).
#[derive(Debug, Clone)]
pub struct TitleRules {
    pub strip_suffixes: Vec<String>,
    pub group_keywords: Vec<String>,
    /// Whole titles that are not a call (the bare app window).
    pub not_a_call: Vec<String>,
}

impl Default for TitleRules {
    fn default() -> Self {
        Self {
            strip_suffixes: vec![" | Microsoft Teams".into(), " - Microsoft Teams".into()],
            group_keywords: vec!["meeting".into(), "group".into(), "call with".into()],
            not_a_call: vec![
                "microsoft teams".into(),
                "teams".into(),
                "calendar".into(),
                "chat".into(),
            ],
        }
    }
}

fn looks_like_person(s: &str) -> bool {
    let parts: Vec<&str> = s.split_whitespace().collect();
    (2..=4).contains(&parts.len())
        && parts.iter().all(|p| {
            p.chars().next().is_some_and(|c| c.is_uppercase())
                && p.chars()
                    .all(|c| c.is_alphabetic() || c == '-' || c == '\'' || c == '.')
        })
}

/// Parse a Teams call window title. Anything that does not parse cleanly is `Unknown`.
pub fn parse_call_title(title: &str, rules: &TitleRules) -> Observation {
    let mut t = title.trim();
    for suf in &rules.strip_suffixes {
        if let Some(rest) = t.strip_suffix(suf.as_str()) {
            t = rest.trim();
            break;
        }
    }
    if t.is_empty() {
        return Observation::Unknown;
    }
    let lower = t.to_lowercase();
    if rules.not_a_call.contains(&lower) {
        return Observation::Unknown;
    }
    if rules
        .group_keywords
        .iter()
        .any(|k| lower.contains(k.as_str()))
    {
        return Observation::Group;
    }
    if looks_like_person(t) {
        return Observation::OneToOne(t.to_string());
    }
    Observation::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAM: &str = "Sam Sample";

    fn one() -> Observation {
        Observation::OneToOne(SAM.into())
    }

    #[test]
    fn needs_two_agreeing_polls_to_name() {
        let mut t = LabelTracker::new();
        assert_eq!(t.observe(&one(), 0), None);
        assert_eq!(
            t.remote_label(),
            "Others",
            "never guess before confirmation"
        );
        let ch = t.observe(&one(), 15_000).expect("confirmed");
        assert_eq!(
            ch.text,
            "Call with Sam Sample: remote speaker labeled Sam Sample"
        );
        assert_eq!(ch.at_ms, 0);
        assert_eq!(t.remote_label(), SAM);
    }

    #[test]
    fn single_blip_does_not_flap() {
        let mut t = LabelTracker::new();
        t.observe(&one(), 0);
        t.observe(&one(), 15_000);
        assert_eq!(t.observe(&Observation::Group, 30_000), None);
        assert_eq!(
            t.remote_label(),
            "Others",
            "named label withheld while a change is pending"
        );
        assert_eq!(t.observe(&one(), 45_000), None);
        assert_eq!(t.remote_label(), SAM, "blip cleared, name restored");
        assert_eq!(t.mode(), &Mode::Named(SAM.into()));
    }

    #[test]
    fn group_and_back_matches_spec_events() {
        let mut t = LabelTracker::new();
        t.observe(&one(), 0);
        t.observe(&one(), 15_000);
        t.observe(&Observation::Group, 1_205_000);
        let g = t.observe(&Observation::Group, 1_220_000).unwrap();
        assert_eq!(g.at_ms, 1_205_000);
        assert_eq!(
            g.text,
            "Call became a group call: remote speakers labeled Others"
        );
        assert_eq!(t.remote_label(), "Others");
        t.observe(&one(), 1_900_000);
        let b = t.observe(&one(), 1_915_000).unwrap();
        assert_eq!(
            b.text,
            "Back to 1:1 with Sam Sample: remote speaker labeled Sam Sample"
        );
    }

    #[test]
    fn unavailable_providers_stay_unknown_silently() {
        let mut t = LabelTracker::new();
        for i in 0..10 {
            assert_eq!(t.observe(&Observation::Unknown, i * 15_000), None);
        }
        assert_eq!(t.remote_label(), "Others");
    }

    #[test]
    fn title_parsing() {
        let r = TitleRules::default();
        assert_eq!(
            parse_call_title("Sam Sample | Microsoft Teams", &r),
            Observation::OneToOne("Sam Sample".into())
        );
        assert_eq!(
            parse_call_title("Weekly IT Sync meeting | Microsoft Teams", &r),
            Observation::Group
        );
        assert_eq!(
            parse_call_title("Microsoft Teams", &r),
            Observation::Unknown
        );
        assert_eq!(parse_call_title("calendar", &r), Observation::Unknown);
        assert_eq!(parse_call_title("", &r), Observation::Unknown);
        assert_eq!(
            parse_call_title("Mary-Jane O'Neil - Microsoft Teams", &r),
            Observation::OneToOne("Mary-Jane O'Neil".into())
        );
    }
}

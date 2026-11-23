//! Which assistant features are on (ADR-009). The core four are on once the assistant is on;
//! every extra is off until the user turns it on. Policy can only narrow the list.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    Title,
    Recap,
    Actions,
    Ask,
    Agenda,
    Commitments,
    Pins,
    ParkingLot,
    Explain,
    Shorten,
    Prep,
    /// "Was I asked something?": questions from Others since you last spoke. No model.
    Asked,
    /// A summary of the whole meeting so far, on this PC.
    Summary,
    /// A short catch-up written in the background every 10 minutes of meeting time.
    Rolling,
}

pub const ALL: [Feature; 14] = [
    Feature::Title,
    Feature::Recap,
    Feature::Actions,
    Feature::Ask,
    Feature::Agenda,
    Feature::Commitments,
    Feature::Pins,
    Feature::ParkingLot,
    Feature::Explain,
    Feature::Shorten,
    Feature::Prep,
    Feature::Asked,
    Feature::Summary,
    Feature::Rolling,
];

impl Feature {
    /// Stable name used in policy values and the UI bridge.
    pub fn key(self) -> &'static str {
        match self {
            Feature::Title => "title",
            Feature::Recap => "recap",
            Feature::Actions => "actions",
            Feature::Ask => "ask",
            Feature::Agenda => "agenda",
            Feature::Commitments => "commitments",
            Feature::Pins => "pins",
            Feature::ParkingLot => "parkinglot",
            Feature::Explain => "explain",
            Feature::Shorten => "shorten",
            Feature::Prep => "prep",
            Feature::Asked => "asked",
            Feature::Summary => "summary",
            Feature::Rolling => "rolling",
        }
    }

    pub fn parse(s: &str) -> Option<Feature> {
        let s = s.trim().to_ascii_lowercase();
        ALL.into_iter().find(|f| f.key() == s)
    }

    pub fn is_core(self) -> bool {
        matches!(
            self,
            Feature::Title | Feature::Recap | Feature::Actions | Feature::Ask
        )
    }

    fn bit(self) -> u16 {
        1 << ALL.iter().position(|f| *f == self).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureSet(u16);

impl FeatureSet {
    pub fn none() -> Self {
        FeatureSet(0)
    }
    pub fn all() -> Self {
        ALL.into_iter().fold(Self::none(), |s, f| s.with(f, true))
    }
    /// The core features on, every extra off.
    pub fn core() -> Self {
        ALL.into_iter()
            .filter(|f| f.is_core())
            .fold(Self::none(), |s, f| s.with(f, true))
    }
    pub fn has(self, f: Feature) -> bool {
        self.0 & f.bit() != 0
    }
    pub fn with(self, f: Feature, on: bool) -> Self {
        if on {
            FeatureSet(self.0 | f.bit())
        } else {
            FeatureSet(self.0 & !f.bit())
        }
    }
    pub fn intersect(self, other: Self) -> Self {
        FeatureSet(self.0 & other.0)
    }
    /// Parses a comma or semicolon separated list; unknown names are ignored.
    pub fn parse_list(s: &str) -> Self {
        s.split([',', ';'])
            .filter_map(Feature::parse)
            .fold(Self::none(), |set, f| set.with(f, true))
    }
    pub fn keys(self) -> Vec<&'static str> {
        ALL.into_iter()
            .filter(|f| self.has(*f))
            .map(Feature::key)
            .collect()
    }
}

impl Default for FeatureSet {
    fn default() -> Self {
        Self::core()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_on_extras_off_by_default() {
        let s = FeatureSet::default();
        assert!(s.has(Feature::Recap) && s.has(Feature::Ask));
        assert!(!s.has(Feature::Agenda) && !s.has(Feature::Prep));
        assert_eq!(s.keys(), vec!["title", "recap", "actions", "ask"]);
    }

    #[test]
    fn parse_and_intersect() {
        let allow = FeatureSet::parse_list("Recap; pins, nonsense ,prep");
        assert_eq!(allow.keys(), vec!["recap", "pins", "prep"]);
        let user = FeatureSet::core().with(Feature::Pins, true);
        assert_eq!(user.intersect(allow).keys(), vec!["recap", "pins"]);
        assert_eq!(FeatureSet::all().keys().len(), ALL.len());
    }
}

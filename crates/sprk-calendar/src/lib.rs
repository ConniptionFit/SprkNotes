//! Calendar seam and meeting selection (SPEC 7). Local only: no Graph, no network.
//! The Outlook COM provider (cfg(windows)) fills [`Meeting`]; everything here is pure.
//!
//! Meeting subjects, organizers, invitees and bodies are PHI-grade (CLAUDE.md rule 11):
//! they go only into transcript files. `Meeting` therefore has a redacting `Debug`.
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Teams,
    Zoom,
    Other,
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Teams => "teams",
            Platform::Zoom => "zoom",
            Platform::Other => "other",
        }
    }

    /// A `teams.microsoft.com` join link means Teams, `zoom.us` means Zoom (SPEC 7.1).
    pub fn detect(location: &str, body: &str) -> Platform {
        let hay = format!("{location}\n{body}").to_ascii_lowercase();
        if hay.contains("teams.microsoft.com") {
            Platform::Teams
        } else if hay.contains("zoom.us") {
            Platform::Zoom
        } else {
            Platform::Other
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    Free,
    Tentative,
    Busy,
    OutOfOffice,
    WorkingElsewhere,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Meeting {
    /// Opaque provider id (safe to log).
    pub id: String,
    pub start_epoch: i64,
    pub end_epoch: i64,
    pub subject: String,
    pub organizer: String,
    pub required: Vec<String>,
    pub optional: Vec<String>,
    pub location: String,
    pub body: String,
    pub cancelled: bool,
    pub all_day: bool,
    pub busy: Busy,
}

impl fmt::Debug for Meeting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Meeting")
            .field("id", &self.id)
            .field("start_epoch", &self.start_epoch)
            .field("end_epoch", &self.end_epoch)
            .finish_non_exhaustive()
    }
}

impl Meeting {
    pub fn new(id: &str, subject: &str, start_epoch: i64, end_epoch: i64) -> Self {
        Meeting {
            id: id.into(),
            start_epoch,
            end_epoch,
            subject: subject.into(),
            organizer: String::new(),
            required: vec![],
            optional: vec![],
            location: String::new(),
            body: String::new(),
            cancelled: false,
            all_day: false,
            busy: Busy::Busy,
        }
    }

    pub fn platform(&self) -> Platform {
        Platform::detect(&self.location, &self.body)
    }

    /// SPEC 7.2 step 1: skip cancelled, all-day and "free" events.
    pub fn eligible(&self) -> bool {
        !self.cancelled && !self.all_day && self.busy != Busy::Free
    }
}

pub trait CalendarProvider: Send {
    /// Meetings in the lookup window. Empty when the provider is unavailable.
    fn meetings_between(&self, from_epoch: i64, to_epoch: i64) -> Vec<Meeting>;
}

/// Lookup window around now (SPEC 7.1): now minus 12 h to now plus 15 min.
pub const LOOKBACK_SECS: i64 = 12 * 3600;
pub const LOOKAHEAD_SECS: i64 = 15 * 60;

pub fn window(now: i64) -> (i64, i64) {
    (now - LOOKBACK_SECS, now + LOOKAHEAD_SECS)
}

/// SPEC 7.2: meetings in progress first (most recent start first), then upcoming within
/// 15 minutes (soonest first). The first entry is the default pick; more than one means
/// the name field offers a dropdown.
pub fn candidates(all: &[Meeting], now: i64) -> Vec<Meeting> {
    let mut live: Vec<&Meeting> = all
        .iter()
        .filter(|m| m.eligible() && m.start_epoch <= now && now < m.end_epoch)
        .collect();
    live.sort_by_key(|m| std::cmp::Reverse(m.start_epoch));
    let mut soon: Vec<&Meeting> = all
        .iter()
        .filter(|m| m.eligible() && m.start_epoch > now && m.start_epoch <= now + LOOKAHEAD_SECS)
        .collect();
    soon.sort_by_key(|m| m.start_epoch);
    live.into_iter().chain(soon).cloned().collect()
}

/// Convenience for the 5-minute re-check: query the provider and apply the rules.
pub fn find(provider: &dyn CalendarProvider, now: i64) -> Vec<Meeting> {
    let (a, b) = window(now);
    candidates(&provider.meetings_between(a, b), now)
}

/// Markdown for the transcript's `## Meeting context` section (SPEC 7.3), accepted by the
/// user. `fmt_time` renders an epoch as local text (the caller owns the clock and zone).
pub fn context_section(m: &Meeting, fmt_time: &dyn Fn(i64) -> String) -> String {
    let flat = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut s = String::from("## Meeting context\n");
    s.push_str(&format!("- **Subject:** {}\n", flat(&m.subject)));
    if !m.organizer.is_empty() {
        s.push_str(&format!("- **Organizer:** {}\n", flat(&m.organizer)));
    }
    s.push_str(&format!(
        "- **Scheduled:** {} to {}\n",
        fmt_time(m.start_epoch),
        fmt_time(m.end_epoch)
    ));
    if !m.location.is_empty() {
        s.push_str(&format!("- **Location:** {}\n", flat(&m.location)));
    }
    s.push_str(&format!("- **Platform:** {}\n", m.platform().as_str()));
    if !m.required.is_empty() {
        s.push_str(&format!(
            "- **Invitees (required):** {}\n",
            m.required.join(", ")
        ));
    }
    if !m.optional.is_empty() {
        s.push_str(&format!(
            "- **Invitees (optional):** {}\n",
            m.optional.join(", ")
        ));
    }
    let body = m.body.replace('\r', "");
    let body = body.trim();
    if !body.is_empty() {
        s.push_str("\n```text\n");
        // A fence inside the body must not end ours early.
        s.push_str(&body.replace("```", "'''"));
        s.push_str("\n```\n");
    }
    s.push('\n');
    s
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextPolicy {
    Ask,
    Always,
    Never,
}

pub struct FakeCalendar(pub Vec<Meeting>);
impl CalendarProvider for FakeCalendar {
    fn meetings_between(&self, from: i64, to: i64) -> Vec<Meeting> {
        self.0
            .iter()
            .filter(|m| m.end_epoch > from && m.start_epoch < to)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000;

    #[test]
    fn platform_detection() {
        assert_eq!(
            Platform::detect("", "Join: https://teams.microsoft.com/l/meetup-join/x"),
            Platform::Teams
        );
        assert_eq!(
            Platform::detect("https://acme.zoom.us/j/1", ""),
            Platform::Zoom
        );
        assert_eq!(Platform::detect("Room 4", "agenda"), Platform::Other);
    }

    #[test]
    fn skips_cancelled_all_day_and_free() {
        let mut a = Meeting::new("a", "A", NOW - 10, NOW + 100);
        a.cancelled = true;
        let mut b = Meeting::new("b", "B", NOW - 10, NOW + 100);
        b.all_day = true;
        let mut c = Meeting::new("c", "C", NOW - 10, NOW + 100);
        c.busy = Busy::Free;
        assert!(candidates(&[a, b, c], NOW).is_empty());
    }

    #[test]
    fn prefers_in_progress_most_recent_start_then_soonest_upcoming() {
        let old = Meeting::new("old", "Old", NOW - 3600, NOW + 600);
        let recent = Meeting::new("recent", "Recent", NOW - 60, NOW + 600);
        let soon = Meeting::new("soon", "Soon", NOW + 300, NOW + 900);
        let later = Meeting::new("later", "Later", NOW + 600, NOW + 1200);
        let far = Meeting::new("far", "Far", NOW + 20 * 60, NOW + 3000);
        let got: Vec<String> = candidates(&[far, later, soon.clone(), old, recent], NOW)
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(got, vec!["recent", "old", "soon", "later"]);
    }

    #[test]
    fn ended_meetings_are_not_offered() {
        let done = Meeting::new("d", "D", NOW - 3600, NOW - 1);
        assert!(candidates(&[done], NOW).is_empty());
    }

    #[test]
    fn find_applies_window_and_rules() {
        let cal = FakeCalendar(vec![
            Meeting::new("in", "In", NOW - 100, NOW + 100),
            Meeting::new("yesterday", "Y", NOW - 20 * 3600, NOW - 19 * 3600),
        ]);
        let got = find(&cal, NOW);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "in");
    }

    #[test]
    fn debug_never_prints_subject_or_body() {
        let mut m = Meeting::new("id-1", "Sensitive Subject", 1, 2);
        m.body = "secret body".into();
        m.organizer = "Pat Organizer".into();
        let d = format!("{m:?}");
        assert!(d.contains("id-1"));
        assert!(!d.contains("Sensitive") && !d.contains("secret") && !d.contains("Pat"));
    }

    #[test]
    fn context_section_renders_and_guards_fences() {
        let mut m = Meeting::new("i", "Budget\nreview", 0, 3600);
        m.organizer = "Pat Organizer".into();
        m.required = vec!["Sam Sample".into()];
        m.location = "https://teams.microsoft.com/l/x".into();
        m.body = "Agenda\r\n```\nx".into();
        let s = context_section(&m, &|e| format!("t{e}"));
        assert!(s.starts_with("## Meeting context\n- **Subject:** Budget review\n"));
        assert!(s.contains("- **Scheduled:** t0 to t3600"));
        assert!(s.contains("- **Platform:** teams"));
        assert!(s.contains("- **Invitees (required):** Sam Sample"));
        assert_eq!(s.matches("```").count(), 2);
    }
}

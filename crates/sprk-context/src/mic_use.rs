//! Another app holding the microphone (SPEC 8.3a). Read-only: SprkNotes only looks at which other
//! apps hold a microphone, never what they hear, and never touches it. App ids and names stay in
//! memory for the prompt and the user's "Don't ask for this app" list; nothing here is logged.

/// The apps other than SprkNotes using a microphone right now, as stable ids (see [`app_id`]).
pub trait MicUse: Send {
    fn apps_in_use(&self) -> Vec<String>;
}

/// Test and non-Windows stand-in: answers whatever it holds.
#[derive(Clone, Default)]
pub struct FakeMicUse(pub std::sync::Arc<std::sync::Mutex<Vec<String>>>);
impl MicUse for FakeMicUse {
    fn apps_in_use(&self) -> Vec<String> {
        self.0.lock().map(|v| v.clone()).unwrap_or_default()
    }
}

/// A stable id for a privacy-data entry name. Packaged apps are named by package family
/// (`Microsoft.Teams_8wekyb3d8bbwe`); unpackaged ones by path with `#` for `\`
/// (`C:#Users#jo#AppData#Local#Zoom#bin#Zoom.exe`). The path holds user and version folders, so
/// an unpackaged app is known by its file name alone. Lower case.
pub fn app_id(entry: &str, packaged: bool) -> String {
    let id = if packaged {
        entry
    } else {
        entry.rsplit(['#', '\\', '/']).next().unwrap_or(entry)
    };
    id.trim().to_lowercase()
}

/// A readable name for an app id, for the prompt and Settings: `microsoft.teams_8wekyb3d8bbwe`
/// becomes "Microsoft Teams" and `zoom.exe` becomes "Zoom".
pub fn display_name(id: &str) -> String {
    let base = match id.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("exe") => stem,
        _ => id.split('_').next().unwrap_or(id),
    };
    let words: Vec<String> = base
        .split(['.', '_', '-', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().chain(c).collect(),
                None => String::new(),
            }
        })
        .collect();
    if words.is_empty() {
        "Another app".into()
    } else {
        words.join(" ")
    }
}

/// The apps still worth asking about: those not on the user's skip list.
pub fn not_skipped(apps: Vec<String>, skipped: &[String]) -> Vec<String> {
    apps.into_iter()
        .filter(|a| !skipped.iter().any(|s| s.eq_ignore_ascii_case(a)))
        .collect()
}

/// How long another app must hold the microphone before SprkNotes asks. Long enough that a
/// voice typing burst, a sound check or a notification does not count; short enough that a
/// meeting has barely begun.
pub const HOLD_MS: u64 = 5_000;

/// What the app should do after a poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicPrompt {
    /// Nothing changes.
    Nothing,
    /// Ask "Start a transcript?" now.
    Ask,
    /// The reason for the question went away (the call ended, or a session started): take it
    /// down if it is still showing.
    Withdraw,
}

/// Decides when to ask. One question per hold: once asked, answered or not, SprkNotes waits for
/// the other app to let go of the microphone before it can ask again. A hold during which a
/// SprkNotes session ran counts as answered, so ending a session mid-call never re-asks.
#[derive(Debug, Default)]
pub struct MicWatch {
    since: Option<u64>,
    asked: bool,
    showing: bool,
}

impl MicWatch {
    pub fn new() -> Self {
        Self::default()
    }

    /// `in_use`: another app holds the microphone. `busy`: a SprkNotes session is running.
    pub fn poll(&mut self, now_ms: u64, in_use: bool, busy: bool) -> MicPrompt {
        if !in_use {
            self.since = None;
            self.asked = false;
            return self.withdraw();
        }
        let since = *self.since.get_or_insert(now_ms);
        if busy {
            self.asked = true;
            return self.withdraw();
        }
        if !self.asked && now_ms.saturating_sub(since) >= HOLD_MS {
            self.asked = true;
            self.showing = true;
            return MicPrompt::Ask;
        }
        MicPrompt::Nothing
    }

    /// The user answered (Start or Dismiss); the question is no longer up.
    pub fn answered(&mut self) {
        self.showing = false;
    }

    fn withdraw(&mut self) -> MicPrompt {
        if std::mem::take(&mut self.showing) {
            MicPrompt::Withdraw
        } else {
            MicPrompt::Nothing
        }
    }
}

/// FILETIME ticks (100 ns since 1601-01-01 UTC) for a Unix time in seconds.
pub fn filetime_from_unix(secs: u64) -> u64 {
    (secs + 11_644_473_600) * 10_000_000
}

/// An app's last microphone use, as Windows privacy settings keep it: `start` and `stop` are
/// FILETIME ticks, `stop` is 0 while the app is still using it. A start older than `max_age`
/// with no stop is treated as stale (an app that crashed mid-use never writes its stop), so a
/// crashed app does not make SprkNotes ask on every launch.
pub fn entry_in_use(start: u64, stop: u64, now: u64, max_age_secs: u64) -> bool {
    stop == 0 && start != 0 && now.saturating_sub(start) <= max_age_secs * 10_000_000
}

/// Longest a single microphone use is believed (12 hours).
pub const MAX_USE_SECS: u64 = 12 * 3_600;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_once_after_a_hold_and_not_for_a_brief_use() {
        let mut w = MicWatch::new();
        assert_eq!(w.poll(0, true, false), MicPrompt::Nothing);
        assert_eq!(w.poll(3_000, false, false), MicPrompt::Nothing, "brief use");
        assert_eq!(w.poll(4_000, true, false), MicPrompt::Nothing);
        assert_eq!(w.poll(8_000, true, false), MicPrompt::Nothing);
        assert_eq!(w.poll(9_000, true, false), MicPrompt::Ask);
        assert_eq!(
            w.poll(11_000, true, false),
            MicPrompt::Nothing,
            "asked once"
        );
        w.answered();
        assert_eq!(
            w.poll(60_000, true, false),
            MicPrompt::Nothing,
            "dismissed stays dismissed"
        );
    }

    #[test]
    fn asks_again_only_after_the_microphone_was_let_go() {
        let mut w = MicWatch::new();
        w.poll(0, true, false);
        assert_eq!(w.poll(5_000, true, false), MicPrompt::Ask);
        w.answered();
        assert_eq!(w.poll(7_000, false, false), MicPrompt::Nothing);
        w.poll(10_000, true, false);
        assert_eq!(w.poll(15_000, true, false), MicPrompt::Ask, "a new call");
    }

    #[test]
    fn takes_the_question_down_when_the_call_ends_or_a_session_starts() {
        let mut w = MicWatch::new();
        w.poll(0, true, false);
        assert_eq!(w.poll(5_000, true, false), MicPrompt::Ask);
        assert_eq!(w.poll(6_000, false, false), MicPrompt::Withdraw);
        w.poll(10_000, true, false);
        assert_eq!(w.poll(15_000, true, false), MicPrompt::Ask);
        assert_eq!(w.poll(16_000, true, true), MicPrompt::Withdraw);
    }

    #[test]
    fn never_asks_during_a_session_or_after_one_in_the_same_call() {
        let mut w = MicWatch::new();
        assert_eq!(w.poll(0, true, true), MicPrompt::Nothing);
        assert_eq!(w.poll(10_000, true, true), MicPrompt::Nothing);
        // The session ended while the call goes on.
        assert_eq!(w.poll(20_000, true, false), MicPrompt::Nothing);
        assert_eq!(w.poll(40_000, true, false), MicPrompt::Nothing);
    }

    #[test]
    fn in_use_entries_need_a_recent_start_and_no_stop() {
        let now = filetime_from_unix(1_800_000_000);
        let min = 60 * 10_000_000;
        assert!(entry_in_use(now - 5 * min, 0, now, MAX_USE_SECS));
        assert!(
            !entry_in_use(now - 5 * min, now - min, now, MAX_USE_SECS),
            "stopped"
        );
        assert!(!entry_in_use(0, 0, now, MAX_USE_SECS), "never used");
        assert!(
            !entry_in_use(now - 13 * 60 * min, 0, now, MAX_USE_SECS),
            "stale"
        );
    }

    #[test]
    fn fake_reports_what_it_holds() {
        let f = FakeMicUse::default();
        assert!(f.apps_in_use().is_empty());
        f.0.lock().unwrap().push("zoom.exe".into());
        assert_eq!(f.apps_in_use(), vec!["zoom.exe".to_string()]);
    }

    #[test]
    fn app_ids_drop_the_path_of_unpackaged_apps() {
        assert_eq!(
            app_id("C:#Users#jo#AppData#Local#Zoom#bin#Zoom.exe", false),
            "zoom.exe"
        );
        assert_eq!(
            app_id("Microsoft.Teams_8wekyb3d8bbwe", true),
            "microsoft.teams_8wekyb3d8bbwe"
        );
    }

    #[test]
    fn display_names_read_like_the_app() {
        assert_eq!(display_name("zoom.exe"), "Zoom");
        assert_eq!(
            display_name("microsoft.teams_8wekyb3d8bbwe"),
            "Microsoft Teams"
        );
        assert_eq!(display_name("discord.exe"), "Discord");
        assert_eq!(display_name("ms-teams.exe"), "Ms Teams");
        assert_eq!(display_name(""), "Another app");
    }

    #[test]
    fn skipped_apps_are_left_out() {
        let apps = vec!["zoom.exe".to_string(), "discord.exe".to_string()];
        assert_eq!(
            not_skipped(apps, &["Zoom.exe".to_string()]),
            vec!["discord.exe".to_string()]
        );
    }
}

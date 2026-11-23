//! Audio source detection logic (SPEC 18.2, 18.3): turns polled audio sessions into activity
//! events with resolved titles. Pure: the Windows pieces (session metering, media sessions,
//! window titles, browser tab strips) arrive through seams. Titles are PHI-grade and only ever
//! flow into transcript event lines (CLAUDE.md rule 11).
use crate::activity::{ActivityEvent, ActivityTracker};
use crate::{MediaSession, Window};
use std::collections::HashMap;

/// -50 dBFS as a linear peak.
pub const AUDIBLE_PEAK: f32 = 0.003_162;
/// Sound must persist this long to count as audible (SPEC 18.2).
pub const AUDIBLE_HOLD_MS: u64 = 300;

/// One app's audio session on a render endpoint, as the platform reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioSession {
    /// Main process of the owning app (browser helpers already walked up to the browser).
    pub owner_pid: u32,
    /// Executable name without extension, for example `firefox`.
    pub exe: String,
    pub peak: f32,
    pub muted: bool,
    /// SprkNotes itself and the Windows system-sounds session are marked ignorable.
    pub ignorable: bool,
}

pub trait AudioSessionSource: Send {
    fn sessions(&self) -> Vec<AudioSession>;
}

pub struct FakeSessions(pub Vec<AudioSession>);
impl AudioSessionSource for FakeSessions {
    fn sessions(&self) -> Vec<AudioSession> {
        self.0.clone()
    }
}

/// Browser tab strip lookup (UI Automation, read-only). Returns the tab title that carries
/// the audio indicator for this browser, if any.
pub trait TabTitles: Send {
    fn playing_tab(&self, exe: &str) -> Option<String>;
}

pub struct NoTabs;
impl TabTitles for NoTabs {
    fn playing_tab(&self, _exe: &str) -> Option<String> {
        None
    }
}

const BROWSER_SUFFIXES: &[&str] = &[
    " - Mozilla Firefox",
    " - Google Chrome",
    " - Microsoft Edge",
    " - Microsoft\u{200b} Edge",
    " \u{2013} Mozilla Firefox",
];

/// Remove the browser name the window title ends with.
pub fn strip_browser_suffix(title: &str) -> String {
    let t = title.trim();
    for s in BROWSER_SUFFIXES {
        if let Some(rest) = t.strip_suffix(s) {
            return rest.trim().to_string();
        }
    }
    t.to_string()
}

const CALL_APPS: &[&str] = &[
    "teams",
    "ms-teams",
    "msteams",
    "zoom",
    "webex",
    "phone link",
    "phonelink",
    "yourphone",
];

pub fn is_call_app(exe: &str) -> bool {
    let e = exe.to_ascii_lowercase();
    CALL_APPS.iter().any(|c| e == *c)
}

const BROWSERS: &[&str] = &[
    "firefox", "chrome", "msedge", "edge", "brave", "opera", "vivaldi", "iexplore", "arc",
];

pub fn is_browser(exe: &str) -> bool {
    let e = exe.to_ascii_lowercase();
    BROWSERS.iter().any(|b| e == *b)
}

/// Readable app name from an executable stem: `firefox` becomes `Firefox`.
pub fn display_name(exe: &str) -> String {
    let mut c = exe.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Title resolver chain, first hit wins (SPEC 18.3): media session, browser tab, the window
/// title of an app with exactly one titled window (never a browser), then the app name.
pub fn resolve_title(
    s: &AudioSession,
    media: &[MediaSession],
    tabs: &dyn TabTitles,
    windows: &[Window],
) -> String {
    let exe = s.exe.to_ascii_lowercase();
    if let Some(t) = media
        .iter()
        .filter(|m| m.playing)
        .find(|m| {
            m.app.to_ascii_lowercase().contains(&exe) || exe.contains(&m.app.to_ascii_lowercase())
        })
        .and_then(|m| m.title.clone())
        .filter(|t| !t.trim().is_empty())
    {
        return strip_browser_suffix(&t);
    }
    if let Some(t) = tabs.playing_tab(&s.exe).filter(|t| !t.trim().is_empty()) {
        return strip_browser_suffix(&t);
    }
    // A window title is only trusted when it cannot belong to something else. A browser's
    // window title is its focused tab, not the tab that is making sound, and an app with
    // several windows (Teams, an editor) gives no way to tell which one is playing. In
    // those cases the title is left out and only the app name is used.
    if !is_browser(&s.exe) {
        let mut own = windows
            .iter()
            .filter(|w| w.pid == s.owner_pid && !w.title.trim().is_empty());
        if let (Some(w), None) = (own.next(), own.next()) {
            return strip_browser_suffix(&w.title);
        }
    }
    display_name(&s.exe)
}

/// Polls sessions about every 250 ms and feeds the activity tracker.
pub struct SourcePoller {
    tracker: ActivityTracker,
    audible_since: HashMap<u32, u64>,
}

impl SourcePoller {
    pub fn new(tracker: ActivityTracker) -> Self {
        Self {
            tracker,
            audible_since: HashMap::new(),
        }
    }

    pub fn tracker(&self) -> &ActivityTracker {
        &self.tracker
    }

    pub fn poll(
        &mut self,
        now_ms: u64,
        sessions: &[AudioSession],
        media: &[MediaSession],
        tabs: &dyn TabTitles,
        windows: &[Window],
    ) -> Vec<ActivityEvent> {
        let mut events = Vec::new();
        let mut seen = Vec::new();
        for s in sessions.iter().filter(|s| !s.ignorable) {
            if s.muted || s.peak < AUDIBLE_PEAK {
                self.audible_since.remove(&s.owner_pid);
                continue;
            }
            seen.push(s.owner_pid);
            let since = *self.audible_since.entry(s.owner_pid).or_insert(now_ms);
            if now_ms.saturating_sub(since) >= AUDIBLE_HOLD_MS {
                let title = resolve_title(s, media, tabs, windows);
                events.extend(self.tracker.audible(
                    &display_name(&s.exe),
                    &title,
                    is_call_app(&s.exe),
                    now_ms,
                ));
            }
        }
        // Sessions that vanished are silent now.
        self.audible_since.retain(|pid, _| seen.contains(pid));
        events.extend(self.tracker.tick(now_ms));
        events
    }

    pub fn pause(&mut self, now_ms: u64) -> Vec<ActivityEvent> {
        self.audible_since.clear();
        self.tracker.pause(now_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::{Exclusions, Timing};

    fn sess(exe: &str, pid: u32, peak: f32) -> AudioSession {
        AudioSession {
            owner_pid: pid,
            exe: exe.into(),
            peak,
            muted: false,
            ignorable: false,
        }
    }

    fn poller() -> SourcePoller {
        SourcePoller::new(ActivityTracker::new(
            Timing::default(),
            Exclusions::default(),
        ))
    }

    #[test]
    fn strips_browser_suffixes() {
        assert_eq!(
            strip_browser_suffix("Group rules - YouTube - Mozilla Firefox"),
            "Group rules - YouTube"
        );
        assert_eq!(strip_browser_suffix("Plain title"), "Plain title");
    }

    #[test]
    fn title_chain_prefers_media_then_tab_then_window_then_app() {
        let s = sess("firefox", 10, 0.2);
        let media = vec![MediaSession {
            app: "Firefox".into(),
            title: Some("Media title - Mozilla Firefox".into()),
            playing: true,
        }];
        let win = vec![Window {
            pid: 10,
            title: "Window title - Mozilla Firefox".into(),
        }];
        struct Tab;
        impl TabTitles for Tab {
            fn playing_tab(&self, _: &str) -> Option<String> {
                Some("Tab title".into())
            }
        }
        assert_eq!(resolve_title(&s, &media, &Tab, &win), "Media title");
        assert_eq!(resolve_title(&s, &[], &Tab, &win), "Tab title");
        // A browser's window title is its focused tab, so it is never used.
        assert_eq!(resolve_title(&s, &[], &NoTabs, &win), "Firefox");
        assert_eq!(resolve_title(&s, &[], &NoTabs, &[]), "Firefox");
        // A paused media session is not a hit.
        let paused = vec![MediaSession {
            app: "firefox".into(),
            title: Some("Old".into()),
            playing: false,
        }];
        assert_eq!(resolve_title(&s, &paused, &NoTabs, &win), "Firefox");
    }

    #[test]
    fn window_title_is_used_only_for_a_single_window_non_browser_app() {
        let s = sess("vlc", 20, 0.2);
        let one = vec![
            Window {
                pid: 20,
                title: "Lecture 3 - VLC media player".into(),
            },
            Window {
                pid: 99,
                title: "Other app".into(),
            },
        ];
        assert_eq!(
            resolve_title(&s, &[], &NoTabs, &one),
            "Lecture 3 - VLC media player"
        );
        let two = vec![
            Window {
                pid: 20,
                title: "Lecture 3".into(),
            },
            Window {
                pid: 20,
                title: "Settings".into(),
            },
        ];
        assert_eq!(resolve_title(&s, &[], &NoTabs, &two), "Vlc");
    }

    #[test]
    fn a_browser_title_does_not_follow_the_focused_tab() {
        // The user switches tabs while one tab keeps playing: one activity, no title flapping.
        let mut p = poller();
        let s = [sess("firefox", 10, 0.2)];
        let mut ev = Vec::new();
        for t in (0..=20_000).step_by(250) {
            let title = if t < 10_000 {
                "A - Mozilla Firefox"
            } else {
                "B - Mozilla Firefox"
            };
            let w = [Window {
                pid: 10,
                title: title.into(),
            }];
            ev.extend(p.poll(t, &s, &[], &NoTabs, &w));
        }
        let started = ev
            .iter()
            .filter(|e| e.text().starts_with("Audio source started"))
            .count();
        assert_eq!(started, 1, "{ev:?}");
    }

    #[test]
    fn call_apps_are_recognized() {
        assert!(is_call_app("Teams"));
        assert!(is_call_app("zoom"));
        assert!(!is_call_app("firefox"));
    }

    #[test]
    fn sound_must_persist_300ms_and_chimes_below_minimum_never_announce() {
        let mut p = poller();
        let s = [sess("firefox", 10, 0.2)];
        assert!(p.poll(0, &s, &[], &NoTabs, &[]).is_empty());
        // Not yet 300 ms of sound: nothing reaches the tracker.
        assert!(p.tracker().records().is_empty());
        let mut ev = Vec::new();
        for t in (250..=4_000).step_by(250) {
            ev.extend(p.poll(t, &s, &[], &NoTabs, &[]));
        }
        // After the 3 s minimum an event is announced with the app name.
        assert!(ev.iter().any(|e| e.text().contains("Firefox")), "{ev:?}");
    }

    #[test]
    fn quiet_muted_and_ignorable_sessions_are_skipped() {
        let mut p = poller();
        let mut muted = sess("spotify", 1, 0.5);
        muted.muted = true;
        let quiet = sess("vlc", 2, 0.0001);
        let mut own = sess("sprknotes", 3, 0.5);
        own.ignorable = true;
        let all = [muted, quiet, own];
        let mut ev = Vec::new();
        for t in (0..=6_000).step_by(250) {
            ev.extend(p.poll(t, &all, &[], &NoTabs, &[]));
        }
        assert!(ev.is_empty());
    }

    #[test]
    fn exclusions_apply_before_anything_is_stored() {
        let tr = ActivityTracker::new(Timing::default(), Exclusions(vec!["spotify".into()]));
        let mut p = SourcePoller::new(tr);
        let s = [sess("spotify", 5, 0.5)];
        let mut ev = Vec::new();
        for t in (0..=6_000).step_by(250) {
            ev.extend(p.poll(t, &s, &[], &NoTabs, &[]));
        }
        assert!(ev.is_empty());
        assert!(p.tracker().records().is_empty());
    }
}

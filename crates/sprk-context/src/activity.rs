//! Audio source activity tracking and timing rules (SPEC 18.4). Pure: the platform layer
//! feeds already-thresholded "audible" samples and the session clock.
//! Titles are PHI-grade: they appear only in events destined for the transcript, never in logs.
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Timing {
    pub merge_general_ms: u64,
    pub merge_call_ms: u64,
    pub min_activity_ms: u64,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            merge_general_ms: 30_000,
            merge_call_ms: 120_000,
            min_activity_ms: 3_000,
        }
    }
}

impl Timing {
    /// Clamp to the ranges in SPEC 18.4.
    pub fn clamped(self) -> Self {
        Self {
            merge_general_ms: self.merge_general_ms.clamp(10_000, 300_000),
            merge_call_ms: self.merge_call_ms.clamp(30_000, 600_000),
            min_activity_ms: self.min_activity_ms.clamp(1_000, 30_000),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityEvent {
    Started {
        app: String,
        title: String,
        at_ms: u64,
    },
    Ended {
        app: String,
        title: String,
        end_ms: u64,
        duration_ms: u64,
    },
}

impl ActivityEvent {
    pub fn at_ms(&self) -> u64 {
        match self {
            ActivityEvent::Started { at_ms, .. } => *at_ms,
            ActivityEvent::Ended { end_ms, .. } => *end_ms,
        }
    }

    /// Event line text for the transcript (SPEC 5.1).
    pub fn text(&self) -> String {
        match self {
            ActivityEvent::Started { app, title, .. } => {
                format!("Audio source started: {app} \"{title}\"")
            }
            ActivityEvent::Ended {
                app,
                title,
                duration_ms,
                ..
            } => {
                format!(
                    "Audio source ended: {app} \"{title}\" ({})",
                    short_duration(*duration_ms)
                )
            }
        }
    }
}

/// `M:SS`, or `H:MM:SS` from an hour up.
pub fn short_duration(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// A finished, announced activity for the finalize table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityRecord {
    pub app: String,
    pub title: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl ActivityRecord {
    pub fn duration_ms(&self) -> u64 {
        self.end_ms - self.start_ms
    }
}

struct Open {
    title: String,
    start_ms: u64,
    last_audible_ms: u64,
    announced: bool,
    call_app: bool,
}

/// Exclusion patterns: process names or title substrings, case-insensitive.
/// Excluded activities are omitted entirely, not masked (SPEC 18.6).
#[derive(Debug, Clone, Default)]
pub struct Exclusions(pub Vec<String>);

impl Exclusions {
    pub fn matches(&self, app: &str, title: &str) -> bool {
        let (a, t) = (app.to_lowercase(), title.to_lowercase());
        self.0.iter().filter(|p| !p.is_empty()).any(|p| {
            let p = p.to_lowercase();
            a.contains(&p) || t.contains(&p)
        })
    }
}

pub struct ActivityTracker {
    timing: Timing,
    exclusions: Exclusions,
    open: HashMap<String, Open>,
    done: Vec<ActivityRecord>,
}

impl ActivityTracker {
    pub fn new(timing: Timing, exclusions: Exclusions) -> Self {
        Self {
            timing: timing.clamped(),
            exclusions,
            open: HashMap::new(),
            done: Vec::new(),
        }
    }

    /// Finished and announced activities so far, for the Audio sources table.
    pub fn records(&self) -> &[ActivityRecord] {
        &self.done
    }

    /// One poll for one app (`audible` already passed the -50 dBFS / 300 ms test).
    /// Call it for audible apps; silence is handled by `tick`.
    pub fn audible(
        &mut self,
        app: &str,
        title: &str,
        is_call_app: bool,
        now_ms: u64,
    ) -> Vec<ActivityEvent> {
        if self.exclusions.matches(app, title) {
            return Vec::new();
        }
        let mut events = Vec::new();
        let min = self.timing.min_activity_ms;
        let mut finished = None;
        if let Some(o) = self.open.get_mut(app) {
            if o.title == title {
                o.last_audible_ms = now_ms;
                if !o.announced && now_ms - o.start_ms >= min {
                    o.announced = true;
                    events.push(ActivityEvent::Started {
                        app: app.into(),
                        title: title.into(),
                        at_ms: o.start_ms,
                    });
                }
                return events;
            }
            // Title change: end the current activity at its last audible moment.
            finished = self.open.remove(app);
        }
        if let Some(o) = finished {
            self.close(app, o, &mut events);
        }
        self.open.insert(
            app.to_string(),
            Open {
                title: title.into(),
                start_ms: now_ms,
                last_audible_ms: now_ms,
                announced: false,
                call_app: is_call_app,
            },
        );
        events
    }

    /// Advance the clock: activities whose silence reached the merge window end at their
    /// last audible moment (not when the window expired).
    pub fn tick(&mut self, now_ms: u64) -> Vec<ActivityEvent> {
        let (g, c) = (self.timing.merge_general_ms, self.timing.merge_call_ms);
        let expired: Vec<String> = self
            .open
            .iter()
            .filter(|(_, o)| {
                now_ms.saturating_sub(o.last_audible_ms) >= if o.call_app { c } else { g }
            })
            .map(|(k, _)| k.clone())
            .collect();
        let mut events = Vec::new();
        for app in expired {
            if let Some(o) = self.open.remove(&app) {
                self.close(&app, o, &mut events);
            }
        }
        events.sort_by_key(|e| e.at_ms());
        events
    }

    /// Pause (or end): close open activities at the pause moment (SPEC 18.2).
    pub fn pause(&mut self, now_ms: u64) -> Vec<ActivityEvent> {
        let apps: Vec<String> = self.open.keys().cloned().collect();
        let mut events = Vec::new();
        for app in apps {
            if let Some(mut o) = self.open.remove(&app) {
                o.last_audible_ms = now_ms.max(o.last_audible_ms);
                self.close(&app, o, &mut events);
            }
        }
        events.sort_by_key(|e| e.at_ms());
        events
    }

    fn close(&mut self, app: &str, o: Open, events: &mut Vec<ActivityEvent>) {
        let duration = o.last_audible_ms - o.start_ms;
        if duration < self.timing.min_activity_ms {
            return; // a notification chime: never announced, never recorded
        }
        if !o.announced {
            events.push(ActivityEvent::Started {
                app: app.into(),
                title: o.title.clone(),
                at_ms: o.start_ms,
            });
        }
        events.push(ActivityEvent::Ended {
            app: app.into(),
            title: o.title.clone(),
            end_ms: o.last_audible_ms,
            duration_ms: duration,
        });
        self.done.push(ActivityRecord {
            app: app.into(),
            title: o.title,
            start_ms: o.start_ms,
            end_ms: o.last_audible_ms,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker() -> ActivityTracker {
        ActivityTracker::new(Timing::default(), Exclusions::default())
    }

    /// Drive `audible` once a second from `from` to `to` (ms) inclusive.
    fn play(
        t: &mut ActivityTracker,
        app: &str,
        title: &str,
        from: u64,
        to: u64,
    ) -> Vec<ActivityEvent> {
        let mut ev = Vec::new();
        let mut now = from;
        while now <= to {
            ev.extend(t.audible(app, title, false, now));
            now += 1_000;
        }
        ev
    }

    #[test]
    fn short_gap_is_same_activity_and_counts_toward_duration() {
        let mut t = tracker();
        play(&mut t, "Firefox", "Video", 0, 90_000);
        assert!(
            t.tick(110_000).is_empty(),
            "20 s silence is inside the 30 s window"
        );
        play(&mut t, "Firefox", "Video", 110_000, 150_000);
        let ev = t.tick(200_000);
        assert_eq!(
            ev,
            vec![ActivityEvent::Ended {
                app: "Firefox".into(),
                title: "Video".into(),
                end_ms: 150_000,
                duration_ms: 150_000
            }]
        );
    }

    #[test]
    fn long_silence_ends_at_last_audible_moment() {
        let mut t = tracker();
        play(&mut t, "Firefox", "Video", 0, 90_000);
        assert!(t.tick(119_999).is_empty());
        let ev = t.tick(120_000);
        match &ev[0] {
            ActivityEvent::Ended {
                end_ms,
                duration_ms,
                ..
            } => {
                assert_eq!(*end_ms, 90_000);
                assert_eq!(*duration_ms, 90_000);
                assert_eq!(short_duration(*duration_ms), "1:30");
            }
            e => panic!("unexpected {e:?}"),
        }
    }

    #[test]
    fn title_change_starts_a_new_activity() {
        let mut t = tracker();
        play(&mut t, "Firefox", "Video A", 0, 20_000);
        let ev = t.audible("Firefox", "Video B", false, 21_000);
        assert!(matches!(
            ev.last(),
            Some(ActivityEvent::Ended { end_ms: 20_000, .. })
        ));
        play(&mut t, "Firefox", "Video B", 22_000, 40_000);
        let ev = t.pause(41_000);
        assert!(ev
            .iter()
            .any(|e| matches!(e, ActivityEvent::Ended { title, .. } if title == "Video B")));
        assert_eq!(t.records().len(), 2);
    }

    #[test]
    fn chimes_below_minimum_are_ignored() {
        let mut t = tracker();
        let ev = t.audible("Outlook", "New mail", false, 5_000);
        assert!(ev.is_empty());
        t.audible("Outlook", "New mail", false, 6_000);
        assert!(t.tick(60_000).is_empty());
        assert!(t.records().is_empty());
    }

    #[test]
    fn started_event_is_stamped_at_real_start() {
        let mut t = tracker();
        let ev = play(&mut t, "Firefox", "Video", 10_000, 14_000);
        assert_eq!(
            ev,
            vec![ActivityEvent::Started {
                app: "Firefox".into(),
                title: "Video".into(),
                at_ms: 10_000
            }]
        );
    }

    #[test]
    fn call_apps_use_the_longer_merge_window() {
        let mut t = tracker();
        for s in 0..=10 {
            t.audible("Microsoft Teams", "Weekly IT Sync", true, s * 1_000);
        }
        assert!(
            t.tick(100_000).is_empty(),
            "90 s of silence is inside the 120 s call window"
        );
        let ev = t.tick(130_000);
        assert!(matches!(
            ev.last(),
            Some(ActivityEvent::Ended { end_ms: 10_000, .. })
        ));
    }

    #[test]
    fn apps_are_tracked_separately_and_exclusions_are_omitted() {
        let mut t = ActivityTracker::new(
            Timing::default(),
            Exclusions(vec!["EHR".into(), "patient".into()]),
        );
        play(&mut t, "Firefox", "Video", 0, 10_000);
        play(&mut t, "Spotify", "Song", 0, 10_000);
        let ev = play(&mut t, "Chrome", "Patient chart", 0, 10_000);
        assert!(ev.is_empty());
        let ev = play(&mut t, "EHR Client", "Home", 0, 10_000);
        assert!(ev.is_empty());
        let ended = t.pause(11_000);
        assert_eq!(ended.len(), 2, "only Firefox and Spotify");
        assert_eq!(t.records().len(), 2);
    }

    #[test]
    fn event_text_matches_the_spec() {
        let s = ActivityEvent::Started {
            app: "Firefox".into(),
            title: "Group rules overview - YouTube".into(),
            at_ms: 0,
        };
        assert_eq!(
            s.text(),
            "Audio source started: Firefox \"Group rules overview - YouTube\""
        );
        let e = ActivityEvent::Ended {
            app: "Firefox".into(),
            title: "Group rules overview - YouTube".into(),
            end_ms: 0,
            duration_ms: 210_000,
        };
        assert_eq!(
            e.text(),
            "Audio source ended: Firefox \"Group rules overview - YouTube\" (3:30)"
        );
    }

    #[test]
    fn timing_is_clamped() {
        let t = Timing {
            merge_general_ms: 1,
            merge_call_ms: 9_999_999,
            min_activity_ms: 0,
        }
        .clamped();
        assert_eq!(
            (t.merge_general_ms, t.merge_call_ms, t.min_activity_ms),
            (10_000, 600_000, 1_000)
        );
    }
}

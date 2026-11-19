//! Participants, group periods and audio sources, rebuilt from the event lines already in the
//! file (SPEC 5.1, 6.3). Written at finalize, so a crash loses none of this data.
use crate::format::{timestamp, yaml_str};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    pub name: String,
    pub joined: String,
    pub left: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Period {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioSourceRow {
    pub app: String,
    pub title: String,
    pub start: String,
    pub end: String,
    pub duration: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extras {
    pub participants: Vec<Participant>,
    pub group_periods: Vec<Period>,
    pub audio_sources: Vec<AudioSourceRow>,
}

fn stamp_to_ms(s: &str) -> Option<u64> {
    let mut p = s.split(':').map(|x| x.parse::<u64>().ok());
    Some((p.next()?? * 3600 + p.next()?? * 60 + p.next()??) * 1000)
}

/// `> \`HH:MM:SS\` text` into (stamp, text).
fn event_parts(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("> `")?;
    let (stamp, text) = rest.split_once("` ")?;
    Some((stamp, text))
}

fn last_stamp(body: &str) -> String {
    let mut best = 0u64;
    for l in body.lines() {
        let rest = l.strip_prefix("- `").or_else(|| l.strip_prefix("> `"));
        if let Some(r) = rest {
            if let Some((st, _)) = r.split_once('`') {
                if let Some(ms) = stamp_to_ms(st) {
                    best = best.max(ms);
                }
            }
        }
    }
    timestamp(best)
}

/// `Firefox "Group rules overview - YouTube"` into (app, title).
fn app_title(s: &str) -> Option<(String, String)> {
    let (app, rest) = s.split_once(" \"")?;
    let end = rest.rfind('"')?;
    Some((app.to_string(), rest[..end].to_string()))
}

impl Extras {
    /// Rebuild from the transcript body (everything after the front matter).
    pub fn from_events(body: &str) -> Self {
        let mut x = Extras::default();
        let session_end = last_stamp(body);
        let mut open_group: Option<String> = None;
        let mut open_src: Vec<(String, String, String)> = Vec::new(); // app, title, start
        for line in body.lines() {
            let Some((st, text)) = event_parts(line) else {
                continue;
            };
            if let Some(rest) = text.strip_prefix("Audio source started: ") {
                if let Some((app, title)) = app_title(rest) {
                    open_src.push((app, title, st.to_string()));
                }
            } else if let Some(rest) = text.strip_prefix("Audio source ended: ") {
                // `App "Title" (3:30)`
                let rest = rest.rfind(" (").map_or(rest, |i| &rest[..i]);
                let dur = text
                    .rfind(" (")
                    .map(|i| text[i + 2..].trim_end_matches(')').to_string());
                if let Some((app, title)) = app_title(rest) {
                    if let Some(pos) = open_src
                        .iter()
                        .position(|(a, t, _)| *a == app && *t == title)
                    {
                        let (a, t, start) = open_src.remove(pos);
                        x.audio_sources.push(AudioSourceRow {
                            app: a,
                            title: t,
                            start,
                            end: st.to_string(),
                            duration: dur.unwrap_or_default(),
                        });
                    }
                }
            } else if text.starts_with("Call became a group call") {
                open_group = Some(st.to_string());
            } else if let Some(rest) = text.strip_prefix("Back to 1:1 with ") {
                if let Some(s) = open_group.take() {
                    x.group_periods.push(Period {
                        start: s,
                        end: st.to_string(),
                    });
                }
                x.note_participant(rest.split(':').next().unwrap_or(""), st);
            } else if let Some(rest) = text.strip_prefix("Call with ") {
                x.note_participant(rest.split(':').next().unwrap_or(""), st);
            }
        }
        if let Some(s) = open_group {
            x.group_periods.push(Period {
                start: s,
                end: session_end.clone(),
            });
        }
        // Activities still open at a crash end at the last stamp.
        for (app, title, start) in open_src {
            let dur = stamp_to_ms(&session_end)
                .unwrap_or(0)
                .saturating_sub(stamp_to_ms(&start).unwrap_or(0))
                / 1000;
            x.audio_sources.push(AudioSourceRow {
                app,
                title,
                start,
                end: session_end.clone(),
                duration: format!("{}:{:02}", dur / 60, dur % 60),
            });
        }
        for p in &mut x.participants {
            p.left = session_end.clone();
        }
        x.audio_sources.sort_by(|a, b| a.start.cmp(&b.start));
        x
    }

    fn note_participant(&mut self, name: &str, at: &str) {
        let name = name.trim();
        if name.is_empty() || self.participants.iter().any(|p| p.name == name) {
            return;
        }
        self.participants.push(Participant {
            name: name.into(),
            joined: at.into(),
            left: String::new(),
        });
    }

    pub fn is_empty(&self) -> bool {
        self.participants.is_empty()
            && self.group_periods.is_empty()
            && self.audio_sources.is_empty()
    }

    /// YAML blocks for the front matter (`call:` and `audio_sources:`).
    pub fn front_matter_blocks(&self) -> String {
        let mut s = String::new();
        if !self.participants.is_empty() || !self.group_periods.is_empty() {
            s.push_str("call:\n  provider: teams\n");
            if !self.participants.is_empty() {
                s.push_str("  participants:\n");
                for p in &self.participants {
                    s.push_str(&format!(
                        "    - {{ name: {}, joined: {}, left: {} }}\n",
                        yaml_str(&p.name),
                        yaml_str(&p.joined),
                        yaml_str(&p.left)
                    ));
                }
            }
            if !self.group_periods.is_empty() {
                s.push_str("  group_periods:\n");
                for g in &self.group_periods {
                    s.push_str(&format!(
                        "    - {{ start: {}, end: {} }}\n",
                        yaml_str(&g.start),
                        yaml_str(&g.end)
                    ));
                }
            }
        }
        if !self.audio_sources.is_empty() {
            s.push_str("audio_sources:\n");
            for a in &self.audio_sources {
                s.push_str(&format!(
                    "  - {{ app: {}, title: {}, start: {}, end: {}, duration: {} }}\n",
                    yaml_str(&a.app),
                    yaml_str(&a.title),
                    yaml_str(&a.start),
                    yaml_str(&a.end),
                    yaml_str(&a.duration)
                ));
            }
        }
        s
    }

    /// The `## Participants` and `## Audio sources` sections (empty string when there is nothing).
    pub fn sections(&self) -> String {
        let esc = |s: &str| s.replace('|', "\\|");
        let mut out = String::new();
        if !self.participants.is_empty() {
            out.push_str("## Participants\n| Name | Joined | Left |\n|---|---|---|\n");
            for p in &self.participants {
                out.push_str(&format!(
                    "| {} | {} | {} |\n",
                    esc(&p.name),
                    p.joined,
                    p.left
                ));
            }
            for g in &self.group_periods {
                out.push_str(&format!(
                    "\n**Group call:** {} to {} (additional participants not identified)\n",
                    g.start, g.end
                ));
            }
            out.push('\n');
        }
        if !self.audio_sources.is_empty() {
            out.push_str("## Audio sources\n| App | Title | Start | End | Duration |\n|---|---|---|---|---|\n");
            for a in &self.audio_sources {
                out.push_str(&format!(
                    "| {} | {} | {} | {} | {} |\n",
                    esc(&a.app),
                    esc(&a.title),
                    a.start,
                    a.end,
                    a.duration
                ));
            }
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = "# T\n\n## Transcript\n\n- `00:00:04` **Sam Sample:** hi\n\n> `00:20:05` Call became a group call: remote speakers labeled Others\n> `00:22:10` Audio source started: Firefox \"Group rules overview - YouTube\"\n> `00:25:40` Audio source ended: Firefox \"Group rules overview - YouTube\" (3:30)\n> `00:31:40` Back to 1:1 with Sam Sample: remote speaker labeled Sam Sample\n- `00:42:11` **Me:** bye\n";

    #[test]
    fn rebuilds_from_events() {
        let x = Extras::from_events(&format!(
            "> `00:00:00` Call with Sam Sample: remote speaker labeled Sam Sample\n{BODY}"
        ));
        assert_eq!(x.participants.len(), 1);
        assert_eq!(x.participants[0].name, "Sam Sample");
        assert_eq!(x.participants[0].joined, "00:00:00");
        assert_eq!(x.participants[0].left, "00:42:11");
        assert_eq!(
            x.group_periods,
            vec![Period {
                start: "00:20:05".into(),
                end: "00:31:40".into()
            }]
        );
        assert_eq!(x.audio_sources.len(), 1);
        assert_eq!(x.audio_sources[0].duration, "3:30");
        assert_eq!(x.audio_sources[0].title, "Group rules overview - YouTube");
    }

    #[test]
    fn crash_leaves_open_activities_and_group_periods_closed_at_last_stamp() {
        let body = "> `00:01:00` Audio source started: Spotify \"Song\"\n> `00:02:00` Call became a group call: remote speakers labeled Others\n- `00:05:00` **Me:** x\n";
        let x = Extras::from_events(body);
        assert_eq!(x.audio_sources[0].end, "00:05:00");
        assert_eq!(x.audio_sources[0].duration, "4:00");
        assert_eq!(x.group_periods[0].end, "00:05:00");
    }

    #[test]
    fn renders_blocks_and_sections() {
        let x = Extras::from_events(BODY);
        let fm = x.front_matter_blocks();
        assert!(fm.contains("audio_sources:\n  - { app: \"Firefox\""));
        assert!(fm.contains("group_periods:"));
        let sec = x.sections();
        assert!(sec.contains("## Audio sources\n| App | Title | Start | End | Duration |"));
        assert!(sec
            .contains("| Firefox | Group rules overview - YouTube | 00:22:10 | 00:25:40 | 3:30 |"));
        assert!(Extras::default().sections().is_empty());
    }

    #[test]
    fn pipes_in_titles_are_escaped() {
        let x = Extras {
            audio_sources: vec![AudioSourceRow {
                app: "Teams".into(),
                title: "Sync | Microsoft Teams".into(),
                start: "00:00:02".into(),
                end: "00:01:00".into(),
                duration: "0:58".into(),
            }],
            ..Extras::default()
        };
        assert!(x.sections().contains("Sync \\| Microsoft Teams"));
    }
}

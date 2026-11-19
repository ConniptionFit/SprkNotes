//! Reads a `sprknotes-transcript/1` Markdown file back into a model (SPEC 5.1 grammar),
//! optionally merged with the JSONL sidecar for exact times and sources (SPEC 5.2).
use crate::extras::{AudioSourceRow, Extras};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub id: u64,
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: String,
    pub source: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub t_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    pub format: String,
    pub title: String,
    pub date: String,
    pub start: String,
    pub end: String,
    pub duration: String,
    pub status: String,
    pub sources: Vec<String>,
    pub speakers: Vec<String>,
    pub name_source: String,
    pub engine: String,
    pub app: String,
    /// Raw Markdown of the optional `## Meeting context` section (empty when absent).
    pub meeting_context: String,
    pub segments: Vec<Segment>,
    pub events: Vec<Event>,
    pub extras: Extras,
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    match v.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        Some(inner) => {
            let mut out = String::new();
            let mut esc = false;
            for c in inner.chars() {
                if esc {
                    out.push(c);
                    esc = false;
                } else if c == '\\' {
                    esc = true;
                } else {
                    out.push(c);
                }
            }
            out
        }
        None => v.to_string(),
    }
}

/// `[a, "b c"]` into items (no commas inside quoted items are expected in practice,
/// but quotes are honored).
fn list(v: &str) -> Vec<String> {
    let inner = v.trim().trim_start_matches('[').trim_end_matches(']');
    let mut items = Vec::new();
    let mut cur = String::new();
    let (mut in_q, mut esc) = (false, false);
    for c in inner.chars() {
        if esc {
            cur.push(c);
            esc = false;
        } else if c == '\\' && in_q {
            cur.push(c);
            esc = true;
        } else if c == '"' {
            in_q = !in_q;
            cur.push(c);
        } else if c == ',' && !in_q {
            items.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    items.push(cur);
    items
        .into_iter()
        .map(|s| unquote(&s))
        .filter(|s| !s.is_empty())
        .collect()
}

fn stamp_ms(s: &str) -> Option<u64> {
    let mut p = s.split(':').map(|x| x.parse::<u64>().ok());
    Some((p.next()?? * 3600 + p.next()?? * 60 + p.next()??) * 1000)
}

fn sidecar_segments(sc: &str) -> (Vec<Segment>, Vec<Event>) {
    let (mut segs, mut evs) = (Vec::new(), Vec::new());
    for line in sc.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue; // a torn last line after a crash is ignored
        };
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let n = |k: &str| v.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
        if v.get("event").is_some() {
            evs.push(Event {
                t_ms: n("t_ms"),
                text: s("type"),
            });
        } else {
            segs.push(Segment {
                id: n("id"),
                start_ms: n("start_ms"),
                end_ms: n("end_ms"),
                speaker: s("speaker"),
                source: s("source"),
                text: s("text"),
            });
        }
    }
    (segs, evs)
}

impl Parsed {
    /// Parse the Markdown; when a sidecar is given, its segments supply exact end times and
    /// sources (matched by order, only when counts agree so a mismatch never mislabels).
    pub fn parse(md: &str, sidecar: Option<&str>) -> Parsed {
        let mut p = Parsed::default();
        let mut lines = md.lines().peekable();
        if lines.peek().is_some_and(|l| l.trim() == "---") {
            lines.next();
            for l in lines.by_ref() {
                if l.trim() == "---" {
                    break;
                }
                if l.starts_with(' ') || l.starts_with('-') {
                    continue; // nested blocks are rebuilt from event lines
                }
                let Some((k, v)) = l.split_once(':') else {
                    continue;
                };
                let v = v.trim();
                match k {
                    "format" => p.format = v.to_string(),
                    "title" => p.title = unquote(v),
                    "date" => p.date = unquote(v),
                    "start" => p.start = unquote(v),
                    "end" => p.end = unquote(v),
                    "duration" => p.duration = unquote(v),
                    "status" => p.status = v.to_string(),
                    "sources" => p.sources = list(v),
                    "speakers" => p.speakers = list(v),
                    "name_source" => p.name_source = v.to_string(),
                    "engine" => p.engine = unquote(v),
                    "app" => p.app = unquote(v),
                    _ => {}
                }
            }
        }
        let body: String = lines.collect::<Vec<_>>().join("\n");
        let mut in_context = false;
        let mut in_transcript = false;
        let mut id = 0u64;
        for l in body.lines() {
            if let Some(h) = l.strip_prefix("## ") {
                in_context = h.trim() == "Meeting context";
                in_transcript = h.trim() == "Transcript";
                continue;
            }
            if in_context {
                p.meeting_context.push_str(l);
                p.meeting_context.push('\n');
                continue;
            }
            if !in_transcript {
                continue;
            }
            if let Some(rest) = l.strip_prefix("- `") {
                if let Some((st, after)) = rest.split_once("` **") {
                    if let (Some(ms), Some((sp, text))) = (stamp_ms(st), after.split_once(":** ")) {
                        p.segments.push(Segment {
                            id,
                            start_ms: ms,
                            end_ms: ms,
                            speaker: sp.to_string(),
                            source: String::new(),
                            text: text.to_string(),
                        });
                        id += 1;
                    }
                }
            } else if let Some(rest) = l.strip_prefix("> `") {
                if let Some((st, text)) = rest.split_once("` ") {
                    if let Some(ms) = stamp_ms(st) {
                        p.events.push(Event {
                            t_ms: ms,
                            text: text.to_string(),
                        });
                    }
                }
            }
        }
        p.meeting_context = p.meeting_context.trim().to_string();
        p.extras = Extras::from_events(&body);
        if let Some(sc) = sidecar {
            let (segs, evs) = sidecar_segments(sc);
            if segs.len() == p.segments.len() {
                for (mine, theirs) in p.segments.iter_mut().zip(segs) {
                    mine.start_ms = theirs.start_ms;
                    mine.end_ms = theirs.end_ms;
                    mine.source = theirs.source;
                }
            }
            if evs.len() == p.events.len() {
                for (mine, theirs) in p.events.iter_mut().zip(evs) {
                    mine.t_ms = theirs.t_ms;
                }
            }
        }
        // Without a sidecar the best end estimate is the next statement's start.
        if sidecar.is_none() {
            let starts: Vec<u64> = p.segments.iter().map(|s| s.start_ms).collect();
            for (i, s) in p.segments.iter_mut().enumerate() {
                if let Some(n) = starts.get(i + 1) {
                    s.end_ms = (*n).max(s.start_ms);
                }
            }
        }
        p
    }
}

impl AudioSourceRow {
    pub fn label(&self) -> String {
        format!("{} \"{}\"", self.app, self.title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const MD: &str = "---\nformat: sprknotes-transcript/1\ntitle: \"Sync with \\\"Sam\\\"\"\ndate: 2026-11-19\nstart: \"09:00\"\nend: \"09:30\"\nduration: \"0:30:00\"\nstatus: complete\nsources: [mic, system]\nspeakers: [\"Me\", \"Others\"]\nname_source: calendar\nengine: \"parakeet\"\napp: \"0.0.1\"\n---\n# T\n\n## Meeting context\nAgenda: budget\n\n## Transcript\n\n- `00:00:04` **Me:** hello there\n\n- `00:00:09` **Others:** hi: how are you\n\n> `00:00:20` Transcription paused\n\n- `00:00:30` **Me:** bye\n";

    #[test]
    fn parses_front_matter_segments_and_events() {
        let p = Parsed::parse(MD, None);
        assert_eq!(p.title, "Sync with \"Sam\"");
        assert_eq!(p.sources, vec!["mic", "system"]);
        assert_eq!(p.speakers, vec!["Me", "Others"]);
        assert_eq!(p.segments.len(), 3);
        assert_eq!(p.segments[1].speaker, "Others");
        assert_eq!(p.segments[1].text, "hi: how are you");
        assert_eq!(p.segments[0].end_ms, 9_000);
        assert_eq!(p.events.len(), 1);
        assert_eq!(p.events[0].t_ms, 20_000);
        assert_eq!(p.meeting_context, "Agenda: budget");
    }

    #[test]
    fn sidecar_supplies_exact_times_and_sources() {
        let sc = "{\"id\":0,\"start_ms\":4000,\"end_ms\":6500,\"speaker\":\"Me\",\"source\":\"mic\",\"text\":\"hello there\"}\n{\"id\":1,\"start_ms\":9000,\"end_ms\":12000,\"speaker\":\"Others\",\"source\":\"system\",\"text\":\"x\"}\n{\"event\":true,\"t_ms\":20000,\"type\":\"Transcription paused\"}\n{\"id\":2,\"start_ms\":30000,\"end_ms\":31000,\"speaker\":\"Me\",\"source\":\"mic\",\"text\":\"bye\"}\n{\"id\":3,\"sta";
        let p = Parsed::parse(MD, Some(sc));
        assert_eq!(p.segments[0].end_ms, 6_500);
        assert_eq!(p.segments[1].source, "system");
        assert_eq!(p.segments[2].end_ms, 31_000);
    }

    #[test]
    fn mismatched_sidecar_is_ignored() {
        let sc = "{\"id\":0,\"start_ms\":1,\"end_ms\":2,\"speaker\":\"Me\",\"source\":\"mic\",\"text\":\"a\"}\n";
        let p = Parsed::parse(MD, Some(sc));
        assert_eq!(p.segments[0].start_ms, 4_000);
        assert_eq!(p.segments[0].source, "");
    }
}

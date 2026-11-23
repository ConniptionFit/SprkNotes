//! The transcript "view" the model sees (ADR-009 grounding). Each line carries a stable id and a
//! timestamp so answers can cite lines and a verifier can check the citations.
//! This module only formats text it is given; it never logs and never keeps it.

/// One committed transcript line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Stable within a session. Matches the transcript's segment id.
    pub id: u64,
    pub t_ms: u64,
    /// `Me`, `Others`, or a name from the confirmed 1:1 label (never guessed).
    pub speaker: String,
    pub text: String,
}

/// A pause, a join, an audio source window: the `>` lines of the transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub t_ms: u64,
    pub text: String,
}

/// `HH:MM:SS`, matching the transcript format.
pub fn hms(t_ms: u64) -> String {
    let s = t_ms / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

/// Cheap, deliberately pessimistic token estimate (3 characters per token) so budgets never
/// run over. The real tokenizer lives in the engine.
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(3)
}

/// Which end of the window survives when it does not fit the budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// The start of the meeting (titles).
    Head,
    /// The most recent lines ("what did I miss").
    Tail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub text: String,
    /// Ids of the lines that made it into `text`, in order.
    pub line_ids: Vec<u64>,
    /// True when lines were dropped to fit the budget.
    pub truncated: bool,
}

enum Item<'a> {
    Line(&'a Line),
    Note(&'a Note),
}

impl Item<'_> {
    fn t(&self) -> u64 {
        match self {
            Item::Line(l) => l.t_ms,
            Item::Note(n) => n.t_ms,
        }
    }
    fn render(&self) -> String {
        match self {
            Item::Line(l) => format!(
                "[L{} {}] {}: {}",
                l.id,
                hms(l.t_ms),
                l.speaker,
                l.text.trim()
            ),
            Item::Note(n) => format!("[{}] > {}", hms(n.t_ms), n.text.trim()),
        }
    }
}

/// Renders lines and notes with `from_ms <= t < to_ms`, in time order, into at most
/// `budget_tokens` (estimated).
pub fn render(
    lines: &[Line],
    notes: &[Note],
    window: (u64, u64),
    budget_tokens: usize,
    keep: Keep,
) -> Rendered {
    let mut items: Vec<Item> = lines
        .iter()
        .filter(|l| l.t_ms >= window.0 && l.t_ms < window.1)
        .map(Item::Line)
        .chain(
            notes
                .iter()
                .filter(|n| n.t_ms >= window.0 && n.t_ms < window.1)
                .map(Item::Note),
        )
        .collect();
    // Stable sort keeps a note after a line with the same timestamp.
    items.sort_by_key(Item::t);

    let rendered: Vec<(String, Option<u64>)> = items
        .iter()
        .map(|i| {
            (
                i.render(),
                match i {
                    Item::Line(l) => Some(l.id),
                    Item::Note(_) => None,
                },
            )
        })
        .collect();

    let mut used = 0usize;
    let mut picked: Vec<usize> = Vec::new();
    let order: Vec<usize> = match keep {
        Keep::Head => (0..rendered.len()).collect(),
        Keep::Tail => (0..rendered.len()).rev().collect(),
    };
    for idx in order {
        let cost = estimate_tokens(&rendered[idx].0) + 1;
        if used + cost > budget_tokens {
            break;
        }
        used += cost;
        picked.push(idx);
    }
    picked.sort_unstable();
    let truncated = picked.len() < rendered.len();
    Rendered {
        text: picked
            .iter()
            .map(|i| rendered[*i].0.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        line_ids: picked.iter().filter_map(|i| rendered[*i].1).collect(),
        truncated,
    }
}

/// Splits a window into pieces that each fit `budget_tokens`, in time order, so a long stretch
/// can be handled one piece at a time (map, then reduce). A single line bigger than the budget
/// still gets a piece of its own.
pub fn render_chunks(
    lines: &[Line],
    notes: &[Note],
    window: (u64, u64),
    budget_tokens: usize,
) -> Vec<Rendered> {
    let mut items: Vec<Item> = lines
        .iter()
        .filter(|l| l.t_ms >= window.0 && l.t_ms < window.1)
        .map(Item::Line)
        .chain(
            notes
                .iter()
                .filter(|n| n.t_ms >= window.0 && n.t_ms < window.1)
                .map(Item::Note),
        )
        .collect();
    items.sort_by_key(Item::t);
    let mut out: Vec<Rendered> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut ids: Vec<u64> = Vec::new();
    let mut used = 0usize;
    let flush = |cur: &mut Vec<String>, ids: &mut Vec<u64>, out: &mut Vec<Rendered>| {
        if !cur.is_empty() {
            out.push(Rendered {
                text: cur.join("\n"),
                line_ids: std::mem::take(ids),
                truncated: false,
            });
            cur.clear();
        }
    };
    for item in &items {
        let text = item.render();
        let cost = estimate_tokens(&text) + 1;
        if used + cost > budget_tokens && !cur.is_empty() {
            flush(&mut cur, &mut ids, &mut out);
            used = 0;
        }
        used += cost;
        if let Item::Line(l) = item {
            ids.push(l.id);
        }
        cur.push(text);
    }
    flush(&mut cur, &mut ids, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: u64, t_ms: u64, who: &str, text: &str) -> Line {
        Line {
            id,
            t_ms,
            speaker: who.into(),
            text: text.into(),
        }
    }

    fn sample() -> (Vec<Line>, Vec<Note>) {
        (
            vec![
                line(1, 1_000, "Me", "Let's start with the budget."),
                line(2, 65_000, "Others", "Finance needs the numbers by Friday."),
                line(3, 3_725_000, "Me", "I will send them."),
            ],
            vec![Note {
                t_ms: 30_000,
                text: "Paused".into(),
            }],
        )
    }

    #[test]
    fn timestamps_match_the_transcript_format() {
        assert_eq!(hms(0), "00:00:00");
        assert_eq!(hms(65_000), "00:01:05");
        assert_eq!(hms(3_725_000), "01:02:05");
    }

    #[test]
    fn lines_carry_ids_and_notes_are_marked() {
        let (l, n) = sample();
        let r = render(&l, &n, (0, u64::MAX), 1000, Keep::Head);
        assert_eq!(
            r.text,
            "[L1 00:00:01] Me: Let's start with the budget.\n[00:00:30] > Paused\n[L2 00:01:05] Others: Finance needs the numbers by Friday.\n[L3 01:02:05] Me: I will send them."
        );
        assert_eq!(r.line_ids, vec![1, 2, 3]);
        assert!(!r.truncated);
    }

    #[test]
    fn window_limits_lines_and_notes() {
        let (l, n) = sample();
        let r = render(&l, &n, (60_000, 120_000), 1000, Keep::Head);
        assert_eq!(r.line_ids, vec![2]);
        assert!(!r.text.contains("Paused"));
    }

    #[test]
    fn head_keeps_the_start_and_tail_keeps_the_end() {
        let (l, n) = sample();
        let budget = estimate_tokens("[L1 00:00:01] Me: Let's start with the budget.") + 2;
        let head = render(&l, &n, (0, u64::MAX), budget, Keep::Head);
        assert_eq!(head.line_ids, vec![1]);
        assert!(head.truncated);
        let budget = estimate_tokens("[L3 01:02:05] Me: I will send them.") + 2;
        let tail = render(&l, &n, (0, u64::MAX), budget, Keep::Tail);
        assert_eq!(tail.line_ids, vec![3]);
        assert!(tail.truncated);
    }

    #[test]
    fn chunks_cover_everything_in_order_and_respect_the_budget() {
        let (l, n) = sample();
        let one = estimate_tokens("[L1 00:00:01] Me: Let's start with the budget.") + 1;
        let chunks = render_chunks(&l, &n, (0, u64::MAX), one * 2);
        let all: Vec<u64> = chunks.iter().flat_map(|c| c.line_ids.clone()).collect();
        assert_eq!(all, vec![1, 2, 3]);
        assert!(chunks.len() >= 2);
        assert!(render_chunks(&l, &n, (0, 100_000), 1000).len() == 1);
        assert!(render_chunks(&l, &n, (9_000_000, 9_000_001), 1000).is_empty());
    }

    #[test]
    fn a_zero_budget_renders_nothing() {
        let (l, n) = sample();
        let r = render(&l, &n, (0, u64::MAX), 0, Keep::Tail);
        assert!(r.text.is_empty() && r.line_ids.is_empty() && r.truncated);
    }
}

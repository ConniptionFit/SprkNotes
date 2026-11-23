//! Finds the lines a question is about (ADR-009): BM25 over the transcript lines, then a little
//! context around each hit. No embedding model; the Library's full-text search works the same way.
use crate::view::{estimate_tokens, Line};
use std::collections::{BTreeSet, HashMap};

const STOP: [&str; 48] = [
    "the", "a", "an", "and", "or", "of", "to", "in", "on", "at", "for", "is", "are", "was", "were",
    "be", "been", "it", "this", "that", "what", "when", "who", "where", "how", "why", "do", "does",
    "did", "i", "we", "you", "they", "he", "she", "my", "our", "your", "about", "with", "by", "as",
    "any", "there", "their", "me", "us", "have",
];

pub fn terms(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| !w.is_empty() && !STOP.contains(&w.as_str()))
        .map(|w| stem(&w))
        .collect()
}

/// Very light stemming so "invoices" finds "invoice" and "scheduled" finds "schedule".
fn stem(w: &str) -> String {
    let mut base = w;
    for suffix in ["ing", "ed", "es", "s"] {
        if w.len() > suffix.len() + 2 {
            if let Some(b) = w.strip_suffix(suffix) {
                base = b;
                break;
            }
        }
    }
    // "invoice" and "invoices" both end up as "invoic".
    if base.len() > 3 {
        base = base.strip_suffix('e').unwrap_or(base);
    }
    base.to_string()
}

/// Lines most relevant to `query`, best first, with scores. Empty when nothing matches.
pub fn search(lines: &[Line], query: &str, top: usize) -> Vec<(usize, f64)> {
    let q: BTreeSet<String> = terms(query).into_iter().collect();
    if q.is_empty() || lines.is_empty() {
        return vec![];
    }
    let docs: Vec<Vec<String>> = lines.iter().map(|l| terms(&l.text)).collect();
    let n = docs.len() as f64;
    let avg = docs.iter().map(Vec::len).sum::<usize>() as f64 / n;
    let mut df: HashMap<&str, f64> = HashMap::new();
    for d in &docs {
        for t in d.iter().collect::<BTreeSet<_>>() {
            *df.entry(t.as_str()).or_insert(0.0) += 1.0;
        }
    }
    let (k1, b) = (1.2, 0.75);
    let mut scored: Vec<(usize, f64)> = docs
        .iter()
        .enumerate()
        .filter_map(|(i, d)| {
            let len = d.len() as f64;
            let mut score = 0.0;
            for t in &q {
                let tf = d.iter().filter(|x| *x == t).count() as f64;
                if tf == 0.0 {
                    continue;
                }
                let dfv = df.get(t.as_str()).copied().unwrap_or(0.0);
                let idf = ((n - dfv + 0.5) / (dfv + 0.5) + 1.0).ln();
                score += idf * (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * len / avg.max(1.0)));
            }
            (score > 0.0).then_some((i, score))
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.truncate(top);
    scored
}

/// The lines to show the model: hits plus `around` neighbours each side, in time order, within
/// `budget_tokens`. Best hits claim budget first. Empty when the question matches nothing.
pub fn context_for(
    lines: &[Line],
    query: &str,
    top: usize,
    around: usize,
    budget_tokens: usize,
) -> Vec<Line> {
    let mut keep: BTreeSet<usize> = BTreeSet::new();
    let mut used = 0usize;
    for (i, _) in search(lines, query, top) {
        let lo = i.saturating_sub(around);
        let hi = (i + around).min(lines.len() - 1);
        let add: Vec<usize> = (lo..=hi).filter(|j| !keep.contains(j)).collect();
        let cost: usize = add
            .iter()
            .map(|j| estimate_tokens(&lines[*j].text) + 12)
            .sum();
        if used + cost > budget_tokens {
            continue;
        }
        used += cost;
        keep.extend(add);
    }
    keep.into_iter().map(|j| lines[j].clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines() -> Vec<Line> {
        let texts = [
            "Good morning everyone, let's get started.",
            "The vendor invoices are due on the fifteenth.",
            "Finance wants the quarterly numbers by Friday.",
            "Does the budget include the laptop purchase?",
            "Yes the laptop purchase order is in the second tab.",
            "Let's talk about the holiday party.",
            "We will schedule the migration after the audit.",
        ];
        texts
            .iter()
            .enumerate()
            .map(|(i, t)| Line {
                id: i as u64 + 1,
                t_ms: i as u64 * 10_000,
                speaker: "Others".into(),
                text: (*t).into(),
            })
            .collect()
    }

    #[test]
    fn stemming_lets_plurals_and_tenses_match() {
        assert_eq!(terms("Invoices were scheduled"), vec!["invoic", "schedul"]);
        let hits = search(&lines(), "when is the invoice due", 3);
        assert_eq!(hits[0].0, 1);
    }

    #[test]
    fn the_best_line_ranks_first_and_rare_words_count_more() {
        let hits = search(&lines(), "laptop purchase order", 3);
        assert_eq!(hits[0].0, 4);
        assert!(hits.len() >= 2 && hits[0].1 > hits[1].1);
    }

    #[test]
    fn no_match_means_no_context() {
        assert!(search(&lines(), "kubernetes cluster", 3).is_empty());
        assert!(context_for(&lines(), "kubernetes cluster", 3, 1, 1000).is_empty());
        assert!(search(&lines(), "the and of", 3).is_empty());
    }

    #[test]
    fn context_adds_neighbours_in_time_order_and_respects_the_budget() {
        let c = context_for(&lines(), "migration", 2, 1, 1000);
        let ids: Vec<u64> = c.iter().map(|l| l.id).collect();
        assert_eq!(ids, vec![6, 7]);
        let c = context_for(&lines(), "invoice", 1, 1, 1000);
        assert_eq!(c.iter().map(|l| l.id).collect::<Vec<_>>(), vec![1, 2, 3]);
        // A budget too small for any hit returns nothing rather than a cut-off excerpt.
        assert!(context_for(&lines(), "invoice", 1, 1, 5).is_empty());
    }
}

//! Statement rules (SPEC 4.3): filler filter, fragment merge, long-utterance split.
use crate::Segment;

const FILLERS: &[&str] = &[
    "um", "uh", "umm", "uhh", "er", "erm", "ah", "hmm", "mm", "mhm", "mm-hmm", "uh-huh",
];
pub const MAX_WORDS: usize = 40;
const SHORT_FRAGMENT_WORDS: usize = 4;
const MERGE_GAP_MS: u64 = 1_000;

fn words(text: &str) -> Vec<&str> {
    text.split_whitespace().collect()
}

fn bare(word: &str) -> String {
    word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-')
        .to_lowercase()
}

/// True when the text is nothing but filler words ("Um.", "mm-hmm, uh").
pub fn is_filler_only(text: &str) -> bool {
    let w = words(text);
    !w.is_empty() && w.iter().all(|x| FILLERS.contains(&bare(x).as_str()))
}

/// Drop standalone fillers (setting `drop_fillers`, default on).
pub fn drop_fillers(segs: Vec<Segment>, enabled: bool) -> Vec<Segment> {
    if !enabled {
        return segs;
    }
    segs.into_iter()
        .filter(|s| !is_filler_only(&s.text))
        .collect()
}

/// Merge consecutive short fragments from the same speaker that are within 1 s.
/// Input must be sorted by start time.
pub fn merge_fragments(segs: Vec<Segment>) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    for s in segs {
        if let Some(prev) = out.last_mut() {
            let gap = s.start_ms.saturating_sub(prev.end_ms);
            if prev.speaker == s.speaker
                && gap <= MERGE_GAP_MS
                && words(&prev.text).len() <= SHORT_FRAGMENT_WORDS
                && words(&s.text).len() <= SHORT_FRAGMENT_WORDS
            {
                prev.text = format!("{} {}", prev.text.trim(), s.text.trim());
                prev.end_ms = prev.end_ms.max(s.end_ms);
                continue;
            }
        }
        out.push(s);
    }
    out
}

fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for w in text.split_whitespace() {
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
        if w.ends_with(['.', '?', '!']) {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Split utterances over ~40 words at sentence boundaries. Times are shared out in
/// proportion to word counts. A single sentence longer than the limit stays whole.
pub fn split_long(seg: Segment) -> Vec<Segment> {
    let total = words(&seg.text).len();
    if total <= MAX_WORDS {
        return vec![seg];
    }
    let mut groups: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_words = 0usize;
    for s in sentences(&seg.text) {
        let n = words(&s).len();
        if cur_words > 0 && cur_words + n > MAX_WORDS {
            groups.push(std::mem::take(&mut cur));
            cur_words = 0;
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(&s);
        cur_words += n;
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    let span = seg.end_ms.saturating_sub(seg.start_ms);
    let mut done = 0usize;
    groups
        .into_iter()
        .map(|g| {
            let n = words(&g).len();
            let start = seg.start_ms + span * done as u64 / total as u64;
            done += n;
            let end = seg.start_ms + span * done as u64 / total as u64;
            Segment {
                speaker: seg.speaker.clone(),
                start_ms: start,
                end_ms: end,
                text: g,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Speaker;

    fn seg(sp: Speaker, a: u64, b: u64, t: &str) -> Segment {
        Segment {
            speaker: sp,
            start_ms: a,
            end_ms: b,
            text: t.into(),
        }
    }

    #[test]
    fn fillers_dropped_only_when_standalone() {
        assert!(is_filler_only("Um."));
        assert!(is_filler_only("mm-hmm, uh"));
        assert!(!is_filler_only("um, I think so"));
        let out = drop_fillers(
            vec![seg(Speaker::Me, 0, 1, "uh"), seg(Speaker::Me, 2, 3, "yes")],
            true,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(
            drop_fillers(vec![seg(Speaker::Me, 0, 1, "uh")], false).len(),
            1
        );
    }

    #[test]
    fn short_fragments_merge_within_one_second() {
        let out = merge_fragments(vec![
            seg(Speaker::Me, 0, 1_000, "So the"),
            seg(Speaker::Me, 1_500, 2_500, "first sync"),
            seg(Speaker::Me, 4_000, 5_000, "finished"),
        ]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].text, "So the first sync");
        assert_eq!(out[0].end_ms, 2_500);
    }

    #[test]
    fn fragments_do_not_merge_across_speakers_or_long_lines() {
        let out = merge_fragments(vec![
            seg(Speaker::Me, 0, 1_000, "ok"),
            seg(Speaker::Others, 1_100, 1_500, "right"),
            seg(
                Speaker::Others,
                1_600,
                4_000,
                "this is a considerably longer statement here",
            ),
        ]);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn long_utterances_split_at_sentences() {
        let sentence = "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty."; // 20 words
        let text = format!("{sentence} {sentence} {sentence}");
        let parts = split_long(seg(Speaker::Me, 0, 60_000, &text));
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|p| words(&p.text).len() <= MAX_WORDS));
        assert_eq!(parts[0].start_ms, 0);
        assert_eq!(parts[1].end_ms, 60_000);
        assert_eq!(parts[0].end_ms, parts[1].start_ms);
    }

    #[test]
    fn short_utterances_untouched() {
        assert_eq!(split_long(seg(Speaker::Me, 0, 1, "hello there")).len(), 1);
    }
}

//! Transcript dedupe (SPEC 4.2 layer 3): drop a Me segment when a remote segment overlaps
//! it in time (within +/- 2 s) and the normalized text is at least 60% similar.
//! The remote version is preferred. Always on.
use crate::Segment;

pub const OVERLAP_MS: u64 = 2_000;
pub const DEFAULT_THRESHOLD: f64 = 0.6;

fn tokens(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

fn lcs(a: &[String], b: &[String]) -> usize {
    let mut prev = vec![0usize; b.len() + 1];
    for x in a {
        let mut cur = vec![0usize; b.len() + 1];
        for (j, y) in b.iter().enumerate() {
            cur[j + 1] = if x == y {
                prev[j] + 1
            } else {
                cur[j].max(prev[j + 1])
            };
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Token-level similarity in 0..=1: 2 * LCS / (len a + len b).
pub fn similarity(a: &str, b: &str) -> f64 {
    let (a, b) = (tokens(a), tokens(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    2.0 * lcs(&a, &b) as f64 / (a.len() + b.len()) as f64
}

/// A Me line this short is never judged by containment alone ("yes", "okay thanks").
const MIN_CONTAINED_WORDS: usize = 4;
/// Share of the Me line's words that must appear, in order, in the remote speech around it.
const CONTAINED_SHARE: f64 = 0.8;

fn overlaps(a: &Segment, b: &Segment) -> bool {
    a.start_ms <= b.end_ms + OVERLAP_MS && b.start_ms <= a.end_ms + OVERLAP_MS
}

/// True when `me` is a bleed duplicate of the remote speech around it.
///
/// Two tests. First, whole-line similarity against any single remote line. Second,
/// containment: the microphone and the system stream cut speech into lines differently, so a
/// bleed line can be a piece of one remote line or span two. The remote lines near it are
/// joined in time order and the Me line must sit inside them (at least 80% of its words, in
/// order). Lines under four words are only judged by the first test.
pub fn is_bleed(me: &Segment, remote: &[Segment], threshold: f64) -> bool {
    if me.speaker != crate::Speaker::Me {
        return false;
    }
    let near: Vec<&Segment> = remote
        .iter()
        .filter(|r| r.speaker.is_remote() && overlaps(me, r))
        .collect();
    if near
        .iter()
        .any(|r| similarity(&me.text, &r.text) >= threshold)
    {
        return true;
    }
    let mine = tokens(&me.text);
    if mine.len() < MIN_CONTAINED_WORDS || near.is_empty() {
        return false;
    }
    let mut near = near;
    near.sort_by_key(|r| (r.start_ms, r.end_ms));
    let joined: Vec<String> = near.iter().flat_map(|r| tokens(&r.text)).collect();
    lcs(&mine, &joined) as f64 / mine.len() as f64 >= CONTAINED_SHARE
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
    fn identical_text_is_fully_similar() {
        assert!((similarity("Hello, world!", "hello world") - 1.0).abs() < 1e-9);
        assert_eq!(similarity("", "x"), 0.0);
    }

    #[test]
    fn echo_with_asr_differences_is_caught() {
        let remote = [seg(
            Speaker::Others,
            10_000,
            14_000,
            "the quarterly numbers look strong this time",
        )];
        let me = seg(
            Speaker::Me,
            10_800,
            14_500,
            "the quarterly numbers look strong this tide",
        );
        assert!(is_bleed(&me, &remote, DEFAULT_THRESHOLD));
    }

    #[test]
    fn real_reply_is_kept() {
        let remote = [seg(
            Speaker::Others,
            10_000,
            14_000,
            "the quarterly numbers look strong this time",
        )];
        let me = seg(
            Speaker::Me,
            10_500,
            12_000,
            "great, send me the deck please",
        );
        assert!(!is_bleed(&me, &remote, DEFAULT_THRESHOLD));
    }

    #[test]
    fn similar_text_far_apart_in_time_is_kept() {
        let remote = [seg(Speaker::Others, 10_000, 12_000, "sounds good to me")];
        let me = seg(Speaker::Me, 60_000, 62_000, "sounds good to me");
        assert!(!is_bleed(&me, &remote, DEFAULT_THRESHOLD));
    }

    #[test]
    fn only_me_segments_are_dropped() {
        let remote = [seg(Speaker::Me, 0, 1_000, "hello there")];
        let other = seg(Speaker::Others, 0, 1_000, "hello there");
        assert!(!is_bleed(&other, &remote, DEFAULT_THRESHOLD));
    }

    #[test]
    fn bleed_line_that_is_a_piece_of_a_longer_remote_line_is_caught() {
        let remote = [seg(
            Speaker::Others,
            10_000,
            20_000,
            "so the night shift crew will start the migration at nine and the day team will verify it by noon",
        )];
        let me = seg(Speaker::Me, 14_000, 17_000, "start the migration at nine");
        assert!(similarity(&me.text, &remote[0].text) < DEFAULT_THRESHOLD);
        assert!(is_bleed(&me, &remote, DEFAULT_THRESHOLD));
    }

    #[test]
    fn bleed_line_spanning_two_remote_lines_is_caught() {
        let remote = [
            seg(
                Speaker::Others,
                10_000,
                13_000,
                "we should ship the fix on thursday",
            ),
            seg(
                Speaker::Others,
                13_200,
                16_000,
                "after the review board signs off on it",
            ),
        ];
        let me = seg(
            Speaker::Me,
            11_500,
            15_500,
            "ship the fix on thursday after the review board signs off",
        );
        assert!(is_bleed(&me, &remote, DEFAULT_THRESHOLD));
    }

    #[test]
    fn short_or_different_me_speech_near_remote_speech_is_kept() {
        let remote = [seg(
            Speaker::Others,
            10_000,
            16_000,
            "we should ship the fix on thursday after the review board signs off",
        )];
        let short = seg(Speaker::Me, 11_000, 11_800, "ship the fix");
        let real = seg(
            Speaker::Me,
            12_000,
            15_000,
            "thursday works but I need the rollback plan first",
        );
        assert!(!is_bleed(&short, &remote, DEFAULT_THRESHOLD));
        assert!(!is_bleed(&real, &remote, DEFAULT_THRESHOLD));
    }
}

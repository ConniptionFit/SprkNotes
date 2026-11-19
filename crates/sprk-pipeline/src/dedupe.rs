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

/// Token-level similarity in 0..=1: 2 * LCS / (len a + len b).
pub fn similarity(a: &str, b: &str) -> f64 {
    let (a, b) = (tokens(a), tokens(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut prev = vec![0usize; b.len() + 1];
    for x in &a {
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
    2.0 * prev[b.len()] as f64 / (a.len() + b.len()) as f64
}

fn overlaps(a: &Segment, b: &Segment) -> bool {
    a.start_ms <= b.end_ms + OVERLAP_MS && b.start_ms <= a.end_ms + OVERLAP_MS
}

/// True when `me` is a bleed duplicate of any segment in `remote`.
pub fn is_bleed(me: &Segment, remote: &[Segment], threshold: f64) -> bool {
    me.speaker == crate::Speaker::Me
        && remote.iter().any(|r| {
            r.speaker.is_remote() && overlaps(me, r) && similarity(&me.text, &r.text) >= threshold
        })
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
}

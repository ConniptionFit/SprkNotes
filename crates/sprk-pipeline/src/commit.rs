//! Commit buffer (SPEC 4.4): finalized segments wait a configurable delay (default 6 s,
//! 3 to 15 s) for dedupe and merge, then commit in timestamp order. Flush on pause, end, panic.
use crate::dedupe::{is_bleed, DEFAULT_THRESHOLD};
use crate::rules::{drop_fillers, merge_fragments, split_long};
use crate::Segment;

pub const DEFAULT_DELAY_MS: u64 = 6_000;
const MIN_DELAY_MS: u64 = 3_000;
const MAX_DELAY_MS: u64 = 15_000;
/// A line still waiting on another stream's unfinished speech is released anyway after this
/// much longer than the delay, so a stalled source can never hold the transcript back.
const MAX_HOLD_MS: u64 = 20_000;
/// How long committed remote segments stay available for dedupe of late Me segments.
const REMOTE_HISTORY_MS: u64 = 15_000;

#[derive(Debug, Clone)]
pub struct Config {
    pub delay_ms: u64,
    pub drop_fillers: bool,
    pub dedupe_threshold: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            delay_ms: DEFAULT_DELAY_MS,
            drop_fillers: true,
            dedupe_threshold: DEFAULT_THRESHOLD,
        }
    }
}

pub struct CommitBuffer {
    cfg: Config,
    pending: Vec<Segment>,
    remote_history: Vec<Segment>,
}

impl CommitBuffer {
    pub fn new(mut cfg: Config) -> Self {
        cfg.delay_ms = cfg.delay_ms.clamp(MIN_DELAY_MS, MAX_DELAY_MS);
        Self {
            cfg,
            pending: Vec::new(),
            remote_history: Vec::new(),
        }
    }

    pub fn push(&mut self, seg: Segment) {
        self.pending.push(seg);
    }

    /// Segments whose end time is at least `delay` behind `now_ms` are committed.
    /// Returned in timestamp order. Segments still inside the delay stay buffered.
    pub fn drain_ready(&mut self, now_ms: u64) -> Vec<Segment> {
        self.drain_ready_before(now_ms, u64::MAX)
    }

    /// As [`drain_ready`](Self::drain_ready), but a segment ending after `settled_ms` waits:
    /// speech that began earlier on another stream is still being decoded and would otherwise
    /// land after it. The wait is capped, so a stalled stream cannot block everything.
    pub fn drain_ready_before(&mut self, now_ms: u64, settled_ms: u64) -> Vec<Segment> {
        let delay = self.cfg.delay_ms;
        let (ready, keep): (Vec<_>, Vec<_>) = self.pending.drain(..).partition(|s| {
            s.end_ms + delay <= now_ms
                && (s.end_ms <= settled_ms || s.end_ms + delay + MAX_HOLD_MS <= now_ms)
        });
        self.pending = keep;
        self.finish(ready, now_ms)
    }

    /// Commit everything now (pause, end, panic).
    pub fn flush(&mut self, now_ms: u64) -> Vec<Segment> {
        let all: Vec<_> = std::mem::take(&mut self.pending);
        self.finish(all, now_ms)
    }

    fn finish(&mut self, mut batch: Vec<Segment>, now_ms: u64) -> Vec<Segment> {
        batch.sort_by_key(|s| (s.start_ms, s.end_ms));
        // Remote evidence for dedupe: committed history plus everything still waiting.
        let remote: Vec<Segment> = self
            .remote_history
            .iter()
            .chain(self.pending.iter())
            .chain(batch.iter())
            .filter(|s| s.speaker.is_remote())
            .cloned()
            .collect();
        let thr = self.cfg.dedupe_threshold;
        let batch: Vec<Segment> = batch
            .into_iter()
            .filter(|s| !is_bleed(s, &remote, thr))
            .collect();
        let batch = drop_fillers(batch, self.cfg.drop_fillers);
        let batch = merge_fragments(batch);
        // Long utterances become several lines with their own times; order again afterwards
        // so two people talking at once interleave instead of landing in two blocks.
        let mut out: Vec<Segment> = batch.into_iter().flat_map(split_long).collect();
        out.sort_by_key(|s| (s.start_ms, s.end_ms));
        self.remote_history
            .extend(out.iter().filter(|s| s.speaker.is_remote()).cloned());
        self.remote_history
            .retain(|s| s.end_ms + REMOTE_HISTORY_MS >= now_ms);
        out
    }
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
    fn a_line_waits_for_earlier_speech_still_being_decoded_on_the_other_stream() {
        let mut b = CommitBuffer::new(Config::default());
        b.push(seg(Speaker::Others, 20_000, 24_000, "later remote line"));
        // The mic is still mid-utterance from 11 s: nothing ending after that may go out.
        assert!(b.drain_ready_before(40_000, 11_000).is_empty());
        // The utterance arrives, then both commit in time order.
        b.push(seg(Speaker::Me, 11_000, 18_000, "earlier local line"));
        let out = b.drain_ready_before(40_000, 25_000);
        let who: Vec<_> = out.iter().map(|s| s.start_ms).collect();
        assert_eq!(who, vec![11_000, 20_000]);
    }

    #[test]
    fn long_utterances_from_both_streams_interleave_by_time() {
        let long = |w: &str| {
            (0..72)
                .map(|i| format!("{w}{i}{}", if i % 12 == 11 { "." } else { "" }))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let mut b = CommitBuffer::new(Config::default());
        b.push(seg(Speaker::Me, 0, 40_000, &long("mine")));
        b.push(seg(Speaker::Others, 10_000, 50_000, &long("yours")));
        let out = b.flush(60_000);
        assert!(out.len() >= 4, "long utterances split into several lines");
        assert!(
            out.windows(2).all(|w| w[0].start_ms <= w[1].start_ms),
            "time order"
        );
        let first_other = out.iter().position(|s| s.speaker.is_remote()).unwrap();
        let last_me = out.iter().rposition(|s| !s.speaker.is_remote()).unwrap();
        assert!(first_other < last_me, "the two speakers interleave");
    }

    #[test]
    fn a_stalled_stream_cannot_hold_lines_forever() {
        let mut b = CommitBuffer::new(Config::default());
        b.push(seg(Speaker::Others, 20_000, 24_000, "remote line"));
        assert!(b.drain_ready_before(40_000, 11_000).is_empty());
        // Past the delay plus the hold cap it goes out regardless.
        assert_eq!(
            b.drain_ready_before(24_000 + 6_000 + 20_000, 11_000).len(),
            1
        );
    }

    #[test]
    fn nothing_commits_inside_the_delay() {
        let mut b = CommitBuffer::new(Config::default());
        b.push(seg(Speaker::Me, 1_000, 3_000, "hello team"));
        assert!(b.drain_ready(8_999).is_empty());
        assert_eq!(b.drain_ready(9_000).len(), 1);
    }

    #[test]
    fn commits_in_timestamp_order_across_streams() {
        let mut b = CommitBuffer::new(Config::default());
        b.push(seg(Speaker::Others, 5_000, 6_000, "second thing here"));
        b.push(seg(Speaker::Me, 1_000, 2_000, "first thing"));
        let out = b.flush(7_000);
        assert_eq!(out[0].text, "first thing");
        assert_eq!(out[1].text, "second thing here");
    }

    #[test]
    fn bleed_duplicate_is_dropped_even_when_remote_arrives_later() {
        let mut b = CommitBuffer::new(Config::default());
        b.push(seg(
            Speaker::Me,
            10_000,
            13_000,
            "please review the attached budget",
        ));
        b.push(seg(
            Speaker::Others,
            10_200,
            13_100,
            "please review the attached budget",
        ));
        let out = b.drain_ready(30_000);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].speaker, Speaker::Others);
    }

    #[test]
    fn late_me_echo_is_caught_by_committed_history() {
        let mut b = CommitBuffer::new(Config::default());
        b.push(seg(
            Speaker::Others,
            10_000,
            13_000,
            "please review the attached budget",
        ));
        assert_eq!(b.drain_ready(20_000).len(), 1);
        // the echo is only transcribed after the remote line already committed
        b.push(seg(
            Speaker::Me,
            10_400,
            13_400,
            "please review the attached budget",
        ));
        assert!(b.flush(21_000).is_empty());
    }

    #[test]
    fn flush_commits_everything_and_applies_rules() {
        let mut b = CommitBuffer::new(Config::default());
        b.push(seg(Speaker::Me, 0, 500, "um"));
        b.push(seg(Speaker::Me, 1_000, 2_000, "real statement here"));
        let out = b.flush(2_100);
        assert_eq!(out.len(), 1);
        assert!(b.flush(2_200).is_empty());
    }

    #[test]
    fn delay_is_clamped_to_allowed_range() {
        let mut b = CommitBuffer::new(Config {
            delay_ms: 100,
            ..Config::default()
        });
        b.push(seg(Speaker::Me, 0, 1_000, "clamped delay test"));
        assert!(b.drain_ready(3_999).is_empty());
        assert_eq!(b.drain_ready(4_000).len(), 1);
    }
}

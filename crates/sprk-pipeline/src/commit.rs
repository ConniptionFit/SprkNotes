//! Commit buffer (SPEC 4.4): finalized segments wait a configurable delay (default 6 s,
//! 3 to 15 s) for dedupe and merge, then commit in timestamp order. Flush on pause, end, panic.
use crate::dedupe::{is_bleed, DEFAULT_THRESHOLD};
use crate::rules::{drop_fillers, merge_fragments, split_long};
use crate::Segment;

pub const DEFAULT_DELAY_MS: u64 = 6_000;
const MIN_DELAY_MS: u64 = 3_000;
const MAX_DELAY_MS: u64 = 15_000;
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
        let delay = self.cfg.delay_ms;
        let (ready, keep): (Vec<_>, Vec<_>) = self
            .pending
            .drain(..)
            .partition(|s| s.end_ms + delay <= now_ms);
        self.pending = keep;
        self.finish(ready, now_ms)
    }

    /// Commit everything now (pause, end, panic).
    pub fn flush(&mut self, now_ms: u64) -> Vec<Segment> {
        let all: Vec<_> = self.pending.drain(..).collect();
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
        let out: Vec<Segment> = batch.into_iter().flat_map(split_long).collect();
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

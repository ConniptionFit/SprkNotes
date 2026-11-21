//! Live captions (ADR-006): a rolling look at the speech still in progress.
//!
//! Captions are never committed, written or logged. They are a transient view for the overlay
//! of the last few seconds of audio per source, re-decoded every 0.7 s or so. The committed
//! transcript is untouched: it still comes only from finished VAD utterances (rule 13).
//!
//! Streaming behaviour (ADR-007, ADR-008): each decode is a *hypothesis*. A word two
//! hypotheses in a row agree on is settled; the rest are tentative. Settled words *lock*:
//! they never change in the live view again. Locking happens at a sentence end, when a word
//! is older than 1.5 s, or after a short quiet. Locked sentences leave the live row as
//! `sealed` rows, so non-stop speech never builds a minutes-long grey block. Hypotheses are
//! aligned to what is already locked, so the 10 s window sliding forward does not unsettle
//! the line. When the VAD closes an utterance the committed decode is sent as a final caption.
//!
//! Cost control: the decode time is measured and the next decode waits at least twice that long,
//! so captions never take more than about a third of the session thread. If decoding stays far
//! slower than real time, captions switch themselves off for the rest of the session.
use crate::Speaker;
use sprk_asr::Engine;
use sprk_audio::Source;
use std::time::Instant;

const RATE: usize = 16_000;
/// Only the last 10 s of an utterance are decoded.
const MAX_SAMPLES: usize = 10 * RATE;
/// Need at least 0.6 s of audio before there is anything worth showing.
const MIN_SAMPLES: usize = RATE * 6 / 10;
const BASE_INTERVAL_MS: u64 = 450;
/// Wait for at least this much new audio before decoding again.
const MIN_NEW_SAMPLES: usize = RATE / 5;
const MAX_INTERVAL_MS: u64 = 5_000;
/// A decode slower than this counts as "falling behind".
const SLOW_MS: u64 = 3_000;
const SLOW_LIMIT: u32 = 3;
/// Peak level below which the recent audio is treated as silence and not decoded.
const SILENCE_PEAK: f32 = 0.005;
/// A word that has been on screen this long locks even if its sentence goes on.
const AGE_LOCK_MS: u64 = 1_500;
/// A live row longer than this many locked words is sealed as its own row.
const ROW_MAX_WORDS: usize = 40;
/// How many locked words are remembered to line a new hypothesis up with the locked text.
const MEMORY_WORDS: usize = 12;

#[derive(Debug, Clone, PartialEq)]
pub struct Caption {
    pub speaker: Speaker,
    /// The whole line so far, stable words first.
    pub text: String,
    /// How many leading words of `text` are settled. The rest are tentative.
    pub stable: usize,
    /// The utterance is finished: `text` is the committed decode and every word is settled.
    pub is_final: bool,
    /// Rows that locked and left the live row since the last caption (ADR-008). Oldest first.
    pub sealed: Vec<String>,
    /// Which utterance this belongs to, so a final caption can replace its locked rows.
    pub utt: u64,
}

impl Caption {
    pub fn word_count(&self) -> usize {
        self.text.split_whitespace().count()
    }
}

/// Words compared without case or punctuation, so "Hello," and "hello" agree.
fn norm(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect()
        })
        .collect()
}

/// A word like "Night": first letter upper case, every other letter lower case. Acronyms
/// ("NASA") and "I" do not count.
fn is_capitalized(w: &str) -> bool {
    let mut letters = w.chars().filter(|c| c.is_alphabetic());
    match letters.next() {
        Some(f) if f.is_uppercase() => {
            let rest: Vec<char> = letters.collect();
            !rest.is_empty() && rest.iter().all(|c| c.is_lowercase())
        }
        _ => false,
    }
}

/// A short rolling window has little context, so the engine sometimes title-cases a phrase
/// ("The Night Shift Crew") that the full utterance decode writes in sentence case. Lower a run
/// of three or more capitalized words inside a sentence so the live line matches the saved one.
/// The first word of a sentence keeps its capital, and one or two capitalized words in a row
/// (names, places) are left alone.
fn tame_title_case(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out: Vec<String> = words.iter().map(|w| (*w).to_string()).collect();
    let mut i = 0;
    while i < words.len() {
        if !is_capitalized(words[i]) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < words.len() && is_capitalized(words[j]) {
            j += 1;
            if ends_sentence(words[j - 1]) {
                break;
            }
        }
        let starts_sentence = i == 0 || ends_sentence(words[i - 1]);
        let lead = usize::from(starts_sentence);
        if j - i - lead >= 3 {
            for k in (i + lead)..j {
                out[k] = words[k].to_lowercase();
            }
        }
        i = j;
    }
    out.join(" ")
}

/// One word still on trial.
#[derive(Debug, Clone)]
struct Tw {
    word: String,
    norm: String,
    /// Decodes in a row that produced this word at this place.
    seen: u32,
    first_ms: u64,
}

fn ends_sentence(w: &str) -> bool {
    w.trim_end_matches(['"', '\'', ')', ']'])
        .ends_with(['.', '?', '!'])
}

/// Turns a stream of whole-line hypotheses into locked words plus a short tentative tail.
#[derive(Default)]
struct Stab {
    /// Locked words of the row being built.
    row: Vec<String>,
    /// Rows that locked since the last emit.
    sealed: Vec<String>,
    tail: Vec<Tw>,
    /// Last locked words, normalised, for lining up the next hypothesis.
    memory: Vec<String>,
}

impl Stab {
    fn lock(&mut self, n: usize) {
        for t in self
            .tail
            .drain(..n.min(self.tail.len()))
            .collect::<Vec<_>>()
        {
            self.memory.push(t.norm);
            self.row.push(t.word.clone());
            if ends_sentence(&t.word) {
                self.seal();
            }
        }
        if self.memory.len() > MEMORY_WORDS {
            let cut = self.memory.len() - MEMORY_WORDS;
            self.memory.drain(..cut);
        }
        if self.row.len() >= ROW_MAX_WORDS {
            self.seal();
        }
    }

    fn seal(&mut self) {
        if !self.row.is_empty() {
            self.sealed.push(self.row.join(" "));
            self.row.clear();
        }
    }

    /// Where in `hyp` the not-yet-locked words begin.
    fn start_in(&self, hyp: &[(String, String)]) -> usize {
        let m = self.memory.len();
        if m == 0 {
            return 0;
        }
        for k in (m.min(8).min(2)..=m.min(8)).rev() {
            let want = &self.memory[m - k..];
            for j in 0..=hyp.len().saturating_sub(k) {
                if hyp.len() >= j + k && hyp[j..j + k].iter().map(|h| &h.1).eq(want.iter()) {
                    return j + k;
                }
            }
        }
        0
    }

    fn update(&mut self, text: &str, now_ms: u64) {
        let hyp: Vec<(String, String)> = text
            .split_whitespace()
            .map(|w| (w.to_string(), norm(w).pop().unwrap_or_default()))
            .collect();
        let start = self.start_in(&hyp);
        let new = &hyp[start.min(hyp.len())..];
        let agree = self
            .tail
            .iter()
            .zip(new)
            .take_while(|(a, b)| a.norm == b.1)
            .count();
        let mut tail: Vec<Tw> = Vec::with_capacity(new.len());
        for (i, (w, n)) in new.iter().enumerate() {
            if i < agree {
                let old = &self.tail[i];
                tail.push(Tw {
                    word: w.clone(),
                    norm: n.clone(),
                    seen: old.seen + 1,
                    first_ms: old.first_ms,
                });
            } else {
                tail.push(Tw {
                    word: w.clone(),
                    norm: n.clone(),
                    seen: 1,
                    first_ms: now_ms,
                });
            }
        }
        self.tail = tail;
        // Lock through the last agreed sentence end that has a word after it, and through
        // every leading word that has been on screen long enough.
        let mut n = 0;
        for (i, t) in self.tail.iter().enumerate() {
            if t.seen >= 2 && ends_sentence(&t.word) && i + 1 < self.tail.len() {
                n = i + 1;
            }
        }
        let aged = self
            .tail
            .iter()
            .take_while(|t| now_ms.saturating_sub(t.first_ms) >= AGE_LOCK_MS)
            .count();
        self.lock(n.max(aged));
    }

    /// A pause: everything that is left locks and the row is sealed. True if anything moved.
    fn settle(&mut self) -> bool {
        if self.tail.is_empty() && self.row.is_empty() {
            return false;
        }
        let n = self.tail.len();
        self.lock(n);
        self.seal();
        true
    }

    fn live_text(&self) -> String {
        let mut words: Vec<&str> = self.row.iter().map(String::as_str).collect();
        words.extend(self.tail.iter().map(|t| t.word.as_str()));
        words.join(" ")
    }

    /// Locked words of the row plus the tail words that two decodes agree on.
    fn settled(&self) -> usize {
        self.row.len() + self.tail.iter().take_while(|t| t.seen >= 2).count()
    }
}

#[derive(Default)]
struct Lane {
    buf: Vec<f32>,
    stab: Stab,
    /// What was last sent, so an unchanged line is not sent again.
    last_sent: (String, usize),
    /// Counts finished utterances.
    utt: u64,
    /// Samples fed since the last decode.
    fresh: usize,
    next_at_ms: u64,
}

impl Lane {
    fn reset(&mut self) {
        self.buf.clear();
        self.stab = Stab::default();
        self.last_sent = (String::new(), 0);
        self.fresh = 0;
    }

    fn push(&mut self, s: &[f32]) {
        self.buf.extend_from_slice(s);
        self.fresh += s.len();
        if self.buf.len() > MAX_SAMPLES {
            let cut = self.buf.len() - MAX_SAMPLES;
            self.buf.drain(..cut);
        }
    }

    fn quiet(&self) -> bool {
        let tail = self.buf.len().saturating_sub(RATE / 2);
        self.buf[tail..].iter().all(|x| x.abs() < SILENCE_PEAK)
    }
}

pub struct Captioner {
    mic: Lane,
    system: Lane,
    on: bool,
    me: bool,
    slow: u32,
    disabled: bool,
}

impl Default for Captioner {
    fn default() -> Self {
        Self::new()
    }
}

impl Captioner {
    pub fn new() -> Self {
        Captioner {
            mic: Lane::default(),
            system: Lane::default(),
            on: false,
            me: false,
            slow: 0,
            disabled: false,
        }
    }

    /// What the UI wants: captions on, and whether the microphone ("Me") is included.
    pub fn set_wanted(&mut self, on: bool, me: bool) {
        self.on = on && !self.disabled;
        self.me = me;
        if !self.on {
            self.mic.reset();
            self.system.reset();
        } else if !me {
            self.mic.reset();
        }
    }

    pub fn wants(&self, source: Source) -> bool {
        self.on && (source == Source::System || self.me)
    }

    /// True once captions switched themselves off because decoding could not keep up.
    pub fn gave_up(&self) -> bool {
        self.disabled
    }

    fn lane(&mut self, source: Source) -> &mut Lane {
        match source {
            Source::Mic => &mut self.mic,
            Source::System => &mut self.system,
        }
    }

    /// New audio for `source`. `finished` is true when the VAD just closed an utterance: the
    /// committed line takes over, so the in-progress text starts over.
    pub fn push(&mut self, source: Source, samples: &[f32], finished: bool) {
        if !self.wants(source) {
            return;
        }
        let lane = self.lane(source);
        if finished {
            lane.reset();
            lane.utt += 1;
        }
        lane.push(samples);
    }

    /// The utterance that just finished on `source` (for its final caption).
    pub fn finished_utt(&mut self, source: Source) -> u64 {
        self.lane(source).utt.saturating_sub(1)
    }

    /// Drop everything buffered (pause, resume, end).
    pub fn clear(&mut self) {
        self.mic.reset();
        self.system.reset();
    }

    fn caption(lane: &mut Lane, speaker: Speaker) -> Caption {
        Caption {
            speaker,
            text: lane.stab.live_text(),
            stable: lane.stab.settled(),
            is_final: false,
            sealed: std::mem::take(&mut lane.stab.sealed),
            utt: lane.utt,
        }
    }

    /// Decode the in-progress speech of every wanted source that is due. `remote` is the label
    /// for the system stream.
    pub fn poll(&mut self, now_ms: u64, engine: &mut dyn Engine, remote: &Speaker) -> Vec<Caption> {
        let mut out = Vec::new();
        for source in [Source::System, Source::Mic] {
            if !self.wants(source) {
                continue;
            }
            let speaker = match source {
                Source::System => remote.clone(),
                Source::Mic => Speaker::Me,
            };
            let lane = self.lane(source);
            // A pause locks whatever is left, even between decodes.
            if lane.quiet() && lane.stab.settle() {
                let c = Self::caption(lane, speaker.clone());
                lane.last_sent = (c.text.clone(), c.stable);
                out.push(c);
                continue;
            }
            if now_ms < lane.next_at_ms
                || lane.buf.len() < MIN_SAMPLES
                || lane.fresh < MIN_NEW_SAMPLES
                || lane.quiet()
            {
                continue;
            }
            let began = Instant::now();
            let text = engine.transcribe(&lane.buf).map(|segs| {
                segs.into_iter()
                    .map(|s| s.text)
                    .collect::<Vec<_>>()
                    .join(" ")
                    .trim()
                    .to_string()
            });
            let cost = began.elapsed().as_millis() as u64;
            lane.next_at_ms = now_ms + (cost * 2).clamp(BASE_INTERVAL_MS, MAX_INTERVAL_MS);
            if cost > SLOW_MS {
                self.slow += 1;
            } else {
                self.slow = 0;
            }
            let text = match text {
                Ok(t) if !t.is_empty() => tame_title_case(&t),
                _ => String::new(),
            };
            let lane = self.lane(source);
            lane.fresh = 0;
            if !text.is_empty() {
                lane.stab.update(&text, now_ms);
                let c = Self::caption(lane, speaker);
                if !c.sealed.is_empty() || (c.text.clone(), c.stable) != lane.last_sent {
                    lane.last_sent = (c.text.clone(), c.stable);
                    out.push(c);
                }
            }
            if self.slow >= SLOW_LIMIT {
                self.disabled = true;
                self.on = false;
                self.mic.reset();
                self.system.reset();
                break;
            }
        }
        out
    }
}

#[cfg(test)]
mod case_tests {
    use super::*;

    #[test]
    fn title_cased_phrase_becomes_sentence_case() {
        assert_eq!(
            tame_title_case("The Night Shift Crew"),
            "The night shift crew"
        );
        assert_eq!(
            tame_title_case("we met The Night Shift Crew today"),
            "we met the night shift crew today"
        );
    }

    #[test]
    fn names_and_acronyms_are_kept() {
        assert_eq!(tame_title_case("I flew to New York"), "I flew to New York");
        assert_eq!(
            tame_title_case("Call NASA about Tuesday"),
            "Call NASA about Tuesday"
        );
        assert_eq!(
            tame_title_case("Hello. Good Morning Everyone Here"),
            "Hello. Good morning everyone here"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprk_asr::{AsrError, Segment};

    /// Says how many seconds of audio it was given; counts calls.
    struct Counting(u32);
    impl Engine for Counting {
        fn transcribe(&mut self, s: &[f32]) -> Result<Vec<Segment>, AsrError> {
            self.0 += 1;
            Ok(vec![Segment {
                start_ms: 0,
                end_ms: 0,
                text: format!("heard {} ms", s.len() * 1000 / RATE),
            }])
        }
    }

    fn loud(ms: usize) -> Vec<f32> {
        vec![0.1; RATE * ms / 1000]
    }

    fn on(me: bool) -> Captioner {
        let mut c = Captioner::new();
        c.set_wanted(true, me);
        c
    }

    /// Answers from a script, one per decode.
    struct Script(std::collections::VecDeque<&'static str>);
    impl Engine for Script {
        fn transcribe(&mut self, _: &[f32]) -> Result<Vec<Segment>, AsrError> {
            Ok(vec![Segment {
                start_ms: 0,
                end_ms: 0,
                text: self.0.pop_front().unwrap_or("").to_string(),
            }])
        }
    }

    #[test]
    fn words_settle_once_two_decodes_agree_and_earlier_words_are_corrected() {
        let mut c = on(false);
        let mut e = Script(["i scream for", "Ice cream for the", "ice cream for the win"].into());
        let mut t = 1_000;
        let mut next = |c: &mut Captioner, e: &mut Script| {
            c.push(Source::System, &loud(500), false);
            t += 1_000;
            c.poll(t, e, &Speaker::Others)
        };
        c.push(Source::System, &loud(1_000), false);
        let first = c.poll(1_000, &mut e, &Speaker::Others);
        assert_eq!(
            (first[0].text.as_str(), first[0].stable),
            ("i scream for", 0)
        );
        // "scream" was heard as "Ice cream": the line is corrected, nothing agrees yet.
        let second = next(&mut c, &mut e);
        assert_eq!(second[0].text, "Ice cream for the");
        assert_eq!(second[0].stable, 0);
        // The third decode agrees on the first four words, case and punctuation aside.
        let third = next(&mut c, &mut e);
        assert_eq!(third[0].text, "ice cream for the win");
        assert_eq!(third[0].stable, 4);
        assert!(!third[0].is_final);
    }

    #[test]
    fn nothing_is_decoded_again_without_new_audio() {
        let mut c = on(false);
        let mut e = Counting(0);
        c.push(Source::System, &loud(1_000), false);
        assert_eq!(c.poll(1_000, &mut e, &Speaker::Others).len(), 1);
        assert!(c.poll(9_000, &mut e, &Speaker::Others).is_empty());
        assert_eq!(e.0, 1);
    }

    #[test]
    fn off_means_nothing_is_buffered_or_decoded() {
        let mut c = Captioner::new();
        let mut e = Counting(0);
        c.push(Source::System, &loud(2_000), false);
        assert!(c.poll(10_000, &mut e, &Speaker::Others).is_empty());
        assert_eq!(e.0, 0);
    }

    #[test]
    fn others_only_by_default_skips_the_mic() {
        let mut c = on(false);
        let mut e = Counting(0);
        c.push(Source::Mic, &loud(2_000), false);
        c.push(Source::System, &loud(2_000), false);
        let caps = c.poll(1_000, &mut e, &Speaker::Others);
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].speaker, Speaker::Others);
        assert_eq!(e.0, 1, "the mic stream was never decoded");
    }

    #[test]
    fn me_is_included_when_asked() {
        let mut c = on(true);
        let mut e = Counting(0);
        c.push(Source::Mic, &loud(1_000), false);
        c.push(Source::System, &loud(1_000), false);
        let caps = c.poll(1_000, &mut e, &Speaker::Others);
        assert_eq!(caps.len(), 2);
        assert!(caps.iter().any(|c| c.speaker == Speaker::Me));
    }

    #[test]
    fn decodes_at_most_every_interval_and_only_new_text() {
        let mut c = on(false);
        let mut e = Counting(0);
        c.push(Source::System, &loud(1_000), false);
        assert_eq!(c.poll(1_000, &mut e, &Speaker::Others).len(), 1);
        // Too soon: nothing, even with more audio.
        c.push(Source::System, &loud(300), false);
        assert!(c.poll(1_300, &mut e, &Speaker::Others).is_empty());
        assert_eq!(e.0, 1);
        // Due again with longer audio: new text.
        assert_eq!(c.poll(1_800, &mut e, &Speaker::Others).len(), 1);
        // Same audio length again: same text, not repeated.
        assert!(c.poll(3_000, &mut e, &Speaker::Others).is_empty());
    }

    #[test]
    fn silence_and_short_audio_are_not_decoded() {
        let mut c = on(false);
        let mut e = Counting(0);
        c.push(Source::System, &loud(300), false);
        assert!(c.poll(1_000, &mut e, &Speaker::Others).is_empty());
        c.push(Source::System, &vec![0.0; RATE * 2], false);
        assert!(c.poll(2_000, &mut e, &Speaker::Others).is_empty());
        assert_eq!(e.0, 0);
    }

    #[test]
    fn only_the_last_ten_seconds_are_decoded() {
        let mut c = on(false);
        let mut e = Counting(0);
        c.push(Source::System, &loud(15_000), false);
        let caps = c.poll(1_000, &mut e, &Speaker::Others);
        assert_eq!(caps[0].text, "heard 10000 ms");
    }

    #[test]
    fn a_finished_utterance_starts_the_caption_over() {
        let mut c = on(false);
        let mut e = Counting(0);
        c.push(Source::System, &loud(2_000), false);
        assert_eq!(c.poll(1_000, &mut e, &Speaker::Others).len(), 1);
        c.push(Source::System, &loud(1_000), true);
        let caps = c.poll(3_000, &mut e, &Speaker::Others);
        assert_eq!(caps[0].text, "heard 1000 ms");
    }

    #[test]
    fn turning_off_drops_buffered_audio() {
        let mut c = on(true);
        c.push(Source::Mic, &loud(2_000), false);
        c.set_wanted(true, false);
        let mut e = Counting(0);
        assert!(c.poll(5_000, &mut e, &Speaker::Others).is_empty());
        assert_eq!(e.0, 0);
    }

    /// Non-stop speech: each decode sees only the last six words, like a sliding window.
    struct Sliding {
        words: Vec<&'static str>,
        upto: usize,
    }
    impl Engine for Sliding {
        fn transcribe(&mut self, _: &[f32]) -> Result<Vec<Segment>, AsrError> {
            let from = self.upto.saturating_sub(6);
            Ok(vec![Segment {
                start_ms: 0,
                end_ms: 0,
                text: self.words[from..self.upto].join(" "),
            }])
        }
    }

    #[test]
    fn nonstop_speech_locks_sentences_in_order_without_repeats() {
        let words = "One two three. Four five six. Seven eight nine. Ten eleven twelve. Thirteen fourteen fifteen."
            .split(' ')
            .collect::<Vec<_>>();
        let n = words.len();
        let mut c = on(false);
        let mut e = Sliding { words, upto: 0 };
        let mut sealed: Vec<String> = Vec::new();
        let mut last_live = String::new();
        for i in 1..=n {
            e.upto = i;
            c.push(Source::System, &loud(500), false);
            for cap in c.poll(i as u64 * 1_000, &mut e, &Speaker::Others) {
                sealed.extend(cap.sealed);
                last_live = cap.text;
            }
        }
        assert_eq!(
            sealed,
            [
                "One two three.",
                "Four five six.",
                "Seven eight nine.",
                "Ten eleven twelve."
            ]
        );
        assert_eq!(last_live, "Thirteen fourteen fifteen.");
    }

    #[test]
    fn a_long_sentence_locks_by_age_and_the_live_row_stays_short() {
        let words = "a b c d e f g h i j k l m n o p q r s t"
            .split(' ')
            .collect::<Vec<_>>();
        let mut c = on(false);
        let mut e = Sliding { words, upto: 0 };
        let mut live = String::new();
        let mut stable = 0;
        for i in 1..=20 {
            e.upto = i;
            c.push(Source::System, &loud(500), false);
            for cap in c.poll(i as u64 * 1_000, &mut e, &Speaker::Others) {
                live = cap.text;
                stable = cap.stable;
            }
        }
        // Words lock one by one as they age; none is lost and none repeats.
        assert!(live.starts_with("a b c"));
        assert!(live.ends_with("t"));
        assert_eq!(live.split(' ').count(), 20);
        assert!(
            stable >= 18,
            "only the newest word or two are tentative, got {stable}"
        );
    }

    #[test]
    fn a_pause_locks_the_tail_and_seals_the_row() {
        let mut c = on(false);
        let mut e = Script(["so we begin", "so we begin here"].into());
        c.push(Source::System, &loud(1_000), false);
        c.poll(1_000, &mut e, &Speaker::Others);
        c.push(Source::System, &loud(500), false);
        c.poll(2_000, &mut e, &Speaker::Others);
        c.push(Source::System, &vec![0.0; RATE], false);
        let caps = c.poll(3_000, &mut e, &Speaker::Others);
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].sealed, ["so we begin here"]);
        assert_eq!(caps[0].text, "");
    }
}

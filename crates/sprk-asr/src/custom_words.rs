//! Custom words (SPEC 4.3a): names, drug names and product names a person or an administrator
//! lists so they come out spelled right.
//!
//! Two layers, both engine-agnostic from the pipeline's point of view:
//! - [`Engine::bias_toward`]: an engine that can take word hints (sherpa-onnx hotwords) biases
//!   its decoding toward the list. Most engines and packs cannot, and that is fine.
//! - [`Corrected`]: after every decode, near misses of a listed word are rewritten to the listed
//!   spelling. Deliberately conservative: only words of five or more letters are matched loosely,
//!   the first letter must agree, at most one letter may differ (two from ten letters up), and
//!   common English words are never changed.
//!
//! The list is PHI-grade (it may hold people's names): it is never logged and never put in an
//! error. Only its length is reported.
use crate::{AsrError, Engine, Segment};

/// Most entries kept, after cleaning. Anything past this is dropped.
pub const MAX_WORDS: usize = 500;
/// Longest entry kept, in characters.
pub const MAX_LEN: usize = 64;
/// Most words in one entry ("Charlie Mertens", "St Louis Park").
pub const MAX_PARTS: usize = 4;

/// Clean a list as typed or as set by policy: one entry per item, trimmed, inner spaces
/// collapsed, characters other than letters, digits, spaces, `'`, `-` and `.` removed, entries
/// without a letter or digit dropped, duplicates (ignoring case) dropped, and the size capped.
pub fn clean<'a>(lines: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in lines {
        let kept: String = line
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || matches!(c, '\'' | '-' | '.') {
                    c
                } else {
                    ' '
                }
            })
            .collect();
        let words: Vec<&str> = kept.split_whitespace().collect();
        if words.is_empty() || words.len() > MAX_PARTS {
            continue;
        }
        let entry = words.join(" ");
        let entry = entry.trim_matches(|c: char| !c.is_alphanumeric());
        if entry.is_empty() || entry.chars().count() > MAX_LEN {
            continue;
        }
        if out.iter().any(|e| e.to_lowercase() == entry.to_lowercase()) {
            continue;
        }
        out.push(entry.to_string());
        if out.len() == MAX_WORDS {
            break;
        }
    }
    out
}

/// The list in force: the administrator's entries first, then the person's own. Both apply.
pub fn combine(admin: &[String], user: &[String]) -> Vec<String> {
    clean(admin.iter().chain(user.iter()).map(String::as_str))
}

/// The list as word hints for sherpa-onnx (`one/two words`). Only letters, digits and plain
/// apostrophes survive: sherpa reads `/` as a separator, a word starting with `:` as a score
/// (and aborts on a bad one), and the binding panics on a NUL, so none of those can get through.
pub fn hint_phrases(words: &[String]) -> String {
    words
        .iter()
        .map(|w| {
            let kept: String = w
                .chars()
                .map(|c| match c {
                    '\u{2019}' => '\'',
                    c if c.is_alphanumeric() || c == '\'' => c,
                    _ => ' ',
                })
                .collect();
            kept.split_whitespace()
                .map(|p| p.trim_matches('\''))
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// Letters and digits only, lower case: the form two spellings are compared in.
fn key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Common English words that are never rewritten, even when one letter away from a listed
/// word. Only words of five or more letters matter: shorter ones are never matched loosely.
const COMMON: &[&str] = &[
    "about",
    "above",
    "after",
    "again",
    "against",
    "agree",
    "ahead",
    "allow",
    "almost",
    "alone",
    "along",
    "already",
    "always",
    "among",
    "another",
    "answer",
    "anyone",
    "anything",
    "around",
    "asked",
    "aware",
    "based",
    "basic",
    "because",
    "become",
    "before",
    "begin",
    "being",
    "below",
    "better",
    "between",
    "bring",
    "build",
    "called",
    "cannot",
    "careful",
    "caring",
    "cause",
    "change",
    "check",
    "clear",
    "close",
    "coming",
    "could",
    "count",
    "couple",
    "doing",
    "during",
    "early",
    "either",
    "email",
    "enough",
    "every",
    "everyone",
    "exactly",
    "fine",
    "first",
    "follow",
    "found",
    "friday",
    "front",
    "getting",
    "given",
    "going",
    "great",
    "group",
    "happen",
    "happy",
    "having",
    "heard",
    "hello",
    "helps",
    "honest",
    "hours",
    "house",
    "issue",
    "issues",
    "itself",
    "known",
    "large",
    "later",
    "leave",
    "level",
    "light",
    "little",
    "looked",
    "looking",
    "maybe",
    "means",
    "meeting",
    "might",
    "minute",
    "minutes",
    "monday",
    "money",
    "month",
    "moving",
    "needs",
    "never",
    "night",
    "nobody",
    "notes",
    "nothing",
    "number",
    "often",
    "order",
    "other",
    "others",
    "people",
    "person",
    "place",
    "point",
    "power",
    "pretty",
    "price",
    "problem",
    "quick",
    "quite",
    "rather",
    "ready",
    "really",
    "reason",
    "right",
    "saying",
    "second",
    "seems",
    "sense",
    "seven",
    "shall",
    "should",
    "since",
    "small",
    "something",
    "sorry",
    "sound",
    "speak",
    "spend",
    "start",
    "still",
    "stuff",
    "sunday",
    "super",
    "taken",
    "talking",
    "thank",
    "thanks",
    "their",
    "there",
    "these",
    "thing",
    "things",
    "think",
    "those",
    "three",
    "through",
    "today",
    "together",
    "tomorrow",
    "total",
    "trying",
    "tuesday",
    "under",
    "until",
    "usually",
    "using",
    "value",
    "wanted",
    "water",
    "wednesday",
    "week",
    "weeks",
    "where",
    "which",
    "while",
    "whole",
    "would",
    "write",
    "wrong",
    "years",
    "yesterday",
    "young",
    "yours",
];

/// Possessive endings kept as heard when the word before them is corrected.
const POSSESSIVE: &[&str] = &["'s", "\u{2019}s"];

fn is_common(k: &str) -> bool {
    COMMON.contains(&k)
}

/// Letters that may differ for a loose match, by the listed word's length (letters and digits).
fn allowed_edits(len: usize) -> usize {
    match len {
        0..=4 => 0,
        5..=9 => 1,
        _ => 2,
    }
}

/// Levenshtein distance, stopping early once it passes `cap`.
fn distance(a: &[char], b: &[char], cap: usize) -> usize {
    if a.len().abs_diff(b.len()) > cap {
        return cap + 1;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        let mut row_min = cur[0];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
            row_min = row_min.min(cur[j + 1]);
        }
        if row_min > cap {
            return cap + 1;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

struct Term {
    /// As listed: the spelling written into the transcript.
    text: String,
    /// `key` of each word.
    parts: Vec<String>,
    /// `key` of the whole entry.
    joined: Vec<char>,
    /// Has a capital letter: use it as listed. Otherwise follow the heard capitalization.
    cased: bool,
}

/// One word of decoded text, split into punctuation around it and the word itself.
struct Word {
    lead: String,
    core: String,
    trail: String,
    /// Whitespace after it in the original text.
    space: String,
}

fn split_words(text: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let mut rest = text;
    let lead_ws: String = rest.chars().take_while(|c| c.is_whitespace()).collect();
    rest = &rest[lead_ws.len()..];
    if !lead_ws.is_empty() {
        out.push(Word {
            lead: String::new(),
            core: String::new(),
            trail: String::new(),
            space: lead_ws,
        });
    }
    while !rest.is_empty() {
        let chunk_len = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let chunk = &rest[..chunk_len];
        rest = &rest[chunk_len..];
        let ws_len = rest
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(rest.len());
        let space = rest[..ws_len].to_string();
        rest = &rest[ws_len..];
        let start = chunk.find(char::is_alphanumeric);
        let end = chunk
            .char_indices()
            .rev()
            .find(|(_, c)| c.is_alphanumeric())
            .map(|(i, c)| i + c.len_utf8());
        match (start, end) {
            (Some(s), Some(e)) => out.push(Word {
                lead: chunk[..s].to_string(),
                core: chunk[s..e].to_string(),
                trail: chunk[e..].to_string(),
                space,
            }),
            _ => out.push(Word {
                lead: chunk.to_string(),
                core: String::new(),
                trail: String::new(),
                space,
            }),
        }
    }
    out
}

/// Rewrites near misses of listed words to the listed spelling. Cheap enough to run on every
/// decode, live captions included.
pub struct Corrector {
    terms: Vec<Term>,
}

impl Corrector {
    /// `words` should already be [`clean`]ed.
    pub fn new(words: &[String]) -> Self {
        let terms = words
            .iter()
            .filter_map(|w| {
                let parts: Vec<String> = w.split_whitespace().map(key).collect();
                let joined: Vec<char> = parts.concat().chars().collect();
                (!joined.is_empty()).then(|| Term {
                    text: w.clone(),
                    parts,
                    joined,
                    cased: w.chars().any(char::is_uppercase),
                })
            })
            .collect();
        Self { terms }
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// The best listed word for the `k` words starting at `words[i]`, if one fits.
    /// Returns (term index, distance, possessive ending split off the last word).
    fn best(&self, words: &[Word], i: usize, k: usize) -> Option<(usize, usize, &'static str)> {
        let window = &words[i..i + k];
        if window.iter().any(|w| w.core.is_empty()) {
            return None;
        }
        // Punctuation between the words (a comma, a full stop) means they are not one name.
        if window[..k - 1]
            .iter()
            .any(|w| !w.trail.is_empty() || w.space.is_empty())
            || window[1..].iter().any(|w| !w.lead.is_empty())
        {
            return None;
        }
        let last = &window[k - 1].core;
        let ending = POSSESSIVE
            .iter()
            .copied()
            .find(|e| last.len() > e.len() && last.ends_with(e))
            .unwrap_or("");
        let mut best: Option<(usize, usize, &'static str)> = None;
        for (ti, t) in self.terms.iter().enumerate() {
            let n = t.parts.len();
            // "Martens's" is compared without its 's, unless the listed word has one itself.
            let ending = if POSSESSIVE.iter().any(|e| t.text.ends_with(e)) {
                ""
            } else {
                ending
            };
            let mut keys: Vec<String> = window.iter().map(|w| key(&w.core)).collect();
            if !ending.is_empty() {
                if let Some(l) = keys.last_mut() {
                    l.pop();
                }
            }
            let joined: Vec<char> = keys.concat().chars().collect();
            let d = if joined == t.joined {
                // Same letters. One word that differs only in capitals is left alone.
                if k == 1 && n == 1 {
                    let core = &window[0].core;
                    let heard = &core[..core.len() - ending.len()];
                    if heard.to_lowercase() == t.text.to_lowercase() {
                        continue;
                    }
                }
                // Words run together or split apart: only for listed words of four or more.
                if k != n && t.joined.len() < 4 {
                    continue;
                }
                0
            } else {
                // Loose matches only word for word, never across a join or a split.
                if k != n {
                    continue;
                }
                let cap = allowed_edits(t.joined.len());
                if cap == 0 || joined.len() < 4 || joined.first() != t.joined.first() {
                    continue;
                }
                if keys.iter().any(|k| k.chars().any(|c| c.is_ascii_digit())) {
                    continue;
                }
                if k == 1 && is_common(&keys[0]) {
                    continue;
                }
                if k > 1 {
                    // Each word starts the same, and at least one is heard exactly.
                    let starts = keys
                        .iter()
                        .zip(&t.parts)
                        .all(|(a, b)| a.chars().next() == b.chars().next());
                    if !starts || !keys.iter().zip(&t.parts).any(|(a, b)| a == b) {
                        continue;
                    }
                }
                let d = distance(&joined, &t.joined, cap);
                if d > cap {
                    continue;
                }
                d
            };
            if best.is_none_or(|(_, bd, _)| d < bd) {
                best = Some((ti, d, ending));
            }
        }
        best
    }

    /// `text` with near misses rewritten. Text with nothing to fix comes back unchanged.
    pub fn fix(&self, text: &str) -> String {
        if self.terms.is_empty() || text.is_empty() {
            return text.to_string();
        }
        let words = split_words(text);
        let max_k = self.terms.iter().map(|t| t.parts.len()).max().unwrap_or(1) + 1;
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while i < words.len() {
            let mut hit: Option<(usize, usize, usize, &str)> = None;
            for k in (1..=max_k.min(words.len() - i)).rev() {
                if let Some((ti, d, p)) = self.best(&words, i, k) {
                    if hit.is_none_or(|(_, _, hd, _)| d < hd) {
                        hit = Some((k, ti, d, p));
                    }
                }
            }
            match hit {
                Some((k, ti, _, ending)) => {
                    let t = &self.terms[ti];
                    let first = &words[i];
                    let last = &words[i + k - 1];
                    out.push_str(&first.lead);
                    out.push_str(&styled(&t.text, t.cased, &first.core));
                    out.push_str(ending);
                    out.push_str(&last.trail);
                    out.push_str(&last.space);
                    i += k;
                }
                None => {
                    let w = &words[i];
                    out.push_str(&w.lead);
                    out.push_str(&w.core);
                    out.push_str(&w.trail);
                    out.push_str(&w.space);
                    i += 1;
                }
            }
        }
        out
    }
}

/// The listed spelling, capitalized like the heard word when the list has no capitals of its own
/// ("metoprolol" at the start of a sentence becomes "Metoprolol").
fn styled(term: &str, cased: bool, heard: &str) -> String {
    if cased || !heard.chars().next().is_some_and(char::is_uppercase) {
        return term.to_string();
    }
    let mut c = term.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// An engine whose text goes through a [`Corrector`]. Wraps any engine, so the pipeline never
/// knows which one it has.
pub struct Corrected {
    inner: Box<dyn Engine>,
    fix: Corrector,
}

impl Corrected {
    pub fn new(inner: Box<dyn Engine>, words: &[String]) -> Self {
        Self {
            inner,
            fix: Corrector::new(words),
        }
    }
}

impl Engine for Corrected {
    fn transcribe(&mut self, samples_16k_mono: &[f32]) -> Result<Vec<Segment>, AsrError> {
        let mut segs = self.inner.transcribe(samples_16k_mono)?;
        for s in &mut segs {
            s.text = self.fix.fix(&s.text);
        }
        Ok(segs)
    }
    fn bias_toward(&mut self, phrases: &[String]) -> bool {
        self.inner.bias_toward(phrases)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixer(words: &[&str]) -> Corrector {
        Corrector::new(&clean(words.iter().copied()))
    }

    #[test]
    fn metoprolol_style_misspellings_are_fixed() {
        let f = fixer(&["metoprolol"]);
        for heard in [
            "metoprolo",
            "metaprolol",
            "metroprolol",
            "metoprolal",
            "metoprolols",
        ] {
            assert_eq!(
                f.fix(&format!("Start {heard} today.")),
                "Start metoprolol today."
            );
        }
        // Two letters off is still fine for a ten-letter word.
        assert_eq!(f.fix("on metaprolal now"), "on metoprolol now");
        // At the start of a sentence it keeps the capital.
        assert_eq!(f.fix("Metaprolol, 25 mg."), "Metoprolol, 25 mg.");
        // Split into pieces by the speech model.
        assert_eq!(f.fix("take meto prolol daily"), "take metoprolol daily");
    }

    #[test]
    fn listed_spelling_and_capitals_win_for_names() {
        let f = fixer(&["Mertens", "Lifespark", "Charlie Mertens"]);
        assert_eq!(f.fix("ask martens about it"), "ask Mertens about it");
        assert_eq!(f.fix("welcome to life spark."), "welcome to Lifespark.");
        assert_eq!(f.fix("Lifesparc staff"), "Lifespark staff");
        assert_eq!(f.fix("Charlie Martens said so"), "Charlie Mertens said so");
        // A possessive keeps its 's.
        assert_eq!(f.fix("that was Martens's call"), "that was Mertens's call");
    }

    #[test]
    fn ordinary_words_are_never_changed() {
        let f = fixer(&["metoprolol", "Lifespark", "Mertens", "Lasix", "Haldol"]);
        let plain = "So I think the lifespan of the meeting notes is about seven days, \
                     and there were others there too. Lots of people, really.";
        assert_eq!(f.fix(plain), plain);
        // Common words one letter from a listed word stay as they are.
        let f = fixer(&["Thinks", "Theirs", "Others", "Peoples"]);
        let s = "I think their others and people agree.";
        assert_eq!(f.fix(s), s);
    }

    #[test]
    fn a_realistic_list_leaves_ordinary_meeting_talk_alone() {
        let f = fixer(&[
            "metoprolol",
            "lisinopril",
            "atorvastatin",
            "Eliquis",
            "Lifespark",
            "Charlie Mertens",
            "Mertens",
            "Powers",
            "Freshservice",
            "NinjaOne",
            "Okta",
            "Eldermark",
            "Medicare",
            "hospice",
            "Lasix",
        ]);
        let talk = "Okay, so the meeting today is about the medical team and their methods. \
            Members asked whether mentors could monitor the new lifestyle program. \
            The hospital said Medicare covers the elder market, and new tickets \
            go to the ninja team. Power users want more power, and the towers are fine. \
            Our metrics look good; we should eliminate the backlog and elicit feedback. \
            Charlie said the lists are ready, and the last item is lasers.";
        assert_eq!(f.fix(talk), talk);
        // The one designed exception: ordinary words that spell a listed name exactly when run
        // together are joined, because speech models often split product names this way.
        assert_eq!(f.fix("a fresh service ticket"), "a Freshservice ticket");
    }

    #[test]
    fn short_words_are_only_fixed_exactly() {
        // Four letters or fewer: never a loose match, so "Ana" does not swallow "and".
        let f = fixer(&["Ana", "Xa", "Tums"]);
        let s = "and then a tum and tums";
        assert_eq!(f.fix(s), s);
        // Capitals alone are not "fixed" either: "may" stays "may" with "May" listed.
        let f = fixer(&["May"]);
        assert_eq!(f.fix("you may go"), "you may go");
    }

    #[test]
    fn first_letter_must_agree() {
        let f = fixer(&["Lisinopril"]);
        assert_eq!(f.fix("on lisinopryl"), "on Lisinopril");
        assert_eq!(f.fix("on risinopril"), "on risinopril");
    }

    #[test]
    fn punctuation_and_spacing_survive() {
        let f = fixer(&["metoprolol"]);
        assert_eq!(
            f.fix("  \"metaprolol?\"  yes,metaprolol"),
            "  \"metoprolol?\"  yes,metaprolol"
        );
        assert_eq!(f.fix(""), "");
        // Words split by a comma are not joined into a name.
        let f = fixer(&["Lifespark"]);
        assert_eq!(f.fix("life, spark"), "life, spark");
    }

    #[test]
    fn exact_text_is_left_alone() {
        let f = fixer(&["metoprolol", "E. coli"]);
        assert_eq!(f.fix("Metoprolol works."), "Metoprolol works.");
        assert_eq!(f.fix("tested for e coli"), "tested for E. coli");
    }

    #[test]
    fn numbers_are_not_loosely_matched() {
        let f = fixer(&["A1C level"]);
        assert_eq!(f.fix("the a2c level"), "the a2c level");
        assert_eq!(f.fix("the A1C level"), "the A1C level");
    }

    #[test]
    fn non_latin_text_is_safe() {
        let f = fixer(&["Müller", "metoprolol"]);
        assert_eq!(f.fix("Herr Müler kommt"), "Herr Müller kommt");
        assert_eq!(f.fix("日本語 テキスト"), "日本語 テキスト");
        assert_eq!(f.fix("café’s metaprolol’s"), "café’s metoprolol’s");
    }

    #[test]
    fn clean_trims_dedupes_and_caps() {
        let got = clean([
            "  metoprolol ",
            "Metoprolol",
            "",
            "   ",
            "#$%",
            "Charlie   Mertens",
            "a/b",
            "one two three four five",
            ":)",
        ]);
        assert_eq!(got, vec!["metoprolol", "Charlie Mertens", "a b"]);
        let many: Vec<String> = (0..MAX_WORDS + 20).map(|i| format!("word{i}")).collect();
        assert_eq!(clean(many.iter().map(String::as_str)).len(), MAX_WORDS);
        let long = "x".repeat(MAX_LEN + 1);
        assert!(clean([long.as_str()]).is_empty());
    }

    #[test]
    fn hint_phrases_are_safe_for_sherpa() {
        let words: Vec<String> = [
            "metoprolol",
            "E. coli",
            "a/b",
            ":5 boost",
            "#tag @me",
            "nul\0byte",
            "McDonald\u{2019}s",
            "'quoted'",
            "...",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let h = hint_phrases(&words);
        assert_eq!(
            h,
            "metoprolol/E coli/a b/5 boost/tag me/nul byte/McDonald's/quoted"
        );
        assert!(!h.contains('\0') && !h.contains(':') && !h.contains('#'));
        assert_eq!(hint_phrases(&[]), "");
    }

    #[test]
    fn admin_and_user_lists_combine() {
        let admin = vec!["Lifespark".to_string(), "metoprolol".to_string()];
        let user = vec!["Metoprolol".to_string(), "Mertens".to_string()];
        assert_eq!(
            combine(&admin, &user),
            vec!["Lifespark", "metoprolol", "Mertens"]
        );
        assert!(combine(&[], &[]).is_empty());
    }

    #[test]
    fn wrapper_fixes_every_segment_and_passes_hints_on() {
        struct Hints(bool);
        impl Engine for Hints {
            fn transcribe(&mut self, _: &[f32]) -> Result<Vec<Segment>, AsrError> {
                Ok(vec![Segment {
                    start_ms: 0,
                    end_ms: 10,
                    text: "give metaprolol".into(),
                }])
            }
            fn bias_toward(&mut self, p: &[String]) -> bool {
                self.0 = !p.is_empty();
                self.0
            }
        }
        let words = clean(["metoprolol"]);
        let mut e = Corrected::new(Box::new(Hints(false)), &words);
        assert!(e.bias_toward(&words));
        let out = e.transcribe(&[0.0; 16]).unwrap_or_default();
        assert_eq!(out[0].text, "give metoprolol");
        // The default engine takes no hints.
        let mut fake = crate::FakeEngine { script: vec![] };
        assert!(!fake.bias_toward(&words));
    }
}

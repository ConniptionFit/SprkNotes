//! Plain-code checks on what the model says (ADR-009 grounding). The model proposes; this module
//! decides what the user gets to see. Nothing here trusts the model's own claims.
use crate::view::Line;
use std::collections::{BTreeSet, HashMap};

/// Line ids written as `[L12]`, `[L12, L15]`, `[L12][L15]` or a bare `L12`, in order, no repeats.
pub fn parse_citations(text: &str) -> Vec<u64> {
    let bytes: Vec<char> = text.chars().collect();
    let mut out: Vec<u64> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let boundary = i == 0 || !bytes[i - 1].is_alphanumeric();
        if (c == 'L' || c == 'l') && boundary {
            let mut j = i + 1;
            let mut n = String::new();
            while j < bytes.len() && bytes[j].is_ascii_digit() && n.len() < 9 {
                n.push(bytes[j]);
                j += 1;
            }
            let ends_clean = j >= bytes.len() || !bytes[j].is_alphanumeric();
            if !n.is_empty() && ends_clean {
                if let Ok(id) = n.parse::<u64>() {
                    if !out.contains(&id) {
                        out.push(id);
                    }
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Removes citation marks like `[L12]` or `[L12, L15]` from visible text.
pub fn strip_citations(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        match after.find(']') {
            Some(close) => {
                let inner = &after[..close];
                let is_cite = !inner.is_empty()
                    && inner
                        .split(|c: char| c == ',' || c.is_whitespace())
                        .filter(|p| !p.is_empty())
                        .all(|p| {
                            let p = p.trim_start_matches(['L', 'l']);
                            !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())
                        });
                out.push_str(&rest[..open]);
                if !is_cite {
                    out.push_str(&rest[open..open + 1 + close + 1]);
                }
                rest = &rest[open + 1 + close + 1..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

const WEEKDAYS: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];
const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];
/// Capitalized words that are not names.
const NOT_NAMES: [&str; 22] = [
    "i", "the", "a", "an", "we", "you", "he", "she", "they", "it", "this", "that", "me", "others",
    "action", "owner", "due", "not", "stated", "unknown", "yes", "no",
];

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(|w| w.trim_matches('\'').to_lowercase())
        .collect()
}

/// A capitalized word at the start of a sentence is only checked as a name when a verb that
/// people do (will, asked, needs...) follows it: "Marcus asked" names an actor, "Budget review"
/// does not. Keeps real summaries from being dropped for starting with an ordinary word.
const ACTOR_VERBS: [&str; 24] = [
    "will",
    "would",
    "asked",
    "said",
    "says",
    "needs",
    "need",
    "wants",
    "want",
    "can",
    "could",
    "should",
    "must",
    "is",
    "was",
    "has",
    "agreed",
    "confirmed",
    "owns",
    "to",
    "promised",
    "suggested",
    "mentioned",
    "noted",
];

/// Things a claim asserts that the source must also contain: numbers, weekdays, months, and
/// capitalized words that look like names.
pub fn checkable_facts(claim: &str) -> BTreeSet<String> {
    let tokens: Vec<&str> = claim.split_whitespace().collect();
    let clean =
        |raw: &str| -> String { raw.trim_matches(|c: char| !c.is_alphanumeric()).to_string() };
    let mut facts = BTreeSet::new();
    let mut sentence_start = true;
    for (i, raw) in tokens.iter().enumerate() {
        let word = clean(raw);
        let ends_sentence = raw.ends_with(['.', '!', '?', ':']);
        if word.is_empty() {
            sentence_start = ends_sentence || sentence_start;
            continue;
        }
        let lower = word.to_lowercase();
        if word.chars().any(|c| c.is_ascii_digit()) {
            // "3pm" and "15th" check the number part; "10:30" is split into 10 and 30.
            for part in raw.split(|c: char| !c.is_ascii_digit()) {
                if !part.is_empty() {
                    facts.insert(part.trim_start_matches('0').to_string());
                }
            }
        } else if WEEKDAYS.contains(&lower.as_str()) || MONTHS.contains(&lower.as_str()) {
            facts.insert(lower);
        } else if word.chars().next().is_some_and(char::is_uppercase)
            && !NOT_NAMES.contains(&lower.as_str())
            && word.chars().count() > 1
        {
            let next = tokens.get(i + 1).map(|t| clean(t).to_lowercase());
            let actor = next.is_some_and(|n| ACTOR_VERBS.contains(&n.as_str()));
            if !sentence_start || actor {
                facts.insert(lower);
            }
        }
        sentence_start = ends_sentence;
    }
    facts.remove("");
    facts
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reject {
    NoCitation,
    UnknownLine,
    UnsupportedFact(String),
}

/// Looks lines up by id.
pub struct Evidence<'a> {
    by_id: HashMap<u64, &'a Line>,
    /// Names that may appear without being in a cited line (the Participants section).
    pub participants: Vec<String>,
}

impl<'a> Evidence<'a> {
    pub fn new(lines: &'a [Line], participants: &[String]) -> Self {
        Self {
            by_id: lines.iter().map(|l| (l.id, l)).collect(),
            participants: participants.iter().map(|p| p.to_lowercase()).collect(),
        }
    }

    /// `provided` is the set of ids the model was shown: citing anything else is rejected.
    pub fn check(&self, claim: &str, cited: &[u64], provided: &[u64]) -> Result<(), Reject> {
        if cited.is_empty() {
            return Err(Reject::NoCitation);
        }
        let mut source = String::new();
        for id in cited {
            if !provided.contains(id) {
                return Err(Reject::UnknownLine);
            }
            let Some(l) = self.by_id.get(id) else {
                return Err(Reject::UnknownLine);
            };
            source.push(' ');
            source.push_str(&l.text);
            source.push(' ');
            source.push_str(&l.speaker);
        }
        let source_words: BTreeSet<String> = words(&source).into_iter().collect();
        let source_numbers: BTreeSet<String> = source
            .split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .map(|p| p.trim_start_matches('0').to_string())
            .collect();
        let participant_words: BTreeSet<String> =
            self.participants.iter().flat_map(|p| words(p)).collect();
        for fact in checkable_facts(&strip_citations(claim)) {
            let numeric = fact.chars().all(|c| c.is_ascii_digit());
            let found = if numeric {
                source_numbers.contains(&fact) || number_word_matches(&fact, &source_words)
            } else {
                let singular = fact.strip_suffix('s').unwrap_or(&fact).to_string();
                let plural = format!("{fact}s");
                source_words.contains(&fact)
                    || source_words.contains(&singular)
                    || source_words.contains(&plural)
                    || participant_words.contains(&fact)
            };
            if !found {
                return Err(Reject::UnsupportedFact(fact));
            }
        }
        Ok(())
    }
}

/// "fifteenth" in the source supports "15" in the claim, and so on for small numbers.
fn number_word_matches(n: &str, source: &BTreeSet<String>) -> bool {
    const WORDS: [(&str, &[&str]); 12] = [
        ("1", &["one", "first"]),
        ("2", &["two", "second"]),
        ("3", &["three", "third"]),
        ("4", &["four", "fourth"]),
        ("5", &["five", "fifth"]),
        ("6", &["six", "sixth"]),
        ("7", &["seven", "seventh"]),
        ("8", &["eight", "eighth"]),
        ("9", &["nine", "ninth"]),
        ("10", &["ten", "tenth"]),
        ("15", &["fifteen", "fifteenth"]),
        ("30", &["thirty", "thirtieth"]),
    ];
    WORDS
        .iter()
        .find(|(d, _)| *d == n)
        .is_some_and(|(_, ws)| ws.iter().any(|w| source.contains(*w)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines() -> Vec<Line> {
        let mk = |id, who: &str, text: &str| Line {
            id,
            t_ms: id * 10_000,
            speaker: who.into(),
            text: text.into(),
        };
        vec![
            mk(1, "Others", "Finance needs the numbers by Friday."),
            mk(2, "Me", "I will send them to Dana on the fifteenth."),
            mk(3, "Others", "The vendor call is at 10:30 on Tuesday."),
            mk(4, "Others", "Let's talk about lunch."),
        ]
    }

    #[test]
    fn citations_parse_in_several_shapes() {
        assert_eq!(parse_citations("Done [L12]."), vec![12]);
        assert_eq!(parse_citations("[L3, L7] and L9, [L3]"), vec![3, 7, 9]);
        assert_eq!(
            parse_citations("no ids here, Lola 5, ALL1"),
            Vec::<u64>::new()
        );
        assert_eq!(parse_citations("[l4]"), vec![4]);
    }

    #[test]
    fn citation_marks_are_stripped_but_other_brackets_stay() {
        assert_eq!(
            strip_citations("Send it [L2] today [L3, L4]."),
            "Send it today ."
        );
        assert_eq!(strip_citations("Use [the form] [L1]"), "Use [the form]");
    }

    #[test]
    fn facts_are_numbers_days_months_and_midsentence_names() {
        let f = checkable_facts("Dana will send the numbers by Friday 15 June to Acme.");
        for want in ["dana", "friday", "15", "june", "acme"] {
            assert!(f.contains(want), "missing {want}: {f:?}");
        }
        // An ordinary first word is not a name; one that acts like a person is checked.
        assert!(!checkable_facts("Budget review is ready.").contains("budget"));
        assert!(checkable_facts("Marcus asked about it.").contains("marcus"));
    }

    #[test]
    fn supported_claims_pass_and_unsupported_facts_fail() {
        let l = lines();
        let e = Evidence::new(&l, &["Dana Sample".to_string()]);
        let provided = [1, 2, 3, 4];
        assert_eq!(e.check("Numbers are due Friday", &[1], &provided), Ok(()));
        // "Dana" is in the cited line and the fifteenth supports 15.
        assert_eq!(e.check("Send to Dana by the 15th", &[2], &provided), Ok(()));
        assert_eq!(
            e.check("Numbers are due Thursday", &[1], &provided),
            Err(Reject::UnsupportedFact("thursday".into()))
        );
        assert_eq!(
            e.check("The call is at 10:45", &[3], &provided),
            Err(Reject::UnsupportedFact("45".into()))
        );
        assert_eq!(
            e.check("The call is at 10:30 Tuesday", &[3], &provided),
            Ok(())
        );
    }

    #[test]
    fn names_may_come_from_participants_and_unknown_names_fail() {
        let l = lines();
        let e = Evidence::new(&l, &["Priya Sample".to_string()]);
        let provided = [1, 2, 3, 4];
        assert_eq!(
            e.check("Priya asked for the numbers", &[1], &provided),
            Ok(())
        );
        assert!(matches!(
            e.check("Marcus asked for the numbers", &[1], &provided),
            Err(Reject::UnsupportedFact(_))
        ));
    }

    #[test]
    fn missing_or_foreign_citations_are_rejected() {
        let l = lines();
        let e = Evidence::new(&l, &[]);
        assert_eq!(e.check("Anything", &[], &[1]), Err(Reject::NoCitation));
        assert_eq!(e.check("Anything", &[2], &[1]), Err(Reject::UnknownLine));
        assert_eq!(e.check("Anything", &[99], &[99]), Err(Reject::UnknownLine));
    }
}

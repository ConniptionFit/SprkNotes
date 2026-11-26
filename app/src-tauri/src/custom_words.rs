//! Custom words in Settings (SPEC 4.3a): the person's own list, kept in `settings.json`, plus
//! the administrator's `CustomWords` policy list. Both apply. The words are PHI-grade (they may
//! be people's names): they are never logged and never put in an error. The only diagnostic is
//! what the last session did with them, as a count and a yes or no, in memory.
use crate::save_location::{load, store};
use serde::Serialize;
use sprk_asr::custom_words::{clean, combine, Corrected};
use sprk_asr::Engine;
use std::sync::Mutex;

/// What the last session did with the list. Counts only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastSession {
    /// Entries in force (the administrator's and the person's together).
    pub words: usize,
    /// The speech model took them as hints too, not only as spelling fixes.
    pub hints: bool,
}

static LAST: Mutex<Option<LastSession>> = Mutex::new(None);

/// The list in force for the next session.
pub fn in_force(policy: &sprk_config::AppPolicy) -> Vec<String> {
    let user = load().custom_words;
    combine(&policy.custom_words, &user)
}

/// Session start: offer the list to the engine as hints, and fix near misses in everything it
/// decodes (lines, the live line and captions alike). With no list the engine is unchanged.
pub fn apply(mut engine: Box<dyn Engine>, words: &[String]) -> (Box<dyn Engine>, LastSession) {
    let hints = !words.is_empty() && engine.bias_toward(words);
    let last = LastSession {
        words: words.len(),
        hints,
    };
    if words.is_empty() {
        return (engine, last);
    }
    (Box::new(Corrected::new(engine, words)), last)
}

pub fn note(last: LastSession) {
    if let Ok(mut l) = LAST.lock() {
        *l = Some(last);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// The person's own entries, as saved.
    pub user: Vec<String>,
    /// Entries the administrator added (read-only in Settings).
    pub admin: Vec<String>,
    /// What the last session in this run of the app did, if one started.
    pub last: Option<LastSession>,
}

#[tauri::command]
pub fn custom_words_status() -> Status {
    let policy = crate::policy();
    Status {
        user: load().custom_words,
        admin: clean(policy.custom_words.iter().map(String::as_str)),
        last: LAST.lock().ok().and_then(|l| *l),
    }
}

/// Save the person's list (one entry per item; blank and duplicate lines are dropped).
/// Takes effect when the next session starts.
#[tauri::command]
pub fn custom_words_set(words: Vec<String>) -> Result<Status, String> {
    let mut saved = load();
    saved.custom_words = clean(words.iter().map(String::as_str));
    store(&saved)?;
    Ok(custom_words_status())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprk_asr::{AsrError, Segment};

    struct Fixed {
        hints: bool,
    }
    impl Engine for Fixed {
        fn transcribe(&mut self, _: &[f32]) -> Result<Vec<Segment>, AsrError> {
            Ok(vec![Segment {
                start_ms: 0,
                end_ms: 1,
                text: "start metaprolol".into(),
            }])
        }
        fn bias_toward(&mut self, _: &[String]) -> bool {
            self.hints
        }
    }

    #[test]
    fn no_list_leaves_the_engine_alone() {
        let (mut e, last) = apply(Box::new(Fixed { hints: true }), &[]);
        assert_eq!(
            last,
            LastSession {
                words: 0,
                hints: false
            }
        );
        let text = e
            .transcribe(&[])
            .map(|s| s[0].text.clone())
            .unwrap_or_default();
        assert_eq!(text, "start metaprolol");
    }

    #[test]
    fn a_list_fixes_text_and_reports_whether_hints_took() {
        let words = clean(["metoprolol", "Mertens"]);
        for hints in [false, true] {
            let (mut e, last) = apply(Box::new(Fixed { hints }), &words);
            assert_eq!(last, LastSession { words: 2, hints });
            let text = e
                .transcribe(&[])
                .map(|s| s[0].text.clone())
                .unwrap_or_default();
            assert_eq!(text, "start metoprolol");
        }
    }
}

//! Shared plumbing for the features: one prompt in, the whole reply out.
use crate::engine::{AssistantEngine, EngineError, GenRequest, RunControl};
use crate::prompt::PromptFormat;

/// Loads the model if needed, sends one system and user message, returns the reply text.
pub fn complete(
    engine: &mut dyn AssistantEngine,
    control: &dyn RunControl,
    format: PromptFormat,
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<String, EngineError> {
    if !engine.is_loaded() {
        engine.load()?;
    }
    let req = GenRequest {
        prompt: format.build(system, user),
        max_tokens,
        temperature: 0.1,
        stop: format.stop(),
    };
    let mut out = String::new();
    engine.generate(&req, control, &mut |t| out.push_str(t))?;
    Ok(out)
}

/// The list lines of a reply: `-`, `*`, a bullet, or `1.` / `1)` markers, with the marker removed.
pub fn list_items(reply: &str) -> Vec<String> {
    reply
        .lines()
        .filter_map(|l| {
            let t = l.trim();
            let t = t
                .strip_prefix(['-', '*', '\u{2022}'])
                .map(str::trim)
                .or_else(|| {
                    let digits = t.chars().take_while(char::is_ascii_digit).count();
                    (digits > 0 && digits <= 2)
                        .then(|| t[digits..].strip_prefix(['.', ')']).map(str::trim))
                        .flatten()
                })?;
            (!t.is_empty()).then(|| t.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_items_accepts_common_markers() {
        let r = "Here you go:\n- one [L1]\n* two\n\u{2022} three\n1. four\n2) five\nplain line\n10. ten";
        assert_eq!(
            list_items(r),
            vec!["one [L1]", "two", "three", "four", "five", "ten"]
        );
        assert!(list_items("nothing to list").is_empty());
    }
}

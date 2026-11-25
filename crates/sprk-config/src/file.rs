//! Admin settings that do not come from separate registry values:
//! - a packed list (`Key=Value;Key=Value`), what the installer's `POLICY` and `DEFAULTS`
//!   switches write into one registry value (`Settings`) each;
//! - `%ProgramData%\SprkNotes\policy.json`, a value and a lock flag per setting (read only when
//!   [`crate::trust`] finds that ordinary users cannot change it);
//! - `%VARIABLE%` expansion so one value works for every user (`%OneDrive%\Transcripts`).
use crate::PolicyStore;
use std::collections::HashMap;

/// Settings keyed case-insensitively (registry names are case-insensitive too). Multi-value
/// settings hold their items separated by `\n`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FilePolicy(HashMap<String, String>);

impl FilePolicy {
    pub fn from_pairs<I: IntoIterator<Item = (String, String)>>(pairs: I) -> Self {
        Self(
            pairs
                .into_iter()
                .map(|(k, v)| (k.trim().to_ascii_lowercase(), v))
                .filter(|(k, _)| !k.is_empty())
                .collect(),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The two halves of a `policy.json`: locked values (policy) and starting values (defaults).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PolicyFile {
    pub locked: FilePolicy,
    pub defaults: FilePolicy,
}

/// Parse `policy.json`:
/// `{"settings": {"TranscriptsRoot": {"value": "%OneDrive%\\Notes", "locked": true},
///                "Theme": {"value": "dark", "locked": false}}}`.
/// A bare value (`"AssistantEnabled": false`) counts as locked. Booleans become `1`/`0`, numbers
/// their text, arrays list items. Anything unreadable is simply empty: a broken file must never
/// stop the app.
pub fn policy_file(text: &str) -> PolicyFile {
    let text = text.trim_start_matches('\u{feff}');
    let Ok(serde_json::Value::Object(root)) = serde_json::from_str::<serde_json::Value>(text)
    else {
        return PolicyFile::default();
    };
    let Some(serde_json::Value::Object(map)) = root.get("settings") else {
        return PolicyFile::default();
    };
    let mut locked = Vec::new();
    let mut defaults = Vec::new();
    for (k, v) in map {
        let (value, lock) = match v {
            serde_json::Value::Object(o) => (
                o.get("value"),
                o.get("locked")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true),
            ),
            other => (Some(other), true),
        };
        if let Some(t) = value.and_then(scalar_text) {
            if lock {
                locked.push((k.clone(), t));
            } else {
                defaults.push((k.clone(), t));
            }
        }
    }
    PolicyFile {
        locked: FilePolicy::from_pairs(locked),
        defaults: FilePolicy::from_pairs(defaults),
    }
}

fn scalar_text(v: &serde_json::Value) -> Option<String> {
    use serde_json::Value::*;
    match v {
        String(s) => Some(s.clone()),
        Bool(b) => Some(if *b { "1" } else { "0" }.to_string()),
        Number(n) => Some(n.to_string()),
        Array(items) => {
            let parts: Vec<std::string::String> = items.iter().filter_map(scalar_text).collect();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        _ => None,
    }
}

/// Parse `Key=Value;Key=Value`. Inside a value `|` separates the items of a multi-value
/// setting. Pairs without `=` are skipped; spaces around keys are ignored.
pub fn packed(text: &str) -> FilePolicy {
    FilePolicy::from_pairs(text.split(';').filter_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        Some((k.to_string(), v.trim().replace('|', "\n")))
    }))
}

impl PolicyStore for FilePolicy {
    fn get(&self, key: &str) -> Option<String> {
        self.0.get(&key.to_ascii_lowercase()).cloned()
    }
    fn get_multi(&self, key: &str) -> Vec<String> {
        self.get(key)
            .map(|v| {
                v.split('\n')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Expand `%NAME%` with `lookup` (the current user's environment in the app). An unknown name or
/// a lone `%` is kept as written, so a mistake shows up in the path instead of vanishing.
pub fn expand_env(text: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end)
                if end > 0
                    && after[..end]
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'(' | b')')) =>
            {
                let name = &after[..end];
                match lookup(name) {
                    Some(v) => out.push_str(&v),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppPolicy;

    #[test]
    fn packed_values_parse_case_insensitively() {
        let p = packed("TranscriptsRoot=%OneDrive%\\Notes; assistantenabled=0;bad;AudioSourceExcludePatterns=bank|vault");
        assert_eq!(
            p.get("transcriptsroot").as_deref(),
            Some("%OneDrive%\\Notes")
        );
        let a = AppPolicy::load(&p);
        assert_eq!(a.assistant_enabled, Some(false));
        assert_eq!(a.transcripts_root.as_deref(), Some("%OneDrive%\\Notes"));
        assert_eq!(a.audio_source_exclude_patterns, vec!["bank", "vault"]);
    }

    #[test]
    fn policy_file_splits_locked_and_starting_values() {
        let text = "\u{feff}".to_string()
            + r#"{"settings":{
            "TranscriptsRoot":{"value":"%OneDrive%\\T","locked":true},
            "Theme":{"value":"dark","locked":false},
            "AssistantEnabled":false,
            "UpdatePublicKeys":{"value":["ssh-ed25519 A","ssh-ed25519 B"]},
            "Broken":{"locked":true}}}"#;
        let f = policy_file(&text);
        let p = AppPolicy::load(&f.locked);
        assert_eq!(p.transcripts_root.as_deref(), Some("%OneDrive%\\T"));
        assert_eq!(p.assistant_enabled, Some(false));
        assert_eq!(p.update_public_keys.len(), 2);
        assert_eq!(f.locked.get("Theme"), None);
        assert_eq!(f.defaults.get("theme").as_deref(), Some("dark"));
        assert_eq!(f.locked.get("Broken"), None);
        assert_eq!(policy_file("not json"), PolicyFile::default());
        assert_eq!(policy_file(r#"{"policy":{}}"#), PolicyFile::default());
    }

    #[test]
    fn env_names_expand_and_unknown_ones_stay() {
        let look = |n: &str| match n.to_ascii_lowercase().as_str() {
            "onedrive" => Some("C:\\Users\\jp\\OneDrive".to_string()),
            "userprofile" => Some("C:\\Users\\jp".to_string()),
            _ => None,
        };
        assert_eq!(
            expand_env("%OneDrive%\\SprkNotes", look),
            "C:\\Users\\jp\\OneDrive\\SprkNotes"
        );
        assert_eq!(expand_env("%NOPE%\\x", look), "%NOPE%\\x");
        assert_eq!(expand_env("100% sure", look), "100% sure");
        assert_eq!(expand_env("%%USERPROFILE%", look), "%C:\\Users\\jp");
        assert_eq!(expand_env("D:\\plain", look), "D:\\plain");
    }
}

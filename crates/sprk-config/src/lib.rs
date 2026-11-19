//! Policy and secret seams (registry, Credential Manager on Windows) and the typed admin
//! policy of SPEC 10.2. Registry overrides win over user settings.
use std::collections::HashMap;

#[cfg(windows)]
pub mod registry;

pub trait PolicyStore: Send {
    /// A string value; a DWORD is returned as its decimal text.
    fn get(&self, key: &str) -> Option<String>;
    /// A multi-string value. Defaults to the single value, if any.
    fn get_multi(&self, key: &str) -> Vec<String> {
        self.get(key).into_iter().collect()
    }
}
pub trait SecretStore: Send {
    fn get(&self, key: &str) -> Option<String>;
}

#[derive(Default)]
pub struct FakePolicy(pub HashMap<String, String>);
impl PolicyStore for FakePolicy {
    fn get(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
    fn get_multi(&self, key: &str) -> Vec<String> {
        self.0
            .get(key)
            .map(|v| v.split('\n').map(str::to_string).collect())
            .unwrap_or_default()
    }
}

/// Machine policy first, then user policy (SPEC 10.2).
pub struct Layered<A: PolicyStore, B: PolicyStore>(pub A, pub B);
impl<A: PolicyStore, B: PolicyStore> PolicyStore for Layered<A, B> {
    fn get(&self, key: &str) -> Option<String> {
        self.0.get(key).or_else(|| self.1.get(key))
    }
    fn get_multi(&self, key: &str) -> Vec<String> {
        let m = self.0.get_multi(key);
        if m.is_empty() {
            self.1.get_multi(key)
        } else {
            m
        }
    }
}

/// Admin policy for the app. `None` means "not set": the user setting or default applies and the
/// matching UI control stays editable. `Some` locks the control.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppPolicy {
    pub transcripts_root: Option<String>,
    pub calendar_provider: Option<String>,
    pub show_claude_handoff: Option<bool>,
    pub show_copilot_handoff: Option<bool>,
    pub filler_removal: Option<bool>,
    pub commit_delay_seconds: Option<u64>,
    pub audio_source_detection: Option<bool>,
    pub audio_merge_window_seconds: Option<u64>,
    pub call_merge_window_seconds: Option<u64>,
    pub audio_source_exclude_patterns: Vec<String>,
    pub call_context: Option<bool>,
    pub call_poll_seconds: Option<u64>,
    pub name_one_to_one_speakers: Option<bool>,
    pub teams_local_api: Option<bool>,
    /// "Download model" button and downloads at all (default on). ADR-005.
    pub model_download: Option<bool>,
    /// Check for a newer model at launch (default on). ADR-005.
    pub model_update_check: Option<bool>,
    /// https address of an internal mirror's `models-index.json`.
    pub model_index_url: Option<String>,
    pub require_signed_models: Option<bool>,
    pub model_public_keys: Vec<String>,
    pub allowed_model_hashes: Vec<String>,
    pub keep_model_versions: Option<u64>,
}

fn flag(s: &dyn PolicyStore, k: &str) -> Option<bool> {
    match s.get(k)?.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

fn num(s: &dyn PolicyStore, k: &str) -> Option<u64> {
    s.get(k)?.trim().parse().ok()
}

impl AppPolicy {
    /// Unparseable values count as "not set" (never an error: policy must not break the app).
    pub fn load(s: &dyn PolicyStore) -> Self {
        let text = |k: &str| {
            s.get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        Self {
            transcripts_root: text("TranscriptsRoot"),
            calendar_provider: text("CalendarProvider").filter(|v| v == "com" || v == "off"),
            show_claude_handoff: flag(s, "ShowClaudeHandoff"),
            show_copilot_handoff: flag(s, "ShowCopilotHandoff"),
            filler_removal: flag(s, "FillerRemoval"),
            commit_delay_seconds: num(s, "CommitDelaySeconds"),
            audio_source_detection: flag(s, "AudioSourceDetection"),
            audio_merge_window_seconds: num(s, "AudioMergeWindowSeconds"),
            call_merge_window_seconds: num(s, "CallMergeWindowSeconds"),
            audio_source_exclude_patterns: s
                .get_multi("AudioSourceExcludePatterns")
                .into_iter()
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect(),
            call_context: flag(s, "CallContext"),
            call_poll_seconds: num(s, "CallPollSeconds"),
            name_one_to_one_speakers: flag(s, "NameOneToOneSpeakers"),
            teams_local_api: flag(s, "TeamsLocalApi"),
            model_download: flag(s, "ModelDownload"),
            model_update_check: flag(s, "ModelUpdateCheck"),
            model_index_url: text("ModelIndexUrl")
                .filter(|v| v.to_ascii_lowercase().starts_with("https://")),
            require_signed_models: flag(s, "RequireSignedModels"),
            model_public_keys: s.get_multi("ModelPublicKeys"),
            allowed_model_hashes: s
                .get_multi("AllowedModelHashes")
                .into_iter()
                .map(|h| h.trim().to_string())
                .filter(|h| !h.is_empty())
                .collect(),
            keep_model_versions: num(s, "KeepModelVersions"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(pairs: &[(&str, &str)]) -> FakePolicy {
        FakePolicy(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    #[test]
    fn fake_policy_lookup() {
        let p = fake(&[("k", "v")]);
        assert_eq!(p.get("k").as_deref(), Some("v"));
        assert_eq!(p.get("missing"), None);
    }

    #[test]
    fn empty_policy_sets_nothing() {
        assert_eq!(
            AppPolicy::load(&FakePolicy::default()),
            AppPolicy::default()
        );
    }

    #[test]
    fn typed_values_parse_and_bad_values_are_ignored() {
        let p = AppPolicy::load(&fake(&[
            ("CallContext", "0"),
            ("NameOneToOneSpeakers", "1"),
            ("CallPollSeconds", "20"),
            ("CommitDelaySeconds", "soon"),
            ("CalendarProvider", "graph"),
            ("AudioSourceExcludePatterns", "banking\n\n  vault  "),
        ]));
        assert_eq!(p.call_context, Some(false));
        assert_eq!(p.name_one_to_one_speakers, Some(true));
        assert_eq!(p.call_poll_seconds, Some(20));
        assert_eq!(p.commit_delay_seconds, None);
        assert_eq!(p.calendar_provider, None);
        assert_eq!(p.audio_source_exclude_patterns, vec!["banking", "vault"]);
    }

    #[test]
    fn model_download_keys_parse_and_http_mirrors_are_ignored() {
        let p = AppPolicy::load(&fake(&[
            ("ModelDownload", "0"),
            ("ModelUpdateCheck", "0"),
            (
                "ModelIndexUrl",
                "https://models.example.internal/models-index.json",
            ),
        ]));
        assert_eq!(p.model_download, Some(false));
        assert_eq!(p.model_update_check, Some(false));
        assert_eq!(
            p.model_index_url.as_deref(),
            Some("https://models.example.internal/models-index.json")
        );
        let p = AppPolicy::load(&fake(&[("ModelIndexUrl", "http://insecure/index.json")]));
        assert_eq!(p.model_index_url, None);
    }

    #[test]
    fn machine_policy_wins_over_user_policy() {
        let l = Layered(
            fake(&[("CallContext", "0")]),
            fake(&[("CallContext", "1"), ("FillerRemoval", "0")]),
        );
        let p = AppPolicy::load(&l);
        assert_eq!(p.call_context, Some(false));
        assert_eq!(p.filler_removal, Some(false));
    }
}

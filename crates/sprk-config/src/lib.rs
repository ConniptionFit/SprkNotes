//! Policy and secret seams (registry, Credential Manager on Windows) and the typed admin
//! policy of SPEC 10.2. Registry overrides win over user settings.
use std::collections::HashMap;

pub mod file;
#[cfg(windows)]
pub mod registry;
pub mod trust;
#[cfg(windows)]
pub mod trust_win;

pub use file::{expand_env, packed, policy_file, FilePolicy, PolicyFile};

/// `%ProgramData%\SprkNotes\policy.json`, the file form of admin policy (SPEC 10.2).
pub fn machine_file() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("ProgramData")?;
    Some(
        std::path::PathBuf::from(base)
            .join("SprkNotes")
            .join("policy.json"),
    )
}

/// The machine policy file, when there is one that only administrators can change. `Err` says why
/// a file that exists is ignored (the app logs it). Off Windows there is no such file.
pub fn machine_policy_file() -> Result<PolicyFile, String> {
    #[cfg(windows)]
    {
        let Some(path) = machine_file().filter(|p| p.is_file()) else {
            return Ok(PolicyFile::default());
        };
        trust::check(&trust_win::WindowsSecurity, &path)?;
        let text =
            std::fs::read_to_string(&path).map_err(|_| "it could not be read".to_string())?;
        Ok(policy_file(&text))
    }
    #[cfg(not(windows))]
    {
        Ok(PolicyFile::default())
    }
}

fn trusted_file() -> PolicyFile {
    machine_policy_file().unwrap_or_default()
}

/// Admin sources, strongest first, for one kind of value (locked or starting): machine registry,
/// then the machine `policy.json`, then the user registry. The registry wins over the file setting
/// by setting, so Group Policy or an RMM registry script can override one value of a shared file.
pub fn layered<M: PolicyStore, U: PolicyStore>(
    machine: M,
    file: FilePolicy,
    user: U,
) -> Layered<M, Layered<FilePolicy, U>> {
    Layered(machine, Layered(file, user))
}

/// Locked settings from every admin source (see [`layered`]). Only administrators can write the
/// machine key and the trusted file, so that is where locks belong. Nothing is set off Windows.
pub fn admin_policy() -> AppPolicy {
    #[cfg(windows)]
    {
        AppPolicy::load(&layered(
            registry::RegistryPolicy::machine(),
            trusted_file().locked,
            registry::RegistryPolicy::user(),
        ))
    }
    #[cfg(not(windows))]
    {
        AppPolicy::load(&trusted_file().locked)
    }
}

/// Starting values (never locked) from the same sources.
pub fn admin_defaults() -> InstallDefaults {
    #[cfg(windows)]
    {
        let r = registry::RegistryPolicy::defaults_layered();
        InstallDefaults::load(&layered(r.0, trusted_file().defaults, r.1))
    }
    #[cfg(not(windows))]
    {
        InstallDefaults::load(&trusted_file().defaults)
    }
}

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
    /// `SummarizeEnabled=0` removes Summarize altogether (transcripts may hold PHI); the
    /// per-app switches below hide one app each.
    pub summarize_enabled: Option<bool>,
    pub show_claude_handoff: Option<bool>,
    pub show_copilot_handoff: Option<bool>,
    /// Where the Copilot handoff opens (SPEC 9.2): an app link or an https page.
    pub copilot_target: Option<String>,
    /// Admin prompt template file (SPEC 9.3).
    pub prompt_template_path: Option<String>,
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
    /// Assistant on or off (ADR-009). Off unless someone turns it on; `Some` locks the setting.
    pub assistant_enabled: Option<bool>,
    /// Largest assistant tier allowed: `off`, `titles`, `light`, `standard` or `auto`.
    pub assistant_tier: Option<String>,
    /// `<id>@<version>` or a bare id for the assistant pack.
    pub assistant_pinned_model: Option<String>,
    pub assistant_idle_unload_minutes: Option<u64>,
    pub assistant_max_threads: Option<u64>,
    /// May the assistant write notes next to the transcript.
    pub assistant_save_outputs: Option<bool>,
    /// May the assistant work in the background (rolling summaries) during a session.
    pub assistant_background: Option<bool>,
    /// Comma separated allow-list of assistant features (`title,recap,actions,ask,agenda,...`).
    /// Absent means the user decides; present means only these can be turned on.
    pub assistant_features: Option<String>,
    /// App updates (ADR-011): `off`, `notify`, `auto` or `force`. `Some` locks the setting.
    pub update_mode: Option<String>,
    /// Release channel: `stable`, `beta` or `alpha`. `Some` locks the setting.
    pub update_channel: Option<String>,
    /// https address of the repository releases come from. `Some` locks the setting.
    pub update_repo_url: Option<String>,
    /// `forgejo` or `github`. Inferred from the address when absent.
    pub update_provider: Option<String>,
    /// OpenSSH `ssh-ed25519 ...` public keys that may sign update manifests.
    pub update_public_keys: Vec<String>,
    /// Hours between checks (default 24, at least 1).
    pub update_check_hours: Option<u64>,
}

/// An app link (`scheme:...`, scheme of two or more characters so `C:` is no link) or an https
/// page. Never a file path, a plain-http page or a `file:` link.
fn safe_link(v: &str) -> bool {
    let l = v.to_ascii_lowercase();
    if l.starts_with("https://") {
        return true;
    }
    let Some((scheme, _)) = l.split_once(':') else {
        return false;
    };
    scheme.len() >= 2
        && scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'+' | b'.'))
        && !matches!(scheme, "http" | "file" | "javascript" | "data")
}

/// Starting values the installer can set (MSI `UPDATE_*` switches). They never lock a setting:
/// they apply until the user picks something else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstallDefaults {
    /// Starting save folder; may hold `%USERPROFILE%`-style variables (expanded per user).
    pub transcripts_root: Option<String>,
    pub update_mode: Option<String>,
    pub update_channel: Option<String>,
    pub update_repo_url: Option<String>,
    pub update_provider: Option<String>,
}

impl InstallDefaults {
    pub fn load(s: &dyn PolicyStore) -> Self {
        // Same validation as policy, so one rule set covers both.
        let p = AppPolicy::load(s);
        Self {
            transcripts_root: p.transcripts_root,
            update_mode: p.update_mode,
            update_channel: p.update_channel,
            update_repo_url: p.update_repo_url,
            update_provider: p.update_provider,
        }
    }
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

/// Every key `AppPolicy::load` reads. The admin install-script form and the ADMX list the same
/// keys; a test keeps the form in step with this list.
pub const KEYS: &[&str] = &[
    "TranscriptsRoot",
    "CalendarProvider",
    "SummarizeEnabled",
    "ShowClaudeHandoff",
    "ShowCopilotHandoff",
    "CopilotTarget",
    "PromptTemplatePath",
    "FillerRemoval",
    "CommitDelaySeconds",
    "AudioSourceDetection",
    "AudioMergeWindowSeconds",
    "CallMergeWindowSeconds",
    "AudioSourceExcludePatterns",
    "CallContext",
    "CallPollSeconds",
    "NameOneToOneSpeakers",
    "TeamsLocalApi",
    "ModelDownload",
    "ModelUpdateCheck",
    "ModelIndexUrl",
    "RequireSignedModels",
    "ModelPublicKeys",
    "AllowedModelHashes",
    "KeepModelVersions",
    "AssistantEnabled",
    "AssistantTier",
    "AssistantPinnedModel",
    "AssistantIdleUnloadMinutes",
    "AssistantMaxThreads",
    "AssistantSaveOutputs",
    "AssistantBackground",
    "AssistantFeatures",
    "UpdateMode",
    "UpdateChannel",
    "UpdateRepoUrl",
    "UpdateProvider",
    "UpdatePublicKeys",
    "UpdateCheckHours",
];

/// Window preferences an admin can set (SPEC 10.2b). The app keeps these in the window, so the
/// backend hands over the raw text and the window checks it; a bad value is ignored there.
pub const UI_KEYS: &[&str] = &[
    "Theme",
    "ThrowbackMode",
    "LiveCaptions",
    "LiveLine",
    "CaptionDelay",
    "CaptionSpeakers",
    "MicPrompt",
    "Hotkey",
    "HotkeyKeys",
];

/// Admin values for the window preferences: `locked` from policy (the control is read-only),
/// `defaults` from the installer (used until the person changes that preference). Includes
/// `AssistantEnabled`, whose switch lives in the window too.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct UiManaged {
    pub locked: std::collections::BTreeMap<String, String>,
    pub defaults: std::collections::BTreeMap<String, String>,
}

impl UiManaged {
    pub fn load(policy: &dyn PolicyStore, defaults: &dyn PolicyStore) -> Self {
        let pick = |s: &dyn PolicyStore| {
            UI_KEYS
                .iter()
                .chain(["AssistantEnabled", "SummarizeEnabled"].iter())
                .filter_map(|k| {
                    let v = s.get(k)?.trim().to_string();
                    (!v.is_empty()).then(|| (k.to_string(), v))
                })
                .collect()
        };
        Self {
            locked: pick(policy),
            defaults: pick(defaults),
        }
    }
}

/// [`UiManaged`] from every admin source (see [`layered`]).
pub fn admin_ui() -> UiManaged {
    let file = trusted_file();
    #[cfg(windows)]
    {
        let d = registry::RegistryPolicy::defaults_layered();
        UiManaged::load(
            &layered(
                registry::RegistryPolicy::machine(),
                file.locked,
                registry::RegistryPolicy::user(),
            ),
            &layered(d.0, file.defaults, d.1),
        )
    }
    #[cfg(not(windows))]
    {
        UiManaged::load(&file.locked, &file.defaults)
    }
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
            summarize_enabled: flag(s, "SummarizeEnabled"),
            show_claude_handoff: flag(s, "ShowClaudeHandoff"),
            show_copilot_handoff: flag(s, "ShowCopilotHandoff"),
            copilot_target: text("CopilotTarget").filter(|v| safe_link(v)),
            prompt_template_path: text("PromptTemplatePath"),
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
            assistant_enabled: flag(s, "AssistantEnabled"),
            assistant_tier: text("AssistantTier")
                .map(|v| v.to_ascii_lowercase())
                .filter(|v| matches!(v.as_str(), "off" | "titles" | "light" | "standard" | "auto")),
            assistant_pinned_model: text("AssistantPinnedModel"),
            assistant_idle_unload_minutes: num(s, "AssistantIdleUnloadMinutes"),
            assistant_max_threads: num(s, "AssistantMaxThreads").filter(|n| *n > 0),
            assistant_save_outputs: flag(s, "AssistantSaveOutputs"),
            assistant_background: flag(s, "AssistantBackground"),
            assistant_features: text("AssistantFeatures"),
            update_mode: text("UpdateMode")
                .map(|v| v.to_ascii_lowercase())
                .filter(|v| matches!(v.as_str(), "off" | "notify" | "auto" | "force")),
            update_channel: text("UpdateChannel")
                .map(|v| v.to_ascii_lowercase())
                .filter(|v| matches!(v.as_str(), "stable" | "beta" | "alpha")),
            update_repo_url: text("UpdateRepoUrl")
                .filter(|v| v.to_ascii_lowercase().starts_with("https://")),
            update_provider: text("UpdateProvider")
                .map(|v| v.to_ascii_lowercase())
                .filter(|v| matches!(v.as_str(), "forgejo" | "github")),
            update_public_keys: s
                .get_multi("UpdatePublicKeys")
                .into_iter()
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
                .collect(),
            update_check_hours: num(s, "UpdateCheckHours").filter(|n| *n >= 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_registry_beats_the_file_which_beats_user_policy() {
        let file = policy_file(
            r#"{"settings":{"TranscriptsRoot":{"value":"D:\\File","locked":true},
               "CommitDelaySeconds":{"value":7,"locked":true},
               "Theme":{"value":"light","locked":false}}}"#,
        );
        let machine = fake(&[("TranscriptsRoot", "D:\\Machine")]);
        let user = fake(&[
            ("TranscriptsRoot", "D:\\User"),
            ("CommitDelaySeconds", "3"),
            ("FillerRemoval", "0"),
        ]);
        let p = AppPolicy::load(&layered(machine, file.locked.clone(), user));
        assert_eq!(p.transcripts_root.as_deref(), Some("D:\\Machine"));
        assert_eq!(p.commit_delay_seconds, Some(7));
        assert_eq!(p.filler_removal, Some(false));
        // An unlocked value in the file is a starting value, never a lock.
        let ui = UiManaged::load(
            &layered(fake(&[]), file.locked, fake(&[])),
            &layered(fake(&[]), file.defaults, fake(&[("Theme", "dark")])),
        );
        assert_eq!(ui.locked.get("Theme"), None);
        assert_eq!(ui.defaults.get("Theme").map(String::as_str), Some("light"));
    }

    #[test]
    fn window_preferences_come_from_policy_and_defaults() {
        let pol = fake(&[("LiveCaptions", "0"), ("Theme", " "), ("Nope", "1")]);
        let def = fake(&[("Theme", "dark"), ("AssistantEnabled", "1")]);
        let m = UiManaged::load(&pol, &def);
        assert_eq!(m.locked.len(), 1);
        assert_eq!(m.locked.get("LiveCaptions").map(String::as_str), Some("0"));
        assert_eq!(m.defaults.get("Theme").map(String::as_str), Some("dark"));
        assert_eq!(
            m.defaults.get("AssistantEnabled").map(String::as_str),
            Some("1")
        );
    }

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
    fn handoff_keys_parse_and_unsafe_targets_are_ignored() {
        let p = AppPolicy::load(&fake(&[
            ("CopilotTarget", "https://m365.cloud.microsoft/chat"),
            (
                "PromptTemplatePath",
                "C:\\ProgramData\\SprkNotes\\summary.md",
            ),
        ]));
        assert_eq!(
            p.copilot_target.as_deref(),
            Some("https://m365.cloud.microsoft/chat")
        );
        assert!(p.prompt_template_path.is_some());
        for bad in [
            "http://x",
            "C:\\evil.exe",
            "C:/evil.exe",
            "file:///x",
            "plain",
        ] {
            assert_eq!(
                AppPolicy::load(&fake(&[("CopilotTarget", bad)])).copilot_target,
                None,
                "{bad}"
            );
        }
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

    #[test]
    fn assistant_keys_parse_and_bad_values_are_ignored() {
        let p = AppPolicy::load(&fake(&[
            ("AssistantEnabled", "1"),
            ("AssistantTier", "Light"),
            ("AssistantIdleUnloadMinutes", "10"),
            ("AssistantMaxThreads", "0"),
            ("AssistantSaveOutputs", "0"),
            ("AssistantBackground", "maybe"),
        ]));
        assert_eq!(p.assistant_enabled, Some(true));
        assert_eq!(p.assistant_tier.as_deref(), Some("light"));
        assert_eq!(p.assistant_idle_unload_minutes, Some(10));
        assert_eq!(p.assistant_max_threads, None);
        assert_eq!(p.assistant_save_outputs, Some(false));
        assert_eq!(p.assistant_background, None);
        let bad = AppPolicy::load(&fake(&[("AssistantTier", "huge")]));
        assert_eq!(bad.assistant_tier, None);
    }

    #[test]
    fn update_keys_parse_and_bad_values_are_ignored() {
        let p = AppPolicy::load(&fake(&[
            ("UpdateMode", "Force"),
            ("UpdateChannel", "BETA"),
            ("UpdateRepoUrl", "https://git.pwrs.dev/powers/sprknotes"),
            ("UpdateProvider", "github"),
            ("UpdateCheckHours", "6"),
        ]));
        assert_eq!(p.update_mode.as_deref(), Some("force"));
        assert_eq!(p.update_channel.as_deref(), Some("beta"));
        assert_eq!(p.update_provider.as_deref(), Some("github"));
        assert_eq!(p.update_check_hours, Some(6));
        assert!(p.update_repo_url.is_some());
        let q = AppPolicy::load(&fake(&[
            ("UpdateMode", "sometimes"),
            ("UpdateChannel", "nightly"),
            ("UpdateRepoUrl", "http://insecure/x/y"),
            ("UpdateCheckHours", "0"),
        ]));
        assert_eq!(q.update_mode, None);
        assert_eq!(q.update_channel, None);
        assert_eq!(q.update_repo_url, None);
        assert_eq!(q.update_check_hours, None);
    }
}

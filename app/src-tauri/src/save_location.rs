//! Where transcripts are saved (SPEC 6.1): admin policy, then the user's choice in Settings,
//! then an installer default, then `Documents\SprkNotes\Transcripts`. Every value may use
//! `%USERPROFILE%`-style variables, expanded for the person running the app, so one admin value
//! works for every user.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// `settings.json`. Each setter loads it, changes its own field and stores it again, so one
/// setting never wipes another.
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Saved {
    pub(crate) transcripts_root: Option<String>,
    /// Custom words (SPEC 4.3a), the person's own list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) custom_words: Vec<String>,
    /// "Keep transcripts for", in days (SPEC 6.6). None or 0 is forever.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) keep_days: Option<u64>,
}

fn saved_path() -> Option<PathBuf> {
    crate::models::user_data_dir().map(|d| d.join("SprkNotes").join("settings.json"))
}

pub(crate) fn load() -> Saved {
    saved_path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub(crate) fn store(s: &Saved) -> Result<(), String> {
    let p = saved_path().ok_or("No settings folder is available.")?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|_| "Your settings could not be saved.")?;
    }
    let b = serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(p, b).map_err(|_| "Your settings could not be saved.".to_string())
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

pub fn expand(v: &str) -> String {
    sprk_config::expand_env(v.trim(), env)
}

/// Which source decided the folder; the UI says it in plain words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Policy,
    User,
    Install,
    Default,
}

/// The folder in force. Pure so it can be tested: `expand` turns `%VAR%` into this user's value.
pub fn resolve(
    policy: Option<&str>,
    user: Option<&str>,
    install: Option<&str>,
    documents_default: Option<PathBuf>,
    expand: &dyn Fn(&str) -> String,
) -> Option<(PathBuf, Source)> {
    let usable = |v: Option<&str>| {
        v.map(expand)
            .filter(|p| !p.is_empty() && !p.contains('%') && PathBuf::from(p).is_absolute())
            .map(PathBuf::from)
    };
    if let Some(p) = usable(policy) {
        return Some((p, Source::Policy));
    }
    if let Some(p) = usable(user) {
        return Some((p, Source::User));
    }
    if let Some(p) = usable(install) {
        return Some((p, Source::Install));
    }
    documents_default.map(|p| (p, Source::Default))
}

pub fn current(app: &AppHandle, policy: &sprk_config::AppPolicy) -> Option<(PathBuf, Source)> {
    let defaults = sprk_config::admin_defaults();
    let user = load();
    resolve(
        policy.transcripts_root.as_deref(),
        user.transcripts_root.as_deref(),
        defaults.transcripts_root.as_deref(),
        sprk_core::settings::default_transcripts_dir(
            app.path().document_dir().ok(),
            app.path().home_dir().ok(),
        ),
        &expand,
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// The folder in use, expanded.
    pub path: String,
    /// What the user typed or picked, as typed (may hold `%VAR%`). None means "not chosen".
    pub chosen: Option<String>,
    pub source: Source,
    /// Set by an administrator: the Settings field is read-only.
    pub locked: bool,
}

#[tauri::command]
pub fn save_location_status(app: AppHandle) -> Result<Status, String> {
    let policy = crate::policy();
    let (path, source) = current(&app, &policy).ok_or("No Documents folder was found.")?;
    Ok(Status {
        path: path.to_string_lossy().into_owned(),
        chosen: load().transcripts_root,
        source,
        locked: source == Source::Policy,
    })
}

/// Save the user's folder (None or empty goes back to the default). The folder is created now so
/// a typo shows up here and not at the next session.
#[tauri::command]
pub fn save_location_set(app: AppHandle, path: Option<String>) -> Result<Status, String> {
    if crate::policy().transcripts_root.is_some() {
        return Err("Your organization sets where transcripts are saved.".into());
    }
    let chosen = path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
    if let Some(c) = &chosen {
        let full = expand(c);
        if full.contains('%') {
            return Err("That folder uses a name this PC does not know (between % signs).".into());
        }
        let p = PathBuf::from(&full);
        if !p.is_absolute() {
            return Err("Enter a full folder path, for example D:\\Transcripts.".into());
        }
        std::fs::create_dir_all(&p)
            .map_err(|_| "That folder could not be created or opened.".to_string())?;
    }
    let mut saved = load();
    saved.transcripts_root = chosen;
    store(&saved)?;
    save_location_status(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exp(v: &str) -> String {
        sprk_config::expand_env(v, |n| {
            (n.eq_ignore_ascii_case("OneDrive")).then(|| abs("od"))
        })
    }

    fn abs(leaf: &str) -> String {
        std::env::temp_dir()
            .join(leaf)
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn policy_then_user_then_install_then_documents() {
        let docs = Some(PathBuf::from(abs("docs")));
        let p = abs("p");
        let u = abs("u");
        let i = abs("i");
        let got = |a, b, c| resolve(a, b, c, docs.clone(), &exp).map(|(_, s)| s);
        assert_eq!(got(Some(&p), Some(&u), Some(&i)), Some(Source::Policy));
        assert_eq!(got(None, Some(&u), Some(&i)), Some(Source::User));
        assert_eq!(got(None, None, Some(&i)), Some(Source::Install));
        assert_eq!(got(None, None, None), Some(Source::Default));
    }

    #[test]
    fn variables_expand_and_unusable_values_are_skipped() {
        let docs = Some(PathBuf::from(abs("docs")));
        let sep = std::path::MAIN_SEPARATOR;
        let (p, s) = resolve(
            None,
            Some(&format!("%OneDrive%{sep}T")),
            None,
            docs.clone(),
            &exp,
        )
        .unwrap_or_else(|| (PathBuf::new(), Source::Default));
        assert_eq!(s, Source::User);
        assert_eq!(p, PathBuf::from(format!("{}{sep}T", abs("od"))));
        // An unknown variable or a relative path never becomes the folder.
        let (_, s) = resolve(Some("%NOPE%\\x"), Some("relative"), None, docs, &exp)
            .unwrap_or_else(|| (PathBuf::new(), Source::Policy));
        assert_eq!(s, Source::Default);
    }
}

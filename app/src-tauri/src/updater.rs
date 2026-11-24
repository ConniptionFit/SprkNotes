//! App updates (ADR-011). Checks a release source (Forgejo or GitHub) for a newer build on the
//! chosen channel, verifies it (SHA-256 plus an Ed25519 signature on the manifest) and installs
//! the MSI. The decisions live in `sprk-update`; this file wires policy, the user's choice, the
//! WinHTTP client and msiexec. Like model downloads it sends only plain GETs: nothing about the
//! user, the PC or any transcript.
//!
//! Who decides what (admin policy wins, then the user's saved choice, then installer defaults):
//! - `UpdateMode` off / notify / auto / force
//! - `UpdateChannel` stable / beta / alpha
//! - `UpdateRepoUrl` (+ `UpdateProvider`) where releases come from
//! Any of the three set by policy is locked: the Settings control shows "Managed by your organization".
use serde::{Deserialize, Serialize};
use sprk_config::{AppPolicy, InstallDefaults};
use sprk_models::remote::Fetcher;
use sprk_update::{Channel, Query, Source, Trust, Update};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

/// Compiled in from `distribution.toml` (see docs/forking.md).
pub const DEFAULT_REPO: &str = env!("SPRK_DEFAULT_REPO_URL");
/// Provider named in `distribution.toml`; empty means infer it from the address.
pub const DEFAULT_PROVIDER: &str = env!("SPRK_DEFAULT_PROVIDER");
const DEFAULT_CHECK_HOURS: u64 = 24;
/// Keys compiled into the app as a fallback trust root for update manifests. Empty until the
/// release key exists; admins add their own with `UpdatePublicKeys`.
const BUILTIN_UPDATE_KEYS: &[&str] = &[];

static BUSY: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Deserialize, Default, Clone)]
struct UserChoice {
    channel: Option<String>,
    repo_url: Option<String>,
    mode: Option<String>,
    /// Seconds since the epoch of the last successful check.
    last_check: Option<u64>,
}

fn data_dir() -> Option<PathBuf> {
    crate::models::user_data_dir().map(|p| p.join("SprkNotes"))
}

fn choice_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("update.json"))
}

fn load_choice() -> UserChoice {
    choice_path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_choice(c: &UserChoice) {
    if let (Some(p), Ok(b)) = (choice_path(), serde_json::to_vec_pretty(c)) {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, b);
    }
}

fn defaults() -> InstallDefaults {
    #[cfg(windows)]
    {
        InstallDefaults::load(&sprk_config::registry::RegistryPolicy::defaults_layered())
    }
    #[cfg(not(windows))]
    {
        InstallDefaults::default()
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The settings in force, after policy, the user's choice and installer defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    pub mode: String,
    pub channel: Channel,
    pub repo_url: String,
    pub provider: Option<String>,
    pub mode_locked: bool,
    pub channel_locked: bool,
    pub repo_locked: bool,
}

fn resolve(p: &AppPolicy, d: &InstallDefaults, u: &UserChoice) -> Effective {
    let pick =
        |pol: &Option<String>, user: &Option<String>, def: &Option<String>, fallback: &str| {
            pol.clone()
                .or_else(|| user.clone())
                .or_else(|| def.clone())
                .unwrap_or_else(|| fallback.to_string())
        };
    let channel = pick(&p.update_channel, &u.channel, &d.update_channel, "stable");
    let repo_url = pick(
        &p.update_repo_url,
        &u.repo_url,
        &d.update_repo_url,
        DEFAULT_REPO,
    );
    // The built-in provider only goes with the built-in address.
    let built_in_provider = (repo_url == DEFAULT_REPO && !DEFAULT_PROVIDER.is_empty())
        .then(|| DEFAULT_PROVIDER.to_string());
    Effective {
        mode: pick(&p.update_mode, &u.mode, &d.update_mode, "notify"),
        channel: Channel::parse(&channel).unwrap_or(Channel::Stable),
        repo_url,
        provider: p
            .update_provider
            .clone()
            .or_else(|| d.update_provider.clone())
            .or(built_in_provider),
        mode_locked: p.update_mode.is_some(),
        channel_locked: p.update_channel.is_some(),
        repo_locked: p.update_repo_url.is_some(),
    }
}

pub fn effective(p: &AppPolicy) -> Effective {
    resolve(p, &defaults(), &load_choice())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub installed: String,
    pub mode: String,
    pub channel: String,
    pub repo_url: String,
    pub mode_locked: bool,
    pub channel_locked: bool,
    pub repo_locked: bool,
    pub busy: bool,
    /// A verified installer is waiting for the next exit.
    pub ready_to_install: bool,
    /// Per-machine installs are updated by the scheduled task, not by this app.
    pub managed_install: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub available: bool,
    pub version: Option<String>,
    pub notes_url: Option<String>,
}

/// True when the app lives under Program Files: updates there come from the SYSTEM task.
fn machine_install() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.to_str().map(|s| s.to_ascii_lowercase()))
        .map(|s| s.contains("\\program files"))
        .unwrap_or(false)
}

pub fn status(p: &AppPolicy) -> UpdateStatus {
    prune_pending();
    let e = effective(p);
    UpdateStatus {
        installed: env!("CARGO_PKG_VERSION").to_string(),
        mode: e.mode,
        channel: e.channel.as_str().to_string(),
        repo_url: e.repo_url,
        mode_locked: e.mode_locked,
        channel_locked: e.channel_locked,
        repo_locked: e.repo_locked,
        busy: BUSY.load(Ordering::SeqCst),
        ready_to_install: pending_path().is_some_and(|p| p.is_file()),
        managed_install: machine_install(),
    }
}

/// Save the user's choice. Anything policy locks is refused so a hand-edited UI cannot bypass it.
pub fn set_choice(
    p: &AppPolicy,
    channel: Option<String>,
    repo_url: Option<String>,
    mode: Option<String>,
) -> Result<(), String> {
    let mut c = load_choice();
    if let Some(ch) = channel {
        if p.update_channel.is_some() {
            return Err("The release channel is managed by your organization.".into());
        }
        c.channel = Some(
            Channel::parse(&ch)
                .ok_or("Unknown channel.")?
                .as_str()
                .into(),
        );
    }
    if let Some(r) = repo_url {
        if p.update_repo_url.is_some() {
            return Err("The update source is managed by your organization.".into());
        }
        Source::parse(&r, p.update_provider.as_deref().or((r == DEFAULT_REPO && !DEFAULT_PROVIDER.is_empty()).then_some(DEFAULT_PROVIDER)))
            .map_err(|_| "That is not a valid repository address.".to_string())?;
        c.repo_url = Some(r);
    }
    if let Some(m) = mode {
        if p.update_mode.is_some() {
            return Err("Updates are managed by your organization.".into());
        }
        if !matches!(m.as_str(), "off" | "notify" | "auto") {
            return Err("Unknown update mode.".into());
        }
        c.mode = Some(m);
    }
    save_choice(&c);
    Ok(())
}

fn trust(p: &AppPolicy) -> Trust {
    let mut text = BUILTIN_UPDATE_KEYS.join("\n");
    text.push('\n');
    text.push_str(&p.update_public_keys.join("\n"));
    let keys = sprk_models::sshsig::parse_keys(&text);
    let v1 = env!("CARGO_PKG_VERSION").split('.').next() != Some("0");
    Trust {
        require_signature: v1 || !keys.is_empty(),
        keys,
    }
}

/// Network check, no download. `Ok(None)` means up to date.
pub fn check(f: &dyn Fetcher, p: &AppPolicy) -> Result<Option<Update>, String> {
    let e = effective(p);
    if e.mode == "off" {
        return Err("Updates are turned off.".into());
    }
    let source = Source::parse(&e.repo_url, e.provider.as_deref())
        .map_err(|_| "The update source is not a valid https repository address.".to_string())?;
    let t = trust(p);
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    };
    let q = Query {
        source: &source,
        channel: e.channel,
        installed: env!("CARGO_PKG_VERSION"),
        arch,
        trust: &t,
    };
    let found = sprk_update::check(f, &q).map_err(|e| match e {
        sprk_update::UpdateError::Network => "Could not reach the update server.".to_string(),
        sprk_update::UpdateError::Signature(_) => {
            "The update could not be verified and was ignored.".to_string()
        }
        other => format!("{other}."),
    });
    if found.is_ok() {
        let mut c = load_choice();
        c.last_check = Some(now());
        save_choice(&c);
    }
    found
}

#[cfg(windows)]
fn fetcher() -> crate::net::WinHttp {
    crate::net::WinHttp
}
#[cfg(not(windows))]
fn fetcher() -> crate::net_unix::Https {
    crate::net_unix::Https
}

pub fn check_now(p: &AppPolicy) -> Result<UpdateInfo, String> {
    let u = check(&fetcher(), p)?;
    Ok(UpdateInfo {
        available: u.is_some(),
        version: u.as_ref().map(|u| u.version.clone()),
        notes_url: u.map(|u| u.notes_url).filter(|n| !n.is_empty()),
    })
}

// ---------- download and install ----------

fn pending_dir() -> Option<PathBuf> {
    data_dir().map(|d| d.join("updates"))
}
fn pending_path() -> Option<PathBuf> {
    pending_dir().map(|d| d.join("pending.msi"))
}

/// Forget a verified installer that is not newer than this build (it was installed, or the user
/// moved on), so it never reinstalls in a loop.
fn prune_pending() {
    let Some(dir) = pending_dir() else { return };
    let pending_version = std::fs::read_to_string(dir.join("pending.version")).ok();
    let newer = pending_version
        .as_deref()
        .and_then(sprk_update::Version::parse)
        .zip(sprk_update::Version::parse(env!("CARGO_PKG_VERSION")))
        .is_some_and(|(p, me)| p > me);
    if !newer {
        let _ = std::fs::remove_file(dir.join("pending.msi"));
        let _ = std::fs::remove_file(dir.join("pending.version"));
    }
}

/// Is the next check due? Spacing comes from policy `UpdateCheckHours` (default 24).
fn check_due(p: &AppPolicy) -> bool {
    let hours = p.update_check_hours.unwrap_or(DEFAULT_CHECK_HOURS);
    load_choice()
        .last_check
        .map(|t| now().saturating_sub(t) >= hours * 3600)
        .unwrap_or(true)
}

/// Download and verify the installer. It is left as `pending.msi`; nothing runs yet.
fn download(f: &dyn Fetcher, u: &Update) -> Result<PathBuf, String> {
    let dir = pending_dir().ok_or("No folder for the update.")?;
    std::fs::create_dir_all(&dir).map_err(|_| "The update folder is not writable.".to_string())?;
    let part = dir.join("pending.part");
    f.get_to_file(
        &u.url,
        &part,
        sprk_update::MAX_PACKAGE_BYTES,
        &mut |_, _| {},
    )
    .map_err(|_| "The update download did not finish.".to_string())?;
    let bytes =
        std::fs::read(&part).map_err(|_| "The update could not be read back.".to_string())?;
    if let Err(e) = sprk_update::verify_download(u, &bytes) {
        let _ = std::fs::remove_file(&part);
        return Err(format!("{e}."));
    }
    let done = dir.join("pending.msi");
    std::fs::rename(&part, &done).map_err(|_| "The update could not be saved.".to_string())?;
    let _ = std::fs::write(dir.join("pending.version"), &u.version);
    Ok(done)
}

/// Check, then download and verify when there is something newer. Returns the new version.
pub fn fetch_update(p: &AppPolicy) -> Result<Option<String>, String> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err("An update is already in progress.".into());
    }
    let r = (|| {
        let f = fetcher();
        match check(&f, p)? {
            Some(u) => download(&f, &u).map(|_| Some(u.version)),
            None => Ok(None),
        }
    })();
    BUSY.store(false, Ordering::SeqCst);
    r
}

/// Run the pending installer silently. Per-user MSI needs no elevation; a per-machine install
/// is updated by the SYSTEM task instead and never reaches here. A failed install leaves the
/// current version untouched (Windows Installer rolls back).
#[cfg(windows)]
pub fn install_pending(quiet_ui: bool, wait: bool) -> bool {
    use std::os::windows::process::CommandExt;
    let Some(msi) = pending_path().filter(|p| p.is_file()) else {
        return false;
    };
    let ui = if quiet_ui { "/qn" } else { "/passive" };
    let log = msi.with_file_name("install.log");
    let mut cmd = std::process::Command::new("msiexec.exe");
    cmd.arg("/i")
        .arg(&msi)
        .args([ui, "/norestart", "/l*v"])
        .arg(log)
        .creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    // Without this a SYSTEM run treats the package as per-user, skips the per-machine product when
    // looking for the version to replace, and installs a second copy next to it.
    if machine_install() {
        cmd.arg("ALLUSERS=1");
    }
    if wait {
        cmd.status()
            .is_ok_and(|s| s.success() || s.code() == Some(3010))
    } else {
        cmd.spawn().is_ok()
    }
}

#[cfg(not(windows))]
pub fn install_pending(_quiet_ui: bool, _wait: bool) -> bool {
    false
}

/// At exit: when the mode allows it, hand a verified installer to msiexec so the new build is
/// there next launch. Never runs mid-session and never on a per-machine install.
pub fn on_exit(p: &AppPolicy) {
    let e = effective(p);
    prune_pending();
    if matches!(e.mode.as_str(), "auto" | "force") && !machine_install() {
        install_pending(true, false);
    }
}

/// Start of the app: a silent check on the admin's interval. In `auto` and `force` modes the
/// installer is also downloaded so it is ready at exit. Failure is silent.
pub fn launch_check(app: AppHandle, p: AppPolicy) {
    let e = effective(&p);
    if e.mode == "off" {
        return;
    }
    if !check_due(&p) {
        return;
    }
    std::thread::spawn(move || {
        let fetch = matches!(e.mode.as_str(), "auto" | "force") && !machine_install();
        if fetch {
            if let Ok(Some(v)) = fetch_update(&p) {
                let _ = app.emit(
                    "app-update",
                    UpdateInfo {
                        available: true,
                        version: Some(v),
                        notes_url: None,
                    },
                );
            }
        } else if let Ok(info) = check_now(&p) {
            if info.available {
                let _ = app.emit("app-update", info);
            }
        }
    });
}

// ---------- headless mode for the SYSTEM scheduled task ----------

/// Is any SprkNotes window running? A per-machine update waits until nobody is using the app.
#[cfg(windows)]
fn app_running() -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("tasklist.exe")
        .args(["/FI", "IMAGENAME eq SprkNotes.exe", "/NH"])
        .creation_flags(0x0800_0000)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("SprkNotes.exe"))
        .unwrap_or(true)
}

/// `SprkNotes.exe --update`: what the SYSTEM task runs. No window. In `auto` and `force` modes it
/// checks on the admin's interval, downloads and verifies, and installs only when nobody has the
/// app open (otherwise the verified installer waits for the next run). Failures are silent and
/// the exit code stays 0, so a blocked network never makes the task look broken.
#[cfg(windows)]
pub fn run_headless(p: &AppPolicy) {
    let e = effective(p);
    if !matches!(e.mode.as_str(), "auto" | "force") {
        return;
    }
    prune_pending();
    let ready = pending_path().is_some_and(|x| x.is_file());
    if !ready && check_due(p) {
        let _ = fetch_update(p);
    }
    if pending_path().is_some_and(|x| x.is_file()) && !app_running() {
        install_pending(true, true);
    }
}

#[cfg(not(windows))]
pub fn run_headless(_p: &AppPolicy) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn pol(f: impl FnOnce(&mut AppPolicy)) -> AppPolicy {
        let mut p = AppPolicy::default();
        f(&mut p);
        p
    }

    #[test]
    fn policy_wins_then_user_then_installer_defaults() {
        let d = InstallDefaults {
            update_channel: Some("beta".into()),
            update_mode: Some("auto".into()),
            ..Default::default()
        };
        let u = UserChoice {
            channel: Some("alpha".into()),
            ..Default::default()
        };
        let none = pol(|_| {});
        let e = resolve(&none, &d, &u);
        assert_eq!(
            (e.channel, e.mode.as_str(), e.channel_locked),
            (Channel::Alpha, "auto", false)
        );
        let locked = pol(|p| p.update_channel = Some("stable".into()));
        let e = resolve(&locked, &d, &u);
        assert_eq!((e.channel, e.channel_locked), (Channel::Stable, true));
        let e = resolve(&none, &InstallDefaults::default(), &UserChoice::default());
        assert_eq!(
            (e.channel, e.mode.as_str(), e.repo_url.as_str()),
            (Channel::Stable, "notify", DEFAULT_REPO)
        );
    }

    #[test]
    fn locked_settings_refuse_user_changes() {
        let p = pol(|p| {
            p.update_channel = Some("stable".into());
            p.update_repo_url = Some("https://git.example/a/b".into());
            p.update_mode = Some("force".into());
        });
        assert!(set_choice(&p, Some("beta".into()), None, None).is_err());
        assert!(set_choice(&p, None, Some("https://git.example/c/d".into()), None).is_err());
        assert!(set_choice(&p, None, None, Some("off".into())).is_err());
    }
}

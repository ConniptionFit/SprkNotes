//! "Download model" and the launch-time update check (ADR-005). The transcription packs go through
//! `sprk_core::download`, shared with the phone app; this file adds the desktop's stores, its
//! https client (WinHTTP on Windows), the window events and the Ask Sprk packs (ADR-010).
//! Never touches audio or transcripts.
use serde::Serialize;
use sprk_config::AppPolicy;
use sprk_core::download::{
    self, download_allowed, friendly, keep_versions, now_stamp, supported_engines, trust,
    update_check_allowed, Progress, Smoke,
};
pub use sprk_core::download::{ModelStatus, PackInfo, UpdateInfo};
use sprk_models::import::{ImportOptions, ImportOutcome};
use sprk_models::manifest::Manifest;
use sprk_models::remote::{self, Fetcher, RemoteError};
use sprk_models::store::{ModelStore, StoreKind};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

/// Where the default index lives unless policy names a mirror.
/// Compiled in from `distribution.toml` (see docs/forking.md).
pub const DEFAULT_INDEX_URL: &str = env!("SPRK_DEFAULT_MODELS_INDEX_URL");

const VERSION: &str = env!("CARGO_PKG_VERSION");

static BUSY: AtomicBool = AtomicBool::new(false);

fn emit(app: &AppHandle, p: Progress) {
    let _ = app.emit(sprk_core::payload::event::MODEL_DOWNLOAD, p);
}

fn emit_assistant(app: &AppHandle, p: Progress) {
    let _ = app.emit("assistant-model-download", p);
}

fn env_dir(var: &str) -> Option<std::path::PathBuf> {
    std::env::var_os(var).map(std::path::PathBuf::from)
}

fn index_url(p: &AppPolicy) -> String {
    download::index_url(p, DEFAULT_INDEX_URL)
}

pub fn status(p: &AppPolicy) -> ModelStatus {
    download::status(&crate::models::roots(), p, BUSY.load(Ordering::SeqCst))
}

/// Network check, no download. Used by the launch check and the "Check for updates" action.
pub fn check(f: &dyn Fetcher, p: &AppPolicy) -> Result<UpdateInfo, String> {
    download::check(f, &crate::models::roots(), p, DEFAULT_INDEX_URL)
}

/// Download and install every missing or older default pack. Blocking; run on a worker thread.
pub fn install(f: &dyn Fetcher, p: &AppPolicy, app: &AppHandle) -> Result<(), String> {
    download::install(
        f,
        &crate::models::roots(),
        p,
        DEFAULT_INDEX_URL,
        VERSION,
        &mut |step| emit(app, step),
    )
}

/// Run `install` on a worker thread and report through the `model-download` event.
pub fn start_install(app: AppHandle, p: AppPolicy) -> Result<(), String> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err("A download is already running.".into());
    }
    std::thread::spawn(move || {
        let result = fetcher_install(&app, &p);
        BUSY.store(false, Ordering::SeqCst);
        if result.is_ok() {
            // Instant Start: time and load the new model now, not at the next Start.
            crate::warm::prepare(&app);
        }
        emit(&app, Progress::finished(&result));
    });
    Ok(())
}

#[cfg(windows)]
fn fetcher_install(app: &AppHandle, p: &AppPolicy) -> Result<(), String> {
    install(&crate::net::WinHttp, p, app)
}

#[cfg(not(windows))]
fn fetcher_install(app: &AppHandle, p: &AppPolicy) -> Result<(), String> {
    install(&crate::net_unix::Https, p, app)
}

#[cfg(windows)]
pub fn check_now(p: &AppPolicy) -> Result<UpdateInfo, String> {
    check(&crate::net::WinHttp, p)
}

#[cfg(not(windows))]
pub fn check_now(p: &AppPolicy) -> Result<UpdateInfo, String> {
    check(&crate::net_unix::Https, p)
}
/// Silent check at launch. Failure is silent (offline, blocked, server down): the user simply
/// sees no update notice. Emits `model-update` only when something is available.
pub fn launch_check(app: AppHandle, p: AppPolicy) {
    if !update_check_allowed(&p) {
        return;
    }
    std::thread::spawn(move || {
        if let Ok(info) = check_now(&p) {
            if info.available {
                let _ = app.emit("model-update", info);
            }
        }
        // Ask Sprk: only for a size the user already has, never to pull a new multi-GB pack.
        for tier in ["titles", "light", "standard"] {
            let have = assistant_status(&p)
                .installed
                .iter()
                .any(|i| i.tier == tier);
            if !have {
                continue;
            }
            if let Ok(info) = assistant_check_now(&p, tier) {
                if info.available {
                    let _ = app.emit("assistant-model-update", info);
                    break;
                }
            }
        }
    });
}

// ---------------------------------------------------------------- Ask Sprk model (ADR-010)

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AssistantPackInfo {
    pub id: String,
    pub version: String,
    pub tier: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AssistantModelStatus {
    /// Every assistant pack on this PC.
    pub installed: Vec<AssistantPackInfo>,
    /// At least one pack is installed, so Ask Sprk can be turned on.
    pub any_installed: bool,
    pub download_enabled: bool,
    pub update_check_enabled: bool,
    pub custom_source: bool,
    pub busy: bool,
}

static ASSISTANT_BUSY: AtomicBool = AtomicBool::new(false);

fn assistant_stores() -> Vec<ModelStore> {
    let roots = sprk_assistant::runtime::pack_roots(
        crate::models::user_data_dir().as_deref(),
        env_dir("ProgramData").as_deref(),
    );
    roots
        .into_iter()
        .map(|r| ModelStore::for_kind(r, StoreKind::User, sprk_models::manifest::KIND_ASSISTANT))
        .collect()
}

fn assistant_tier_of(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("model.json")).ok()?;
    Manifest::from_json(&text)
        .ok()
        .and_then(|m| m.assistant)
        .map(|a| a.tier)
}

pub fn assistant_status(p: &AppPolicy) -> AssistantModelStatus {
    let mut installed = Vec::new();
    for s in assistant_stores() {
        for i in s.list() {
            if let Some(tier) = assistant_tier_of(&i.dir) {
                installed.push(AssistantPackInfo {
                    id: i.id,
                    version: i.version,
                    tier,
                });
            }
        }
    }
    AssistantModelStatus {
        any_installed: !installed.is_empty(),
        installed,
        download_enabled: download_allowed(p),
        update_check_enabled: update_check_allowed(p),
        custom_source: p.model_index_url.is_some(),
        busy: ASSISTANT_BUSY.load(Ordering::SeqCst),
    }
}

/// The size to fetch: the user's choice, capped by the admin's `AssistantTier`. `titles` is the
/// smallest pack; anything unknown falls back to `light`.
fn wanted_tier(p: &AppPolicy, tier: &str) -> &'static str {
    use sprk_assistant::governor::Tier;
    let cap = p
        .assistant_tier
        .as_deref()
        .and_then(Tier::parse)
        .unwrap_or(Tier::Standard);
    let asked = Tier::parse(tier).unwrap_or(Tier::Light).min(cap);
    match asked {
        Tier::Standard => "standard",
        Tier::Light => "light",
        _ => "titles",
    }
}

/// The user store packs are installed into (per user, no admin rights needed).
fn assistant_user_store() -> Option<ModelStore> {
    crate::models::user_data_dir().map(|d| {
        ModelStore::for_kind(
            d.join("SprkNotes").join("assistant-models"),
            StoreKind::User,
            sprk_models::manifest::KIND_ASSISTANT,
        )
    })
}

/// Network check for a newer pack of `tier`. No download.
pub fn assistant_check(f: &dyn Fetcher, p: &AppPolicy, tier: &str) -> Result<UpdateInfo, String> {
    if !download_allowed(p) {
        return Err(friendly(&RemoteError::Disabled));
    }
    let tier = wanted_tier(p, tier);
    let index = remote::fetch_index(f, &index_url(p)).map_err(|e| friendly(&e))?;
    let Some(user) = assistant_user_store() else {
        return Err("No user data folder is available.".into());
    };
    // A pack in the machine folder counts as installed too.
    let mut packs = Vec::new();
    if let Some(u) = remote::pending_assistant(&index, &user, tier) {
        let machine_has = assistant_stores().iter().any(|s| {
            s.list()
                .iter()
                .any(|i| i.id == u.id && !remote::newer(&u.version, &i.version))
        });
        if !machine_has {
            packs.push(PackInfo {
                id: u.id,
                version: u.version,
                installed: u.installed,
            });
        }
    }
    Ok(UpdateInfo {
        available: !packs.is_empty(),
        packs,
    })
}

/// Download and install the assistant pack for `tier`. Blocking; run on a worker thread.
pub fn assistant_install(
    f: &dyn Fetcher,
    p: &AppPolicy,
    app: &AppHandle,
    tier: &str,
) -> Result<(), String> {
    if !download_allowed(p) {
        return Err(friendly(&RemoteError::Disabled));
    }
    let tier = wanted_tier(p, tier);
    emit_assistant(app, Progress::stage("checking", None));
    let index = remote::fetch_index(f, &index_url(p)).map_err(|e| friendly(&e))?;
    let Some(entry) = index.assistant_entry(tier) else {
        return Err("This assistant size is not available for download yet.".into());
    };
    let user = assistant_user_store().ok_or("No user data folder is available.")?;
    let engines = supported_engines();
    let trust = trust(p, VERSION);
    let verifier = trust.verifier();
    let now = now_stamp();
    let opts = ImportOptions {
        verify: trust.verify_options(&engines, &verifier),
        activate: true,
        keep_versions: keep_versions(p),
        now: &now,
    };
    let id = format!("{}@{}", entry.id, entry.version);
    let app2 = app.clone();
    let name = id.clone();
    let mut progress = move |done: u64, total: Option<u64>| {
        emit_assistant(
            &app2,
            Progress {
                done,
                total,
                ..Progress::stage("downloading", Some(name.clone()))
            },
        );
    };
    let outcome = remote::download_and_install(
        f,
        entry,
        &user,
        &opts,
        &Smoke {
            app_version: VERSION,
        },
        &mut progress,
    )
    .map_err(|e| friendly(&e))?;
    emit_assistant(app, Progress::stage("verifying", Some(id)));
    if let ImportOutcome::Rejected { reason } = outcome {
        return Err(format!("The downloaded model was rejected: {reason}"));
    }
    Ok(())
}

pub fn assistant_start_install(app: AppHandle, p: AppPolicy, tier: String) -> Result<(), String> {
    if ASSISTANT_BUSY.swap(true, Ordering::SeqCst) {
        return Err("A download is already running.".into());
    }
    std::thread::spawn(move || {
        #[cfg(windows)]
        let result = assistant_install(&crate::net::WinHttp, &p, &app, &tier);
        #[cfg(not(windows))]
        let result = assistant_install(&crate::net_unix::Https, &p, &app, &tier);
        ASSISTANT_BUSY.store(false, Ordering::SeqCst);
        emit_assistant(&app, Progress::finished(&result));
    });
    Ok(())
}

#[cfg(windows)]
pub fn assistant_check_now(p: &AppPolicy, tier: &str) -> Result<UpdateInfo, String> {
    assistant_check(&crate::net::WinHttp, p, tier)
}

#[cfg(not(windows))]
pub fn assistant_check_now(p: &AppPolicy, tier: &str) -> Result<UpdateInfo, String> {
    assistant_check(&crate::net_unix::Https, p, tier)
}

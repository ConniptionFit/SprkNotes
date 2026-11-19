//! "Download model" and the launch-time update check (ADR-005). Model packs only: an index file and
//! the packs it lists, over https, verified against the index checksum and then the normal import
//! rules. Admin policy can switch it off (`ModelDownload`, `ModelUpdateCheck`) or point it at an
//! internal mirror (`ModelIndexUrl`). Never touches audio or transcripts.
use serde::Serialize;
use sprk_config::AppPolicy;
use sprk_models::import::{ImportOptions, ImportOutcome, SmokeTester};
use sprk_models::manifest::Manifest;
use sprk_models::remote::{self, Fetcher, RemoteError};
use sprk_models::sshsig::parse_keys;
use sprk_models::store::{ModelStore, StoreKind};
use sprk_models::trust::Trust;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

/// Where the default index lives unless policy names a mirror.
pub const DEFAULT_INDEX_URL: &str =
    "https://git.pwrs.dev/powers/sprknotes/releases/download/models/models-index.json";

static BUSY: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PackInfo {
    pub id: String,
    pub version: String,
    pub installed: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// A pack is missing or older than the index's.
    pub available: bool,
    pub packs: Vec<PackInfo>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    /// A transcription pack and the voice activity pack are both present somewhere.
    pub ready: bool,
    pub download_enabled: bool,
    pub update_check_enabled: bool,
    /// Policy points at an internal mirror.
    pub custom_source: bool,
    pub busy: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    /// `checking`, `downloading`, `verifying`, `done` or `error`.
    stage: &'static str,
    pack: Option<String>,
    done: u64,
    total: Option<u64>,
    /// A plain sentence for the UI, never a URL.
    error: Option<String>,
}

fn emit(app: &AppHandle, p: Progress) {
    let _ = app.emit("model-download", p);
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

fn stores() -> (Option<ModelStore>, Option<ModelStore>, Option<ModelStore>) {
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("models")))
        .filter(|p| p.is_dir())
        .map(|p| ModelStore::new(p, StoreKind::Bundled));
    let machine = env_dir("ProgramData")
        .map(|p| ModelStore::new(p.join("SprkNotes").join("models"), StoreKind::Machine));
    let user = crate::models::user_data_dir()
        .map(|p| ModelStore::new(p.join("SprkNotes").join("models"), StoreKind::User));
    (bundled, machine, user)
}

pub fn download_allowed(p: &AppPolicy) -> bool {
    p.model_download.unwrap_or(true)
}

pub fn update_check_allowed(p: &AppPolicy) -> bool {
    download_allowed(p) && p.model_update_check.unwrap_or(true)
}

fn index_url(p: &AppPolicy) -> String {
    p.model_index_url
        .clone()
        .unwrap_or_else(|| DEFAULT_INDEX_URL.to_string())
}

fn engine_of(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("model.json")).ok()?;
    Manifest::from_json(&text).ok().map(|m| m.engine)
}

pub fn status(p: &AppPolicy) -> ModelStatus {
    let (b, m, u) = stores();
    let (mut asr, mut vad) = (false, false);
    for s in [&b, &m, &u].into_iter().flatten() {
        for i in s.list() {
            match engine_of(&i.dir).as_deref() {
                Some("sherpa-onnx/vad-silero") => vad = true,
                Some(_) => asr = true,
                None => {}
            }
        }
    }
    ModelStatus {
        ready: asr && vad,
        download_enabled: download_allowed(p),
        update_check_enabled: update_check_allowed(p),
        custom_source: p.model_index_url.is_some(),
        busy: BUSY.load(Ordering::SeqCst),
    }
}

fn friendly(e: &RemoteError) -> String {
    match e {
        RemoteError::Disabled => "Model downloads are turned off by your organization.".into(),
        RemoteError::Network => {
            "Could not reach the model server. Check the network connection and try again.".into()
        }
        RemoteError::Disk => "Not enough disk space, or the models folder is not writable.".into(),
        other => format!("{other}."),
    }
}

/// Network check, no download. Used by the launch check and the "Check for updates" action.
pub fn check(f: &dyn Fetcher, p: &AppPolicy) -> Result<UpdateInfo, String> {
    if !update_check_allowed(p) && !download_allowed(p) {
        return Err(friendly(&RemoteError::Disabled));
    }
    let index = remote::fetch_index(f, &index_url(p)).map_err(|e| friendly(&e))?;
    let (b, m, u) = stores();
    let all: Vec<&ModelStore> = [&b, &m, &u].into_iter().flatten().collect();
    let packs: Vec<PackInfo> = remote::pending(&index, &all)
        .into_iter()
        .map(|u| PackInfo {
            id: u.id,
            version: u.version,
            installed: u.installed,
        })
        .collect();
    Ok(UpdateInfo {
        available: !packs.is_empty(),
        packs,
    })
}

/// Same smoke test as `sprkctl`: the pack must load through its engine adapter.
struct Smoke;
impl SmokeTester for Smoke {
    fn smoke(&self, dir: &Path, manifest: &Manifest) -> Result<(), String> {
        #[cfg(any(feature = "engine", feature = "engine-shared"))]
        {
            if manifest.engine == "sherpa-onnx/vad-silero" {
                return sprk_asr::sherpa::SileroVad::load(manifest, dir, 20.0)
                    .map(|_| ())
                    .map_err(|e| e.to_string());
            }
            let mut reg = sprk_asr::Registry::default();
            sprk_asr::sherpa::register(&mut reg);
            reg.load(manifest, dir, env!("CARGO_PKG_VERSION"))
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        #[cfg(not(any(feature = "engine", feature = "engine-shared")))]
        {
            manifest.verify_files(dir).map_err(|e| e.to_string())
        }
    }
}

fn supported_engines() -> Vec<&'static str> {
    #[cfg(any(feature = "engine", feature = "engine-shared"))]
    {
        let mut reg = sprk_asr::Registry::default();
        sprk_asr::sherpa::register(&mut reg);
        let mut engines = reg.engines();
        // The voice activity model is loaded by the VAD wrapper, not the transcription registry.
        if !engines.contains(&sprk_asr::sherpa::ENGINE_VAD_SILERO) {
            engines.push(sprk_asr::sherpa::ENGINE_VAD_SILERO);
        }
        engines
    }
    #[cfg(not(any(feature = "engine", feature = "engine-shared")))]
    {
        vec![
            "sherpa-onnx/offline-transducer",
            "sherpa-onnx/whisper",
            "sherpa-onnx/vad-silero",
        ]
    }
}

fn now_stamp() -> String {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("epoch:{s}")
}

/// Download and install every missing or older default pack. Blocking; run on a worker thread.
pub fn install(f: &dyn Fetcher, p: &AppPolicy, app: &AppHandle) -> Result<(), String> {
    if !download_allowed(p) {
        return Err(friendly(&RemoteError::Disabled));
    }
    emit(
        app,
        Progress {
            stage: "checking",
            pack: None,
            done: 0,
            total: None,
            error: None,
        },
    );
    let index = remote::fetch_index(f, &index_url(p)).map_err(|e| friendly(&e))?;
    let (b, m, u) = stores();
    let all: Vec<&ModelStore> = [&b, &m, &u].into_iter().flatten().collect();
    let todo = remote::pending(&index, &all);
    let user = u.as_ref().ok_or("No user data folder is available.")?;
    let engines = supported_engines();
    let trust = Trust {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        policy_require_signed: p.require_signed_models.unwrap_or(false),
        keys: parse_keys(&p.model_public_keys.join("\n")),
        allowed_hashes: p.allowed_model_hashes.clone(),
    };
    let verifier = trust.verifier();
    let now = now_stamp();
    for upd in todo {
        let Some(entry) = index
            .models
            .iter()
            .find(|e| e.id == upd.id && e.version == upd.version)
        else {
            continue;
        };
        let opts = ImportOptions {
            verify: trust.verify_options(&engines, &verifier),
            activate: true,
            keep_versions: p.keep_model_versions.map_or(2, |k| k.clamp(1, 10) as usize),
            now: &now,
        };
        let id = format!("{}@{}", entry.id, entry.version);
        let app2 = app.clone();
        let name = id.clone();
        let mut progress = move |done: u64, total: Option<u64>| {
            emit(
                &app2,
                Progress {
                    stage: "downloading",
                    pack: Some(name.clone()),
                    done,
                    total,
                    error: None,
                },
            );
        };
        let outcome = remote::download_and_install(f, entry, user, &opts, &Smoke, &mut progress)
            .map_err(|e| friendly(&e))?;
        emit(
            app,
            Progress {
                stage: "verifying",
                pack: Some(id),
                done: 0,
                total: None,
                error: None,
            },
        );
        if let ImportOutcome::Rejected { reason } = outcome {
            // The reason is about the pack (hash, signature, engine), never about the user.
            return Err(format!("The downloaded model was rejected: {reason}"));
        }
    }
    Ok(())
}

/// Run `install` on a worker thread and report through the `model-download` event.
pub fn start_install(app: AppHandle, p: AppPolicy) -> Result<(), String> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err("A download is already running.".into());
    }
    std::thread::spawn(move || {
        let result = fetcher_install(&app, &p);
        BUSY.store(false, Ordering::SeqCst);
        let ev = match result {
            Ok(()) => Progress {
                stage: "done",
                pack: None,
                done: 0,
                total: None,
                error: None,
            },
            Err(e) => Progress {
                stage: "error",
                pack: None,
                done: 0,
                total: None,
                error: Some(e),
            },
        };
        emit(&app, ev);
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
    });
}

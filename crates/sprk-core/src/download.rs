//! "Download model" for the transcription packs (ADR-005), shared by the desktop and phone apps.
//! Model packs only: an index file and the packs it lists, over https, verified against the index
//! checksum and then the normal import rules. Admin policy can switch it off (`ModelDownload`,
//! `ModelUpdateCheck`) or point it at an internal mirror (`ModelIndexUrl`). Never touches audio
//! or transcripts. Each shell brings its own https client ([`Fetcher`]) and sends [`Progress`] to
//! its window; the assistant packs (desktop only) stay in the desktop shell.
use crate::models::{stores, Roots};
use serde::Serialize;
use sprk_config::AppPolicy;
use sprk_models::import::{ImportOptions, ImportOutcome, SmokeTester};
use sprk_models::manifest::Manifest;
use sprk_models::remote::{self, Fetcher, RemoteError};
use sprk_models::sshsig::parse_keys;
use sprk_models::store::ModelStore;
use sprk_models::trust::Trust;
use std::path::Path;

/// Engine id of assistant packs (ADR-010).
pub const ASSISTANT_ENGINE: &str = "llama-cpp";

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PackInfo {
    pub id: String,
    pub version: String,
    pub installed: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// A pack is missing or older than the index's.
    pub available: bool,
    pub packs: Vec<PackInfo>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
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

/// What the window shows while a download runs (the `model-download` event).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// `checking`, `downloading`, `verifying`, `done` or `error`.
    pub stage: &'static str,
    pub pack: Option<String>,
    pub done: u64,
    pub total: Option<u64>,
    /// A plain sentence for the UI, never a URL.
    pub error: Option<String>,
}

impl Progress {
    pub fn stage(stage: &'static str, pack: Option<String>) -> Self {
        Progress {
            stage,
            pack,
            done: 0,
            total: None,
            error: None,
        }
    }
    pub fn finished(result: &Result<(), String>) -> Self {
        match result {
            Ok(()) => Progress::stage("done", None),
            Err(e) => Progress {
                error: Some(e.clone()),
                ..Progress::stage("error", None)
            },
        }
    }
}

pub fn download_allowed(p: &AppPolicy) -> bool {
    p.model_download.unwrap_or(true)
}

pub fn update_check_allowed(p: &AppPolicy) -> bool {
    download_allowed(p) && p.model_update_check.unwrap_or(true)
}

/// The policy's mirror, else the build's default index.
pub fn index_url(p: &AppPolicy, default_url: &str) -> String {
    p.model_index_url
        .clone()
        .unwrap_or_else(|| default_url.to_string())
}

fn engine_of(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("model.json")).ok()?;
    Manifest::from_json(&text).ok().map(|m| m.engine)
}

fn all(roots: &Roots) -> Vec<ModelStore> {
    let (b, m, u) = stores(roots);
    [b, m, u].into_iter().flatten().collect()
}

pub fn status(roots: &Roots, p: &AppPolicy, busy: bool) -> ModelStatus {
    let (mut asr, mut vad) = (false, false);
    for s in all(roots) {
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
        busy,
    }
}

pub fn friendly(e: &RemoteError) -> String {
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
pub fn check(
    f: &dyn Fetcher,
    roots: &Roots,
    p: &AppPolicy,
    default_url: &str,
) -> Result<UpdateInfo, String> {
    if !update_check_allowed(p) && !download_allowed(p) {
        return Err(friendly(&RemoteError::Disabled));
    }
    let index = remote::fetch_index(f, &index_url(p, default_url)).map_err(|e| friendly(&e))?;
    let stores = all(roots);
    let refs: Vec<&ModelStore> = stores.iter().collect();
    let packs: Vec<PackInfo> = remote::pending(&index, &refs)
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
pub struct Smoke<'a> {
    /// The running app's version, checked against the pack's `min_app_version`.
    pub app_version: &'a str,
}
impl SmokeTester for Smoke<'_> {
    fn smoke(&self, dir: &Path, manifest: &Manifest) -> Result<(), String> {
        if manifest.kind == sprk_models::manifest::KIND_ASSISTANT {
            // The helper does its own load check at first use; here the files must match.
            return manifest.verify_files(dir).map_err(|e| e.to_string());
        }
        #[cfg(any(feature = "engine", feature = "engine-shared"))]
        {
            if manifest.engine == sprk_asr::sherpa::ENGINE_VAD_SILERO {
                return sprk_asr::sherpa::SileroVad::load(manifest, dir, 20.0)
                    .map(|_| ())
                    .map_err(|e| e.to_string());
            }
            let mut reg = sprk_asr::Registry::default();
            sprk_asr::sherpa::register(&mut reg);
            reg.load(manifest, dir, self.app_version)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        #[cfg(not(any(feature = "engine", feature = "engine-shared")))]
        {
            manifest.verify_files(dir).map_err(|e| e.to_string())
        }
    }
}

/// Engines this build can load, for the import check.
pub fn supported_engines() -> Vec<&'static str> {
    #[cfg(any(feature = "engine", feature = "engine-shared"))]
    {
        let mut reg = sprk_asr::Registry::default();
        sprk_asr::sherpa::register(&mut reg);
        let mut engines = reg.engines();
        engines.push(ASSISTANT_ENGINE);
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
            ASSISTANT_ENGINE,
        ]
    }
}

pub fn now_stamp() -> String {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("epoch:{s}")
}

/// The admin's trust rules for packs (signatures, allowed hashes).
pub fn trust(p: &AppPolicy, app_version: &str) -> Trust {
    Trust {
        app_version: app_version.to_string(),
        policy_require_signed: p.require_signed_models.unwrap_or(false),
        keys: parse_keys(&p.model_public_keys.join("\n")),
        allowed_hashes: p.allowed_model_hashes.clone(),
    }
}

/// How many versions of a model to keep (policy `KeepModelVersions`, 1 to 10, default 2).
pub fn keep_versions(p: &AppPolicy) -> usize {
    p.keep_model_versions.map_or(2, |k| k.clamp(1, 10) as usize)
}

/// Download and install every missing or older default pack into the user store.
/// Blocking; run on a worker thread. `emit` receives every progress step but the final one.
pub fn install(
    f: &dyn Fetcher,
    roots: &Roots,
    p: &AppPolicy,
    default_url: &str,
    app_version: &str,
    emit: &mut dyn FnMut(Progress),
) -> Result<(), String> {
    if !download_allowed(p) {
        return Err(friendly(&RemoteError::Disabled));
    }
    emit(Progress::stage("checking", None));
    let index = remote::fetch_index(f, &index_url(p, default_url)).map_err(|e| friendly(&e))?;
    let (b, m, u) = stores(roots);
    let refs: Vec<&ModelStore> = [&b, &m, &u].into_iter().flatten().collect();
    let todo = remote::pending(&index, &refs);
    let user = u.as_ref().ok_or("No user data folder is available.")?;
    let engines = supported_engines();
    let trust = trust(p, app_version);
    let verifier = trust.verifier();
    let now = now_stamp();
    let smoke = Smoke { app_version };
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
            // Only transcription models become the active model; the voice activity pack
            // is a helper and must not displace it.
            activate: entry.role == "transcription",
            keep_versions: keep_versions(p),
            now: &now,
        };
        let id = format!("{}@{}", entry.id, entry.version);
        let name = id.clone();
        let mut progress = |done: u64, total: Option<u64>| {
            emit(Progress {
                done,
                total,
                ..Progress::stage("downloading", Some(name.clone()))
            });
        };
        let outcome = remote::download_and_install(f, entry, user, &opts, &smoke, &mut progress)
            .map_err(|e| friendly(&e))?;
        emit(Progress::stage("verifying", Some(id)));
        if let ImportOutcome::Rejected { reason } = outcome {
            // The reason is about the pack (hash, signature, engine), never about the user.
            return Err(format!("The downloaded model was rejected: {reason}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_switches() {
        let mut p = AppPolicy::default();
        assert!(download_allowed(&p) && update_check_allowed(&p));
        p.model_update_check = Some(false);
        assert!(download_allowed(&p) && !update_check_allowed(&p));
        p.model_download = Some(false);
        assert!(!download_allowed(&p) && !update_check_allowed(&p));
        assert_eq!(index_url(&p, "https://d/x.json"), "https://d/x.json");
        p.model_index_url = Some("https://mirror/i.json".into());
        assert_eq!(index_url(&p, "https://d/x.json"), "https://mirror/i.json");
    }

    #[test]
    fn empty_store_is_not_ready() {
        let d = std::env::temp_dir().join("sprk-core-download-empty");
        let r = Roots {
            user: Some(d),
            ..Default::default()
        };
        let s = status(&r, &AppPolicy::default(), false);
        assert!(!s.ready && s.download_enabled && !s.busy);
    }

    #[test]
    fn disabled_download_stops_before_the_network() {
        struct NoNet;
        impl Fetcher for NoNet {
            fn get_bytes(&self, _: &str, _: u64) -> Result<Vec<u8>, RemoteError> {
                panic!("must not fetch")
            }
            fn get_to_file(
                &self,
                _: &str,
                _: &Path,
                _: u64,
                _: &mut dyn FnMut(u64, Option<u64>),
            ) -> Result<(), RemoteError> {
                panic!("must not fetch")
            }
        }
        let p = AppPolicy {
            model_download: Some(false),
            ..Default::default()
        };
        let r = Roots::default();
        let mut seen = Vec::new();
        let e = install(&NoNet, &r, &p, "https://d/i.json", "0.0.1", &mut |x| {
            seen.push(x)
        });
        assert!(e.unwrap_err().contains("turned off"));
        assert!(seen.is_empty());
    }

    #[test]
    fn progress_shape_matches_the_window() {
        let v = serde_json::to_value(Progress::finished(&Err("x".into()))).unwrap_or_default();
        assert_eq!(v["stage"], "error");
        assert_eq!(v["error"], "x");
        assert!(v.get("done").is_some() && v.get("total").is_some());
    }
}

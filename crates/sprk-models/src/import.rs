//! Import and promotion flow (SPEC 16.6): detect, verify, extract to staging, atomic rename,
//! smoke test, activate, clean up. Any failure rejects the pack and keeps the current model.
use crate::manifest::Manifest;
use crate::pack::{extract_pack, verify_pack, PackError, VerifyOptions};
use crate::store::ModelStore;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Loads the model in a separate worker and decodes the built-in fixture (SPEC 16.6 step 4).
pub trait SmokeTester {
    fn smoke(&self, dir: &Path, manifest: &Manifest) -> Result<(), String>;
}

pub struct ImportOptions<'a> {
    pub verify: VerifyOptions<'a>,
    /// False for `--no-activate` or when policy pins another model.
    pub activate: bool,
    pub keep_versions: usize,
    /// ISO 8601 timestamp for `active.json` and the log (the caller owns the clock).
    pub now: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ImportOutcome {
    Installed { id_version: String, activated: bool },
    Rejected { reason: String },
}

fn log(store: &ModelStore, pack: &Path, result: &str, reason: &str, now: &str) {
    let name = pack
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let line = serde_json::json!({ "time": now, "pack": name, "result": result, "reason": reason });
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(store.root().join("import-log.jsonl"))
    {
        let _ = writeln!(f, "{line}");
    }
}

fn reject(store: &ModelStore, pack: &Path, reason: String, now: &str) -> ImportOutcome {
    let dir = store.root().join("incoming").join("rejected");
    if std::fs::create_dir_all(&dir).is_ok() {
        if let Some(name) = pack.file_name() {
            let target = dir.join(name);
            let _ = std::fs::rename(pack, &target);
            let mut reason_file = target.into_os_string();
            reason_file.push(".reason.txt");
            let _ = std::fs::write(PathBuf::from(reason_file), &reason);
        }
    }
    log(store, pack, "rejected", &reason, now);
    ImportOutcome::Rejected { reason }
}

/// Import one pack into `store`.
pub fn import_pack(
    store: &ModelStore,
    pack: &Path,
    o: &ImportOptions,
    smoke: &dyn SmokeTester,
) -> ImportOutcome {
    let verified = match verify_pack(pack, &o.verify) {
        Ok(v) => v,
        Err(e) => return reject(store, pack, e.to_string(), o.now),
    };
    if verified.manifest.kind != store.pack_kind() {
        let reason = format!(
            "this is a {} pack and this store holds {} packs",
            verified.manifest.kind,
            store.pack_kind()
        );
        return reject(store, pack, reason, o.now);
    }
    let iv = crate::store::id_version(&verified.manifest.id, &verified.manifest.version);
    let final_dir = store.model_dir(&iv);
    let staging = store.root().join(".staging").join(&iv);
    let _ = std::fs::remove_dir_all(&staging);
    if !final_dir.exists() {
        if let Err(e) = extract_pack(pack, &staging) {
            let _ = std::fs::remove_dir_all(&staging);
            return reject(store, pack, e.to_string(), o.now);
        }
        // Atomic rename on the same volume.
        if let Err(e) = std::fs::rename(&staging, &final_dir) {
            let _ = std::fs::remove_dir_all(&staging);
            return reject(store, pack, PackError::Io(e).to_string(), o.now);
        }
    }
    if let Err(reason) = smoke.smoke(&final_dir, &verified.manifest) {
        let _ = std::fs::remove_dir_all(&final_dir);
        return reject(store, pack, format!("smoke test failed: {reason}"), o.now);
    }
    let activated = o.activate && store.activate(&iv, o.now).is_ok();
    let done = store.root().join("incoming").join("processed");
    if std::fs::create_dir_all(&done).is_ok() {
        if let Some(name) = pack.file_name() {
            let _ = std::fs::rename(pack, done.join(name));
        }
    }
    store.prune(o.keep_versions);
    log(
        store,
        pack,
        "installed",
        &format!("{iv} activated={activated}"),
        o.now,
    );
    ImportOutcome::Installed {
        id_version: iv,
        activated,
    }
}

/// Process `incoming\`: ignore `*.partial` and files modified in the last 10 s (still copying).
pub fn process_incoming(
    store: &ModelStore,
    o: &ImportOptions,
    smoke: &dyn SmokeTester,
    now: SystemTime,
) -> Vec<(PathBuf, ImportOutcome)> {
    let dir = store.root().join("incoming");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut packs: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "sprkmodel"))
        .collect();
    packs.sort();
    let mut out = Vec::new();
    for p in packs {
        let recent = std::fs::metadata(&p)
            .and_then(|m| m.modified())
            .map(|m| now.duration_since(m).unwrap_or(Duration::ZERO) < Duration::from_secs(10))
            .unwrap_or(true);
        if recent {
            continue;
        }
        let r = import_pack(store, &p, o, smoke);
        out.push((p, r));
    }
    out
}

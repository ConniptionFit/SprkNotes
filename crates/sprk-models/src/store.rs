//! A model store folder (SPEC 16.3): `<id>@<version>\` directories plus `active.json`.
//! The same layout serves the bundled, machine, user and portable stores.
use crate::manifest::Manifest;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("file operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("model {0} is not installed")]
    NotInstalled(String),
    #[error("nothing to roll back to")]
    NoPrevious,
    #[error("cannot remove the active or previous model")]
    InUse,
    #[error("invalid active.json: {0}")]
    BadActive(String),
}

/// Which store a model lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreKind {
    Bundled,
    Machine,
    User,
    Portable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// `<id>@<version>`
    pub id_version: String,
    pub id: String,
    pub version: String,
    pub dir: PathBuf,
    pub store: StoreKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveFile {
    pub active: String,
    #[serde(default)]
    pub previous: Option<String>,
    pub updated: String,
}

pub struct ModelStore {
    root: PathBuf,
    kind: StoreKind,
}

pub fn id_version(id: &str, version: &str) -> String {
    format!("{id}@{version}")
}

impl ModelStore {
    pub fn new(root: PathBuf, kind: StoreKind) -> Self {
        Self { root, kind }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn kind(&self) -> StoreKind {
        self.kind
    }

    pub fn model_dir(&self, id_version: &str) -> PathBuf {
        self.root.join(id_version)
    }

    /// Installed models, sorted by id then version. Folders without a readable
    /// `model.json` are ignored (including `.staging` and `incoming`).
    pub fn list(&self) -> Vec<Installed> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(&self.root) else {
            return out;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || !e.path().is_dir() {
                continue;
            }
            let Some((id, version)) = name.split_once('@') else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(e.path().join("model.json")) else {
                continue;
            };
            let Ok(m) = Manifest::from_json(&text) else {
                continue;
            };
            if m.id == id && m.version == version {
                out.push(Installed {
                    id_version: name.clone(),
                    id: m.id,
                    version: m.version,
                    dir: e.path(),
                    store: self.kind,
                });
            }
        }
        out.sort_by(|a, b| (&a.id, &a.version).cmp(&(&b.id, &b.version)));
        out
    }

    pub fn get(&self, id_version: &str) -> Option<Installed> {
        self.list().into_iter().find(|i| i.id_version == id_version)
    }

    pub fn active(&self) -> Option<ActiveFile> {
        let text = std::fs::read_to_string(self.root.join("active.json")).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn write_active(&self, f: &ActiveFile) -> Result<(), StoreError> {
        use std::io::Write;
        std::fs::create_dir_all(&self.root)?;
        let tmp = self.root.join("active.json.tmp");
        let json =
            serde_json::to_string_pretty(f).map_err(|e| StoreError::BadActive(e.to_string()))?;
        {
            let mut file = std::fs::File::create(&tmp)?;
            file.write_all(json.as_bytes())?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp, self.root.join("active.json"))?; // atomic replace
        Ok(())
    }

    /// Make `id_version` active, remembering the old active as `previous`.
    pub fn activate(&self, id_version: &str, now: &str) -> Result<(), StoreError> {
        if self.get(id_version).is_none() {
            return Err(StoreError::NotInstalled(id_version.into()));
        }
        let previous = match self.active() {
            Some(a) if a.active != id_version => Some(a.active),
            Some(a) => a.previous,
            None => None,
        };
        self.write_active(&ActiveFile {
            active: id_version.into(),
            previous,
            updated: now.into(),
        })
    }

    /// Swap active and previous. Takes effect at the next session start.
    pub fn rollback(&self, now: &str) -> Result<String, StoreError> {
        let a = self.active().ok_or(StoreError::NoPrevious)?;
        let prev = a.previous.ok_or(StoreError::NoPrevious)?;
        if self.get(&prev).is_none() {
            return Err(StoreError::NotInstalled(prev));
        }
        self.write_active(&ActiveFile {
            active: prev.clone(),
            previous: Some(a.active),
            updated: now.into(),
        })?;
        Ok(prev)
    }

    pub fn remove(&self, id_version: &str) -> Result<(), StoreError> {
        let a = self.active();
        if a.as_ref()
            .is_some_and(|a| a.active == id_version || a.previous.as_deref() == Some(id_version))
        {
            return Err(StoreError::InUse);
        }
        let inst = self
            .get(id_version)
            .ok_or_else(|| StoreError::NotInstalled(id_version.into()))?;
        std::fs::remove_dir_all(inst.dir)?;
        Ok(())
    }

    /// Keep the newest `keep` versions of each model id; never the active or previous.
    /// Returns what was removed.
    pub fn prune(&self, keep: usize) -> Vec<String> {
        let a = self.active();
        let protected = |iv: &str| {
            a.as_ref()
                .is_some_and(|a| a.active == iv || a.previous.as_deref() == Some(iv))
        };
        let mut removed = Vec::new();
        let all = self.list();
        let mut ids: Vec<&String> = all.iter().map(|i| &i.id).collect();
        ids.dedup();
        for id in ids {
            let mut versions: Vec<&Installed> = all.iter().filter(|i| &i.id == id).collect();
            versions.sort_by(|a, b| b.version.cmp(&a.version)); // newest first
            for old in versions.into_iter().skip(keep.max(1)) {
                if !protected(&old.id_version) && std::fs::remove_dir_all(&old.dir).is_ok() {
                    removed.push(old.id_version.clone());
                }
            }
        }
        removed
    }
}

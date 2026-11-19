//! Which model loads (SPEC 16.4). Evaluated at app launch and at each session start; never
//! mid-session. A failing candidate is skipped and the next valid one is used, so
//! transcription keeps working.
use crate::store::{Installed, ModelStore, StoreKind};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Default)]
pub struct Policy {
    /// `PinnedModel`: `<id>@<version>` or a bare id (newest version wins).
    pub pinned: Option<String>,
    /// `AllowUserModels`
    pub allow_user_models: bool,
    /// True when any admin policy exists (then user models need `allow_user_models`).
    pub policy_present: bool,
    /// `AllowedModelHashes`: SHA-256 of `model.json`. Non-empty means restrict to these.
    pub allowed_hashes: Vec<String>,
}

pub struct Stores<'a> {
    pub bundled: Option<&'a ModelStore>,
    pub machine: Option<&'a ModelStore>,
    /// The user store, or the portable store when running from the zip.
    pub user: Option<&'a ModelStore>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    Pinned,
    MachineActive,
    MachinePrevious,
    UserActive,
    UserPrevious,
    Bundled,
}

#[derive(Debug)]
pub struct Resolution {
    pub chosen: Installed,
    pub why: Why,
    /// Candidates that were passed over and why (IDs and reasons only; for a quiet notice in Settings).
    pub skipped: Vec<(String, String)>,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("no usable transcription model")]
    NoneAvailable,
}

/// SHA-256 of a model's `model.json`, the value `AllowedModelHashes` holds.
pub fn manifest_hash(inst: &Installed) -> Option<String> {
    let bytes = std::fs::read(inst.dir.join("model.json")).ok()?;
    Some(hex::encode(Sha256::digest(bytes)))
}

fn newest_in(stores: &[&ModelStore], spec: &str) -> Option<Installed> {
    for s in stores {
        if let Some(i) = s.get(spec) {
            return Some(i);
        }
    }
    let mut best: Option<Installed> = None;
    for s in stores {
        for i in s.list().into_iter().filter(|i| i.id == spec) {
            if best.as_ref().is_none_or(|b| i.version > b.version) {
                best = Some(i);
            }
        }
    }
    best
}

/// `validate` does the heavy checks (hash verification, load test); it returns a reason on failure.
pub fn resolve(
    policy: &Policy,
    stores: &Stores,
    validate: &dyn Fn(&Installed) -> Result<(), String>,
) -> Result<Resolution, ResolveError> {
    let mut candidates: Vec<(Installed, Why)> = Vec::new();
    let all: Vec<&ModelStore> = [stores.machine, stores.user, stores.bundled]
        .into_iter()
        .flatten()
        .collect();

    if let Some(p) = &policy.pinned {
        if let Some(i) = newest_in(&all, p) {
            candidates.push((i, Why::Pinned));
        }
    }
    if let Some(m) = stores.machine {
        if let Some(a) = m.active() {
            if let Some(i) = m.get(&a.active) {
                candidates.push((i, Why::MachineActive));
            }
            if let Some(i) = a.previous.as_deref().and_then(|p| m.get(p)) {
                candidates.push((i, Why::MachinePrevious));
            }
        }
    }
    if let Some(u) = stores.user {
        if policy.allow_user_models || !policy.policy_present {
            if let Some(a) = u.active() {
                if let Some(i) = u.get(&a.active) {
                    candidates.push((i, Why::UserActive));
                }
                if let Some(i) = a.previous.as_deref().and_then(|p| u.get(p)) {
                    candidates.push((i, Why::UserPrevious));
                }
            }
        }
    }
    if let Some(b) = stores.bundled {
        let pick = b.active().and_then(|a| b.get(&a.active)).or_else(|| {
            b.list()
                .into_iter()
                .max_by(|x, y| x.version.cmp(&y.version))
        });
        if let Some(i) = pick {
            candidates.push((i, Why::Bundled));
        }
    }

    let mut skipped = Vec::new();
    for (inst, why) in candidates {
        // The bundled default is always trusted; everything else obeys the hash allow-list.
        if inst.store != StoreKind::Bundled && !policy.allowed_hashes.is_empty() {
            let ok = manifest_hash(&inst).is_some_and(|h| {
                policy
                    .allowed_hashes
                    .iter()
                    .any(|a| a.eq_ignore_ascii_case(&h))
            });
            if !ok {
                skipped.push((
                    inst.id_version.clone(),
                    "Unverified: not on the allow-list".into(),
                ));
                continue;
            }
        }
        match validate(&inst) {
            Ok(()) => {
                return Ok(Resolution {
                    chosen: inst,
                    why,
                    skipped,
                })
            }
            Err(r) => skipped.push((inst.id_version.clone(), r)),
        }
    }
    Err(ResolveError::NoneAvailable)
}

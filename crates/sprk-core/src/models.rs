//! Finds and loads the active transcription model through the resolver (SPEC 16.4).
//! No model name or path is built in: stores are folders, the resolver picks, a failing
//! candidate falls back to the next one. Decided at session start only, never mid-session.
//! Each shell says where its stores are ([`Roots`]): next to the exe, `%ProgramData%` and
//! `%APPDATA%` on Windows; the app's private data folder on a phone.
use sprk_asr::Engine;
use sprk_models::store::{ModelStore, StoreKind};
use sprk_pipeline::session::VadFactory;
use std::path::PathBuf;

pub struct Loaded {
    /// `id@version`, written into the transcript front matter.
    pub label: String,
    pub engine: Box<dyn Engine>,
    pub make_vad: VadFactory,
}

/// Where the model stores are, most trusted first. Missing ones are skipped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roots {
    /// Shipped with the app (read-only).
    pub bundled: Option<PathBuf>,
    /// Installed for every user by IT.
    pub machine: Option<PathBuf>,
    /// Installed or downloaded by this user.
    pub user: Option<PathBuf>,
}

fn stores(r: &Roots) -> (Option<ModelStore>, Option<ModelStore>, Option<ModelStore>) {
    let bundled = r
        .bundled
        .clone()
        .filter(|p| p.is_dir())
        .map(|p| ModelStore::new(p, StoreKind::Bundled));
    let machine = r
        .machine
        .clone()
        .map(|p| ModelStore::new(p, StoreKind::Machine));
    let user = r.user.clone().map(|p| ModelStore::new(p, StoreKind::User));
    (bundled, machine, user)
}

/// Friendly name of the model the next session will use (no loading, no hashing), for the
/// "Loading model" message. None when nothing is installed.
pub fn active_display_name(roots: &Roots) -> Option<String> {
    use sprk_models::resolver::{resolve, Policy, Stores};
    use sprk_models::Manifest;
    let (b, m, u) = stores(roots);
    let st = Stores {
        bundled: b.as_ref(),
        machine: m.as_ref(),
        user: u.as_ref(),
    };
    let r = resolve(&Policy::default(), &st, &|_| Ok(())).ok()?;
    let text = std::fs::read_to_string(r.chosen.dir.join("model.json")).ok()?;
    Some(Manifest::from_json(&text).ok()?.display_name)
}

#[cfg(any(feature = "engine", feature = "engine-shared"))]
pub fn load_active(roots: &Roots, app_version: &str) -> Result<Loaded, String> {
    use sprk_models::resolver::{resolve, Policy, Stores};
    use sprk_models::Manifest;
    use std::cell::RefCell;
    let (b, m, u) = stores(roots);
    let st = Stores {
        bundled: b.as_ref(),
        machine: m.as_ref(),
        user: u.as_ref(),
    };
    let mut reg = sprk_asr::Registry::default();
    sprk_asr::sherpa::register(&mut reg);
    let slot: RefCell<Option<Box<dyn Engine>>> = RefCell::new(None);
    // Full load as part of validation, so a model that fails to load is skipped for the next.
    let validate = |i: &sprk_models::store::Installed| -> Result<(), String> {
        let text = std::fs::read_to_string(i.dir.join("model.json"))
            .map_err(|_| "model.json unreadable".to_string())?;
        let man = Manifest::from_json(&text).map_err(|e| e.to_string())?;
        let eng = reg
            .load(&man, &i.dir, app_version)
            .map_err(|e| e.to_string())?;
        *slot.borrow_mut() = Some(eng);
        Ok(())
    };
    let r = resolve(&Policy::default(), &st, &validate).map_err(|e| e.to_string())?;
    let engine = slot.into_inner().ok_or("no engine loaded")?;
    let make_vad = vad_factory(&[b.as_ref(), m.as_ref(), u.as_ref()])?;
    Ok(Loaded {
        label: r.chosen.id_version,
        engine,
        make_vad,
    })
}

/// The voice activity detector is a pack too: found by its engine string, not by name.
#[cfg(any(feature = "engine", feature = "engine-shared"))]
fn vad_factory(stores: &[Option<&ModelStore>]) -> Result<VadFactory, String> {
    use sprk_models::Manifest;
    for s in stores.iter().flatten() {
        for i in s.list() {
            let Ok(text) = std::fs::read_to_string(i.dir.join("model.json")) else {
                continue;
            };
            let Ok(man) = Manifest::from_json(&text) else {
                continue;
            };
            if man.engine == sprk_asr::sherpa::ENGINE_VAD_SILERO {
                let dir = i.dir.clone();
                // Verify once now so a bad pack fails Start with a clear message.
                sprk_asr::sherpa::SileroVad::load(&man, &dir, 20.0).map_err(|e| e.to_string())?;
                return Ok(Box::new(move || {
                    match sprk_asr::sherpa::SileroVad::load(&man, &dir, 20.0) {
                        Ok(v) => Box::new(v),
                        // Verified above; an unreadable pack later yields a VAD that hears nothing.
                        Err(_) => Box::new(NoVad),
                    }
                }));
            }
        }
    }
    Err("no voice activity pack installed".into())
}

#[cfg(any(feature = "engine", feature = "engine-shared"))]
struct NoVad;
#[cfg(any(feature = "engine", feature = "engine-shared"))]
impl sprk_asr::Vad for NoVad {
    fn accept(&mut self, _: &[f32]) -> Vec<sprk_asr::Utterance> {
        Vec::new()
    }
    fn flush(&mut self) -> Vec<sprk_asr::Utterance> {
        Vec::new()
    }
}

#[cfg(not(any(feature = "engine", feature = "engine-shared")))]
pub fn load_active(roots: &Roots, _app_version: &str) -> Result<Loaded, String> {
    let _ = stores(roots);
    Err("This build has no speech engine.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_installed_means_no_name() {
        let d = std::env::temp_dir().join("sprk-core-no-models");
        let r = Roots {
            user: Some(d),
            ..Default::default()
        };
        assert_eq!(active_display_name(&r), None);
        assert!(load_active(&r, "0.0.0").is_err());
    }
}

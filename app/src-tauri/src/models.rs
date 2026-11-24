//! Finds and loads the active transcription model through the resolver (SPEC 16.4).
//! No model name or path is built in: stores are folders, the resolver picks, a failing
//! candidate falls back to the next one. Decided at session start only, never mid-session.
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

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

/// Per-user data root: `%APPDATA%` on Windows, `$XDG_DATA_HOME` (or `~/.local/share`) elsewhere.
pub fn user_data_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        return env_dir("APPDATA");
    }
    env_dir("XDG_DATA_HOME")
        .filter(|p| p.is_absolute())
        .or_else(|| env_dir("HOME").map(|h| h.join(".local").join("share")))
}

/// bundled (next to the exe), machine (`%ProgramData%`), user (`%APPDATA%`).
fn stores() -> (Option<ModelStore>, Option<ModelStore>, Option<ModelStore>) {
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("models")))
        .filter(|p| p.is_dir())
        .map(|p| ModelStore::new(p, StoreKind::Bundled));
    let machine = env_dir("ProgramData")
        .map(|p| ModelStore::new(p.join("SprkNotes").join("models"), StoreKind::Machine));
    let user = user_data_dir()
        .map(|p| ModelStore::new(p.join("SprkNotes").join("models"), StoreKind::User));
    (bundled, machine, user)
}

/// Friendly name of the model the next session will use (no loading, no hashing), for the
/// "Loading model" message. None when nothing is installed.
pub fn active_display_name() -> Option<String> {
    use sprk_models::resolver::{resolve, Policy, Stores};
    use sprk_models::Manifest;
    let (b, m, u) = stores();
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
pub fn load_active(app_version: &str) -> Result<Loaded, String> {
    use sprk_models::resolver::{resolve, Policy, Stores};
    use sprk_models::Manifest;
    use std::cell::RefCell;
    let (b, m, u) = stores();
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
pub fn load_active(_app_version: &str) -> Result<Loaded, String> {
    let _ = stores();
    Err("This build has no speech engine.".into())
}

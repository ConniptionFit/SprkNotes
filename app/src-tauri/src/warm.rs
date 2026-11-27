//! Instant Start (SPEC 4.6): the transcription model is loaded in the background (after
//! launch, when the main window comes back, when "Start a transcript?" shows, after a model
//! download) and kept loaded between sessions, so Start does not wait for it. Every session
//! shares the one loaded engine (`SharedEngine`; one session runs at a time). It is loaded
//! again only when the active model or the speed check's thread count changes, or after a
//! decode panicked. If Start comes before it is ready, the session starts capturing anyway and
//! gets the engine the moment it is (the host keeps the audio in memory meanwhile).
//!
//! Memory: the model stays resident while SprkNotes runs (about 1 GB for the default model),
//! the price of starting instantly.
use crate::models;
use sprk_asr::SharedEngine;
use sprk_pipeline::host::{EngineSource, LoadedEngine};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

type Waiter = Sender<Result<LoadedEngine, String>>;

enum Slot {
    Empty,
    Loading {
        key: String,
        waiters: Vec<Waiter>,
    },
    Ready {
        key: String,
        label: String,
        engine: SharedEngine,
    },
}

/// Tauri state: the warm model.
pub struct Warm {
    slot: Arc<Mutex<Slot>>,
}

impl Default for Warm {
    fn default() -> Self {
        Self {
            slot: Arc::new(Mutex::new(Slot::Empty)),
        }
    }
}

fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// What decides whether the loaded model can be reused: which model the resolver picks (and
/// from which folder) and the speed check's thread count for it. Cheap: reads model.json files
/// only. Also returns the model's `id@version`.
fn active_key() -> Result<(String, String, Option<u32>), String> {
    let (label, dir) = models::active_choice()?;
    let threads = crate::speed::threads_for(&label);
    let key = format!(
        "{label}|{}|{}",
        dir.display(),
        threads.map_or_else(|| "default".to_string(), |n| n.to_string())
    );
    Ok((key, label, threads))
}

impl Warm {
    fn lock(&self) -> std::sync::MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The engine for a session that starts now, and the `id@version` to write into the new
    /// transcript. Ready at once when the warm model is current; otherwise it is (re)loaded and
    /// arrives while the session already captures. Fails only when no model is installed.
    pub fn for_session(&self) -> Result<(String, EngineSource), String> {
        let (key, label, threads) = active_key()?;
        let mut slot = self.lock();
        match &mut *slot {
            Slot::Ready {
                key: k,
                label: l,
                engine,
            } if *k == key && !engine.is_poisoned() => {
                return Ok((l.clone(), EngineSource::Ready(Box::new(engine.clone()))));
            }
            Slot::Loading { key: k, waiters } if *k == key => {
                let (tx, rx) = channel();
                waiters.push(tx);
                return Ok((label, EngineSource::Loading(rx)));
            }
            _ => {}
        }
        let (tx, rx) = channel();
        *slot = Slot::Loading {
            key: key.clone(),
            waiters: vec![tx],
        };
        drop(slot);
        self.spawn_load(key, threads);
        Ok((label, EngineSource::Loading(rx)))
    }

    /// Load in the background unless the current model is loaded or loading already. Waits
    /// while a speed check runs (it loads the model itself; the check's caller calls this
    /// again when it is done). Blocks only for the cheap check; the load runs on its own thread.
    pub fn ensure(&self) {
        if crate::speed::is_running() {
            return;
        }
        let Ok((key, _, threads)) = active_key() else {
            return;
        };
        let mut slot = self.lock();
        match &*slot {
            Slot::Ready { key: k, engine, .. } if *k == key && !engine.is_poisoned() => return,
            Slot::Loading { key: k, .. } if *k == key => return,
            _ => {}
        }
        *slot = Slot::Loading {
            key: key.clone(),
            waiters: Vec::new(),
        };
        drop(slot);
        self.spawn_load(key, threads);
    }

    fn spawn_load(&self, key: String, threads: Option<u32>) {
        let slot = self.slot.clone();
        let _ = std::thread::Builder::new()
            .name("sprk-warm".into())
            .spawn(move || {
                let result = models::load_engine(app_version(), threads);
                let mut s = slot.lock().unwrap_or_else(|p| p.into_inner());
                // A newer load (another model or thread count) took over: drop this one.
                let Slot::Loading { key: k, waiters } = &mut *s else {
                    return;
                };
                if *k != key {
                    return;
                }
                let waiters = std::mem::take(waiters);
                match result {
                    Ok(loaded) => {
                        let engine = SharedEngine::new(loaded.engine);
                        for w in waiters {
                            let _ = w.send(Ok(LoadedEngine {
                                label: loaded.label.clone(),
                                engine: Box::new(engine.clone()),
                            }));
                        }
                        *s = Slot::Ready {
                            key,
                            label: loaded.label,
                            engine,
                        };
                    }
                    Err(e) => {
                        for w in waiters {
                            let _ = w.send(Err(e.clone()));
                        }
                        // The next Start (or ensure) tries again.
                        *s = Slot::Empty;
                    }
                }
            });
    }
}

/// Background preparation (app launch, after a model download, after "Check again"): run the
/// speed check first if this model was never checked on this PC, then load the warm model.
pub fn prepare(app: &AppHandle) {
    let a = app.clone();
    let _ = std::thread::Builder::new()
        .name("sprk-prepare".into())
        .spawn(move || {
            if session_running(&a) {
                return;
            }
            crate::speed::check_if_needed(app_version());
            if !session_running(&a) {
                a.state::<Warm>().ensure();
            }
        });
}

/// Make sure the model is loaded or loading (main window shown, "Start a transcript?" shown).
/// Never during a session: a model that changed on disk mid-session is loaded after it, so a
/// second copy never competes with the running one. Returns at once; the cheap check runs on
/// its own thread.
pub fn ensure(app: &AppHandle) {
    let a = app.clone();
    let _ = std::thread::Builder::new()
        .name("sprk-warm-check".into())
        .spawn(move || {
            if !session_running(&a) {
                a.state::<Warm>().ensure();
            }
        });
}

fn session_running(app: &AppHandle) -> bool {
    app.state::<crate::AppState>().busy()
}

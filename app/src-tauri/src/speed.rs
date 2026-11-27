//! Speed check (SPEC 16.8a): pick the decode thread count for this model on this PC.
//!
//! Once per model and machine, the active model is loaded at a couple of thread counts and a
//! short built-in clip (made in code, no voice, nothing read from or written to disk but the
//! result) is decoded with each; the fewest threads about as fast as the fastest win
//! (`sprk_asr::tune`). The choice is kept in `%APPDATA%\SprkNotes\speed.json`, keyed by model and
//! machine, and shown in Settings ("Speed: 3.2x real time on 4 threads"). Any error keeps the
//! old default (`min(4, cores)`). `SPRK_ASR_THREADS` still wins over all of it. The check never
//! runs during a session, and a Start that comes while it runs calls it off.
use serde::{Deserialize, Serialize};
use sprk_asr::tune;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// Length of the built-in clip. Long enough to time, short enough to finish in seconds.
const CLIP_SECONDS: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct Entry {
    threads: u32,
    /// Real-time factor at those threads (decode seconds per audio second).
    rtf: f32,
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static CALL_OFF: AtomicBool = AtomicBool::new(false);

fn cache_path() -> Option<PathBuf> {
    crate::models::user_data_dir().map(|d| d.join("SprkNotes").join("speed.json"))
}

/// The same `%APPDATA%` can follow a person to another PC (roaming profiles), so the PC is
/// part of the key: processor type, logical cores and computer name.
fn machine() -> String {
    let cores = std::thread::available_parallelism().map_or(0, |n| n.get());
    let name = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default();
    format!("{}/{cores}/{name}", std::env::consts::ARCH)
}

fn key(model: &str) -> String {
    format!("{model}|{}", machine())
}

fn read_all() -> BTreeMap<String, Entry> {
    cache_path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn store(model: &str, e: Entry) {
    let Some(p) = cache_path() else {
        return;
    };
    let mut all = read_all();
    all.insert(key(model), e);
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(b) = serde_json::to_vec_pretty(&all) {
        let _ = std::fs::write(p, b);
    }
}

fn cached(model: &str) -> Option<Entry> {
    read_all().get(&key(model)).copied()
}

/// Decode threads for `model` on this PC: the speed check's choice, or None for the default.
/// (`SPRK_ASR_THREADS` is applied by the engine itself and wins over this.)
pub fn threads_for(model: &str) -> Option<u32> {
    cached(model).map(|e| e.threads)
}

/// Whether a speed check is running now.
pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// A session is starting: stop a speed check that is running, so it does not slow the
/// session down. Its result is thrown away; it runs again later.
pub fn call_off() {
    if RUNNING.load(Ordering::SeqCst) {
        CALL_OFF.store(true, Ordering::SeqCst);
    }
}

/// Run the check for the active model now, unless one is running already. Slow (loads the
/// model a few times): never call it on the main thread. Stores and returns the choice.
pub fn run(app_version: &str) -> Result<(), String> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("The speed check is already running.".into());
    }
    CALL_OFF.store(false, Ordering::SeqCst);
    let r = measure(app_version);
    RUNNING.store(false, Ordering::SeqCst);
    if CALL_OFF.swap(false, Ordering::SeqCst) {
        return Err("The speed check stopped because a transcript started.".into());
    }
    r
}

fn measure(app_version: &str) -> Result<(), String> {
    let (model, _) = crate::models::active_choice()?;
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
    let clip = tune::speed_clip(CLIP_SECONDS);
    let picked = tune::speed_check(&tune::speed_candidates(cores), |threads| {
        if CALL_OFF.load(Ordering::SeqCst) {
            return None;
        }
        let mut loaded = crate::models::load_engine(app_version, Some(threads)).ok()?;
        // A fallback model loaded: its speed says nothing about the active one.
        if loaded.label != model {
            return None;
        }
        tune::time_decode(loaded.engine.as_mut(), &clip)
    });
    if CALL_OFF.load(Ordering::SeqCst) {
        return Err("called off".into());
    }
    let t = picked.ok_or("The speed check could not time the model.")?;
    store(
        &model,
        Entry {
            threads: t.threads,
            rtf: t.rtf,
        },
    );
    Ok(())
}

/// Run the check in the background when this model has never been checked on this PC.
/// Returns whether it ran (and stored a new choice).
pub fn check_if_needed(app_version: &str) -> bool {
    if tune::env_threads().is_some() {
        return false;
    }
    let Ok((model, _)) = crate::models::active_choice() else {
        return false;
    };
    if cached(&model).is_some() {
        return false;
    }
    run(app_version).is_ok()
}

/// What Settings shows under the transcription model.
#[derive(Serialize, Clone)]
pub struct SpeedStatus {
    /// `checking`, `done` (speed and threads set), `none` (not checked yet), `env` (threads
    /// set by `SPRK_ASR_THREADS`), or `unavailable` (no model).
    state: &'static str,
    /// Times faster than real time, one decimal.
    speed: Option<f32>,
    threads: Option<u32>,
}

pub fn status() -> SpeedStatus {
    let none = |state| SpeedStatus {
        state,
        speed: None,
        threads: None,
    };
    if RUNNING.load(Ordering::SeqCst) {
        return none("checking");
    }
    if let Some(n) = tune::env_threads() {
        return SpeedStatus {
            state: "env",
            speed: None,
            threads: Some(n),
        };
    }
    let Ok((model, _)) = crate::models::active_choice() else {
        return none("unavailable");
    };
    match cached(&model) {
        Some(e) => {
            let x = tune::ThreadTiming {
                threads: e.threads,
                rtf: e.rtf,
            }
            .times_real_time();
            SpeedStatus {
                state: "done",
                speed: Some((x * 10.0).round() / 10.0),
                threads: Some(e.threads),
            }
        }
        None => none("none"),
    }
}

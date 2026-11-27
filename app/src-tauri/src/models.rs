//! Where the desktop app keeps model stores (SPEC 16.4). Resolving and loading is shared with the
//! phone app in `sprk_core::models`. The model a session uses is fixed at its start and never
//! swapped mid-session; the warm copy (`warm.rs`, SPEC 4.6) is loaded ahead of time.
use sprk_pipeline::host::LoadedEngine;
use sprk_pipeline::session::VadFactory;
use std::path::PathBuf;

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
pub fn roots() -> sprk_core::models::Roots {
    sprk_core::models::Roots {
        bundled: std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|p| p.join("models"))),
        machine: env_dir("ProgramData").map(|p| p.join("SprkNotes").join("models")),
        user: user_data_dir().map(|p| p.join("SprkNotes").join("models")),
    }
}

/// Friendly name of the model the next session will use, for the "Loading model" message.
pub fn active_display_name() -> Option<String> {
    sprk_core::models::active_display_name(&roots())
}

/// The model the next session should use, without loading anything: `id@version` and folder.
pub fn active_choice() -> Result<(String, PathBuf), String> {
    sprk_core::models::active_choice(&roots())
}

/// Load the active transcription model with `threads` decode threads (None: the default).
/// Slow (hashes and loads the model): never call it on the main thread.
pub fn load_engine(app_version: &str, threads: Option<u32>) -> Result<LoadedEngine, String> {
    sprk_core::models::load_engine(&roots(), app_version, threads)
}

/// The voice activity detector for a session.
pub fn load_vad() -> Result<VadFactory, String> {
    sprk_core::models::load_vad(&roots())
}

//! Where the desktop app keeps model stores (SPEC 16.4). Resolving and loading is shared with the
//! phone app in `sprk_core::models`.
use std::path::PathBuf;

pub use sprk_core::models::Loaded;

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
fn roots() -> sprk_core::models::Roots {
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

/// Load the active model and voice activity pack for a session.
pub fn load_active(app_version: &str) -> Result<Loaded, String> {
    sprk_core::models::load_active(&roots(), app_version)
}

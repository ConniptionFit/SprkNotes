//! Phone entry point (ADR-015). The desktop app starts from `main.rs`, which keeps the tray,
//! mini controller, shortcut, updater and Outlook/Teams pieces; none of that exists on a phone.
//!
//! This is the scaffold: it opens the shared UI in one window. Session control, the Library
//! and Settings move here from `main.rs` through `sprk-core` in later steps, so both shells
//! call one API.

#[cfg(mobile)]
#[tauri::mobile_entry_point]
pub fn run() {
    let result = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![platform])
        .run(tauri::generate_context!());
    if let Err(err) = result {
        // No PHI here: a startup failure, never transcript content (CLAUDE.md rule 4).
        eprintln!("SprkNotes could not start: {err}");
    }
}

/// Lets the UI pick the phone layout (ADR-015 Decision 6).
#[cfg(mobile)]
#[tauri::command]
fn platform() -> &'static str {
    "mobile"
}

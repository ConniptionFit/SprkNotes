#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Typed command: the UI shows the version it is running.
#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn main() -> anyhow::Result<()> {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_version])
        .run(tauri::generate_context!())?;
    Ok(())
}

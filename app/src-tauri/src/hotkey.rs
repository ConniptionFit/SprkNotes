//! The background shortcut (SPEC 8.6): off by default; Settings turns it on and picks the keys.
//! A press starts a transcript when none runs, and pauses or resumes the one that does.
use sprk_context::hotkey::{action_for, Combo, GlobalHotkey, HotkeyError, PRESS_GAP_MS};
use std::sync::Mutex;
use std::time::Instant;
use tauri::AppHandle;

static HOTKEY: Mutex<Option<Box<dyn GlobalHotkey>>> = Mutex::new(None);
static LAST_PRESS: Mutex<Option<Instant>> = Mutex::new(None);

#[cfg(windows)]
fn backend() -> Option<Box<dyn GlobalHotkey>> {
    Some(Box::new(sprk_context::win_hotkey::WinHotkey::default()))
}

#[cfg(not(windows))]
fn backend() -> Option<Box<dyn GlobalHotkey>> {
    None
}

/// Turn the shortcut on with these keys, or off. Errors: "invalid", "taken", "failed",
/// "unsupported" (no system shortcut on this platform).
pub fn set(app: &AppHandle, enabled: bool, keys: &str) -> Result<String, String> {
    let combo = if enabled {
        Some(Combo::parse(keys).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let mut slot = HOTKEY.lock().map_err(|_| HotkeyError::Failed.to_string())?;
    if slot.is_none() {
        if combo.is_none() {
            return Ok(String::new());
        }
        *slot = backend();
    }
    let Some(h) = slot.as_mut() else {
        return Err("unsupported".into());
    };
    let a = app.clone();
    h.set(combo.as_ref(), Box::new(move || pressed(&a)))
        .map_err(|e| e.to_string())?;
    Ok(combo.map(|c| c.label()).unwrap_or_default())
}

fn pressed(app: &AppHandle) {
    let now = Instant::now();
    if let Ok(mut last) = LAST_PRESS.lock() {
        if last.is_some_and(|t| now.duration_since(t).as_millis() < u128::from(PRESS_GAP_MS)) {
            return;
        }
        *last = Some(now);
    }
    if let Some(action) = action_for(&crate::shell::status(app)) {
        crate::shell::run_action(app, action);
    }
}

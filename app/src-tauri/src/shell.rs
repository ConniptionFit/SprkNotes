//! Window shell: system tray and the floating mini controller (SPEC 8.3).
//!
//! The mini controller is a second, hidden window that shows only when the main window is
//! minimized and a session is in progress (transcribing or paused). Once shown it stays until
//! the main window comes back, so Pause, End and Start from the mini never make it vanish.
//! Nothing here logs or stores session names, titles or transcript text.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::image::Image;
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewWindow, WindowEvent};

/// Logical size of the mini controller. Keep in step with `app/ui/src/lib/Mini.svelte`.
const MINI_COLLAPSED: (f64, f64) = (130.0, 44.0);
const MINI_EXPANDED: (f64, f64) = (372.0, 44.0);

const ICON_READY: &[u8] = include_bytes!("../icons/tray-ready.png");
const ICON_LIVE: &[u8] = include_bytes!("../icons/tray-transcribing.png");
const ICON_PAUSED: &[u8] = include_bytes!("../icons/tray-paused.png");

/// Session state as the UI sees it: "ready", "transcribing" or "paused".
#[derive(Default)]
pub struct Shell {
    status: Mutex<String>,
    /// Last mini position per monitor name (physical pixels).
    positions: Mutex<HashMap<String, (i32, i32)>>,
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    positions: HashMap<String, (i32, i32)>,
}

fn status_of(app: &AppHandle) -> String {
    match app.state::<Shell>().status.lock() {
        Ok(s) if !s.is_empty() => s.clone(),
        _ => "ready".to_string(),
    }
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    load_positions(app);
    let menu = build_menu(app, "ready")?;
    let mut b = TrayIconBuilder::with_id("main")
        .tooltip("SprkNotes is ready")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| act(app, ev.id().as_ref()))
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = ev
            {
                open_main(tray.app_handle());
            }
        });
    if let Ok(img) = Image::from_bytes(ICON_READY) {
        b = b.icon(img);
    }
    b.build(app)?;
    Ok(())
}

/// Called whenever the session status changes (from the session host's events).
pub fn on_status(app: &AppHandle, status: &str) {
    if let Ok(mut s) = app.state::<Shell>().status.lock() {
        *s = status.to_string();
    }
    update_tray(app, status);
    sync_mini(app);
}

/// Called for main-window events that can mean minimize or restore.
pub fn on_main_event(app: &AppHandle, event: &WindowEvent) {
    if matches!(event, WindowEvent::Resized(_) | WindowEvent::Focused(_)) {
        sync_mini(app);
    }
}

/// Called for mini-window events: remember where the user dragged it.
pub fn on_mini_event(app: &AppHandle, event: &WindowEvent) {
    if let WindowEvent::Moved(pos) = event {
        if let Some(mini) = app.get_webview_window("mini") {
            let key = monitor_key(&mini);
            if let Ok(mut p) = app.state::<Shell>().positions.lock() {
                p.insert(key, (pos.x, pos.y));
            }
        }
    }
}

fn update_tray(app: &AppHandle, status: &str) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    let (bytes, tip) = match status {
        "transcribing" => (ICON_LIVE, "SprkNotes is transcribing"),
        "paused" => (ICON_PAUSED, "Transcription is paused"),
        _ => (ICON_READY, "SprkNotes is ready"),
    };
    if let Ok(img) = Image::from_bytes(bytes) {
        let _ = tray.set_icon(Some(img));
    }
    let _ = tray.set_tooltip(Some(tip));
    if let Ok(menu) = build_menu(app, status) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn build_menu(app: &AppHandle, status: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let header = match status {
        "transcribing" => "SprkNotes is transcribing",
        "paused" => "Transcription is paused",
        _ => "SprkNotes is ready",
    };
    let mut m = MenuBuilder::new(app).item(
        &MenuItemBuilder::with_id("header", header)
            .enabled(false)
            .build(app)?,
    );
    m = m.separator();
    match status {
        "transcribing" => {
            m = m.item(&MenuItemBuilder::with_id("pause", "Pause").build(app)?);
            m = m.item(&MenuItemBuilder::with_id("end", "End").build(app)?);
        }
        "paused" => {
            m = m.item(&MenuItemBuilder::with_id("resume", "Resume").build(app)?);
            m = m.item(&MenuItemBuilder::with_id("end", "End").build(app)?);
        }
        _ => {
            m = m.item(&MenuItemBuilder::with_id("start", "Start").build(app)?);
        }
    }
    m.separator()
        .item(&MenuItemBuilder::with_id("open", "Open SprkNotes").build(app)?)
        .item(&MenuItemBuilder::with_id("quit", "Quit").build(app)?)
        .build()
}

/// Run a tray action off the UI thread (starting a session loads the model).
fn act(app: &AppHandle, id: &str) {
    match id {
        "open" => open_main(app),
        "quit" => {
            let app = app.clone();
            std::thread::spawn(move || {
                // End first so the transcript is finalized; committed lines are already on disk.
                let _ = crate::end_session(app.state());
                app.exit(0);
            });
        }
        "start" | "pause" | "resume" | "end" => {
            let app = app.clone();
            let id = id.to_string();
            std::thread::spawn(move || {
                let r = match id.as_str() {
                    "start" => crate::start_session(app.clone(), app.state(), String::new()),
                    "pause" => crate::pause_session(app.state()),
                    "resume" => crate::resume_session(app.state()),
                    _ => crate::end_session(app.state()),
                };
                if r.is_err() {
                    let _ = app.emit("session-problem", "action_failed");
                }
            });
        }
        _ => {}
    }
}

pub fn open_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    sync_mini(app);
}

/// Show the mini when the main window is minimized and a session is in progress; keep it
/// until the main window is back; hide it then.
fn sync_mini(app: &AppHandle) {
    let (Some(main), Some(mini)) = (app.get_webview_window("main"), app.get_webview_window("mini"))
    else {
        return;
    };
    let away = main.is_minimized().unwrap_or(false) || !main.is_visible().unwrap_or(true);
    let shown = mini.is_visible().unwrap_or(false);
    if away && !shown && status_of(app) != "ready" {
        place_mini(app, &mini);
        let _ = mini.show();
    } else if !away && shown {
        save_positions(app);
        let _ = mini.hide();
    }
}

fn monitor_key(w: &WebviewWindow) -> String {
    w.current_monitor()
        .ok()
        .flatten()
        .and_then(|m| m.name().cloned())
        .unwrap_or_else(|| "primary".to_string())
}

/// Restore the last position on this monitor, or default to top centre of the primary monitor.
fn place_mini(app: &AppHandle, mini: &WebviewWindow) {
    let _ = mini.set_size(LogicalSize::new(MINI_COLLAPSED.0, MINI_COLLAPSED.1));
    let monitors = app.available_monitors().unwrap_or_default();
    let saved = app
        .state::<Shell>()
        .positions
        .lock()
        .ok()
        .and_then(|p| p.values().copied().find(|&(x, y)| inside_any(&monitors, x, y)));
    if let Some((x, y)) = saved {
        let _ = mini.set_position(PhysicalPosition::new(x, y));
        return;
    }
    if let Ok(Some(m)) = mini.primary_monitor() {
        let scale = m.scale_factor();
        let w = (MINI_COLLAPSED.0 * scale) as i32;
        let x = m.position().x + (m.size().width as i32 - w) / 2;
        let y = m.position().y + (24.0 * scale) as i32;
        let _ = mini.set_position(PhysicalPosition::new(x, y));
    }
}

fn inside_any(monitors: &[tauri::Monitor], x: i32, y: i32) -> bool {
    monitors.iter().any(|m| {
        let p = m.position();
        let s = m.size();
        x >= p.x && y >= p.y && x < p.x + s.width as i32 && y < p.y + s.height as i32
    })
}

/// Grow or shrink the mini from its left edge; keep it on its monitor when it grows.
pub fn set_mini_expanded(app: &AppHandle, expanded: bool) {
    let Some(mini) = app.get_webview_window("mini") else {
        return;
    };
    let (w, h) = if expanded { MINI_EXPANDED } else { MINI_COLLAPSED };
    if expanded {
        if let (Ok(pos), Ok(Some(m))) = (mini.outer_position(), mini.current_monitor()) {
            let scale = m.scale_factor();
            let right = m.position().x + m.size().width as i32;
            let max_x = right - (w * scale) as i32;
            if pos.x > max_x {
                let _ = mini.set_position(PhysicalPosition::new(max_x, pos.y));
            }
        }
    }
    let _ = mini.set_size(LogicalSize::new(w, h));
    if !expanded {
        save_positions(app);
    }
}

fn positions_file(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("mini-position.json"))
}

fn load_positions(app: &AppHandle) {
    let Some(f) = positions_file(app) else {
        return;
    };
    if let Ok(text) = std::fs::read_to_string(f) {
        if let Ok(saved) = serde_json::from_str::<Saved>(&text) {
            if let Ok(mut p) = app.state::<Shell>().positions.lock() {
                *p = saved.positions;
            }
        }
    }
}

fn save_positions(app: &AppHandle) {
    let (Some(f), Ok(p)) = (positions_file(app), app.state::<Shell>().positions.lock()) else {
        return;
    };
    let saved = Saved {
        positions: p.clone(),
    };
    if let (Some(dir), Ok(text)) = (f.parent(), serde_json::to_string(&saved)) {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(f, text);
    }
}

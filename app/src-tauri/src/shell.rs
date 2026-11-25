//! Window shell: system tray and the floating mini controller (SPEC 8.3).
//!
//! The mini controller is a second, hidden window that shows only when the main window is
//! minimized and a session is in progress (transcribing or paused). Once shown it stays until
//! the main window comes back, so Pause, End and Start from the mini never make it vanish.
//! Nothing here logs or stores session names, titles or transcript text.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewWindow, WindowEvent,
};

/// Default overlay width in logical pixels, used only to centre it the first time. After that the
/// UI sizes the window to its own content (`resize_mini`).
const MINI_DEFAULT_WIDTH: f64 = 200.0;

/// Logical height of the pill's box inside the overlay window: 6 px padding, 42 px pill, 6 px
/// padding. The pill's centre sits half of this from the box's top (or bottom, when flipped).
const PILL_BOX: f64 = 54.0;
/// Share of the monitor height around the middle where the drawer keeps its current side, so it
/// never flutters when the pill rests near the halfway line.
const FLIP_DEADBAND: f64 = 0.04;
/// How long the pill must sit still before the drawer changes sides.
const FLIP_SETTLE: Duration = Duration::from_millis(220);

const ICON_READY: &[u8] = include_bytes!("../icons/tray-ready.png");
const ICON_LIVE: &[u8] = include_bytes!("../icons/tray-transcribing.png");
const ICON_PAUSED: &[u8] = include_bytes!("../icons/tray-paused.png");
const ICON_APP: &[u8] = include_bytes!("../icons/icon.png");
// Throwback Mode: the first icon set.
const OLD_READY: &[u8] = include_bytes!("../icons/throwback/tray-ready.png");
const OLD_LIVE: &[u8] = include_bytes!("../icons/throwback/tray-transcribing.png");
const OLD_PAUSED: &[u8] = include_bytes!("../icons/throwback/tray-paused.png");
const OLD_APP: &[u8] = include_bytes!("../icons/throwback/icon.png");

/// Session state as the UI sees it: "ready", "transcribing" or "paused".
#[derive(Default)]
pub struct Shell {
    status: Mutex<String>,
    /// Last mini position per monitor name (physical pixels).
    positions: Mutex<HashMap<String, (i32, i32)>>,
    /// Live captions wanted by the overlay: (on, include "Me"). Read by the caption path.
    captions_on: AtomicBool,
    captions_me: AtomicBool,
    /// Throwback Mode: show the first icon set in the tray and on the window.
    throwback: AtomicBool,
    /// False when the desktop has no system tray (some Linux setups).
    no_tray: AtomicBool,
    /// True when the caption drawer sits above the pill (pill in the lower half of the screen).
    flipped: AtomicBool,
    /// When the overlay last moved, and whether a settle watcher is already running.
    last_move: Mutex<Option<Instant>>,
    settling: AtomicBool,
    /// "Start a transcript?" is up in the mini (another app holds the microphone, SPEC 8.3a).
    mic_prompt: AtomicBool,
    /// The mini was shown for that question alone, so answering it hides the mini again.
    mini_for_prompt: AtomicBool,
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
            // Single left click or double click restores the window (Windows; Linux tray hosts
            // built on libayatana-appindicator send no click events, so the menu is the way there).
            if matches!(
                ev,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                open_main(tray.app_handle());
            }
        });
    if let Ok(img) = Image::from_bytes(ICON_READY) {
        b = b.icon(img);
    }
    // Some Linux desktops have no tray host. The app must still start; Close then minimizes.
    if b.build(app).is_err() {
        app.state::<Shell>().no_tray.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// Switch the tray and window icons between the current brand and Throwback Mode.
pub fn set_brand(app: &AppHandle, throwback: bool) {
    app.state::<Shell>()
        .throwback
        .store(throwback, Ordering::Relaxed);
    update_tray(app, &status_of(app));
    if let Some(w) = app.get_webview_window("main") {
        if let Ok(img) = Image::from_bytes(if throwback { OLD_APP } else { ICON_APP }) {
            let _ = w.set_icon(img);
        }
    }
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
    // Closing the window hides it to the tray; the tray menu's Quit really exits. A destroyed
    // window could never be brought back by "Open SprkNotes".
    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.hide();
        }
        sync_mini(app);
        return;
    }
    if matches!(event, WindowEvent::Resized(_) | WindowEvent::Focused(_)) {
        sync_mini(app);
    }
}

/// Called for mini-window events: remember where the user dragged it, and once the pill has
/// stopped moving, decide which side of it the caption drawer belongs on.
pub fn on_mini_event(app: &AppHandle, event: &WindowEvent) {
    if !matches!(event, WindowEvent::Moved(_)) {
        return;
    }
    let shell = app.state::<Shell>();
    if let Ok(mut t) = shell.last_move.lock() {
        *t = Some(Instant::now());
    }
    if shell.settling.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(FLIP_SETTLE / 2);
            let still = app
                .state::<Shell>()
                .last_move
                .lock()
                .ok()
                .and_then(|t| *t)
                .is_none_or(|t| t.elapsed() < FLIP_SETTLE);
            if !still {
                break;
            }
        }
        settle_mini(&app);
        app.state::<Shell>().settling.store(false, Ordering::SeqCst);
    });
}

/// The pill has stopped: remember its spot and flip the drawer to the roomier side if needed.
fn settle_mini(app: &AppHandle) {
    let Some(mini) = app.get_webview_window("mini") else {
        return;
    };
    let (Ok(pos), Ok(size), Ok(Some(m))) = (
        mini.outer_position(),
        mini.outer_size(),
        mini.current_monitor(),
    ) else {
        return;
    };
    let shell = app.state::<Shell>();
    let scale = m.scale_factor();
    let was = shell.flipped.load(Ordering::Relaxed);
    let h = size.height as i32;
    let box_top = pill_box_top(pos.y, h, was, scale);
    let now = wants_flip(
        box_top + (PILL_BOX * scale / 2.0) as i32,
        m.position().y,
        m.size().height as i32,
        was,
    );
    if let Ok(mut p) = shell.positions.lock() {
        p.insert(monitor_key(&mini), (pos.x, box_top));
    }
    if now != was {
        shell.flipped.store(now, Ordering::Relaxed);
        let _ = mini.set_position(PhysicalPosition::new(
            pos.x,
            window_y(box_top, h, now, scale),
        ));
        let _ = mini.emit("mini-flip", now);
    }
    save_positions(app);
}

/// Top edge (physical px) of the pill's box for a window at `y` with outer height `h`.
fn pill_box_top(y: i32, h: i32, flipped: bool, scale: f64) -> i32 {
    if flipped {
        y + h - (PILL_BOX * scale) as i32
    } else {
        y
    }
}

/// Window top (physical px) that puts the pill's box at `box_top`.
fn window_y(box_top: i32, h: i32, flipped: bool, scale: f64) -> i32 {
    if flipped {
        box_top + (PILL_BOX * scale) as i32 - h
    } else {
        box_top
    }
}

/// Which side the drawer belongs on for a pill centred at `center_y`. The drawer goes above the
/// pill in the lower half of the monitor. `current` adds a dead band so it does not flutter.
fn wants_flip(center_y: i32, mon_top: i32, mon_h: i32, current: bool) -> bool {
    let mid = mon_top as f64 + mon_h as f64 / 2.0;
    let band = mon_h as f64 * FLIP_DEADBAND;
    let c = center_y as f64;
    if current {
        c > mid - band
    } else {
        c > mid + band
    }
}

fn update_tray(app: &AppHandle, status: &str) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    let old = app.state::<Shell>().throwback.load(Ordering::Relaxed);
    let (bytes, tip) = match (status, old) {
        ("transcribing", false) => (ICON_LIVE, "SprkNotes is transcribing"),
        ("transcribing", true) => (OLD_LIVE, "SprkNotes is transcribing"),
        ("paused", false) => (ICON_PAUSED, "Transcription is paused"),
        ("paused", true) => (OLD_PAUSED, "Transcription is paused"),
        (_, false) => (ICON_READY, "SprkNotes is ready"),
        (_, true) => (OLD_READY, "SprkNotes is ready"),
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
                let _ = crate::end_session_blocking(&app.state::<crate::AppState>());
                app.exit(0);
            });
        }
        "start" | "pause" | "resume" | "end" => {
            let app = app.clone();
            let id = id.to_string();
            std::thread::spawn(move || {
                let r = match id.as_str() {
                    "start" => tauri::async_runtime::block_on(crate::start_session(
                        app.clone(),
                        String::new(),
                    )),
                    "pause" => crate::pause_session(app.state()),
                    "resume" => crate::resume_session(app.state()),
                    _ => crate::end_session_blocking(&app.state::<crate::AppState>()),
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

/// The main window is minimized or hidden to the tray: SprkNotes is in the background.
pub fn main_away(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .is_some_and(|m| m.is_minimized().unwrap_or(false) || !m.is_visible().unwrap_or(true))
}

pub fn mic_prompt_up(app: &AppHandle) -> bool {
    app.state::<Shell>().mic_prompt.load(Ordering::SeqCst)
}

/// Ask "Start a transcript?" in the mini. Only while in the background and no session runs:
/// with the main window open the Start button is already in view.
pub fn show_mic_prompt(app: &AppHandle) {
    if !main_away(app) || status_of(app) != "ready" {
        return;
    }
    let Some(mini) = app.get_webview_window("mini") else {
        return;
    };
    let shell = app.state::<Shell>();
    shell.mic_prompt.store(true, Ordering::SeqCst);
    if !mini.is_visible().unwrap_or(false) {
        place_mini(app, &mini);
        let _ = mini.show();
        shell.mini_for_prompt.store(true, Ordering::SeqCst);
    }
    let _ = mini.emit("mic-prompt", true);
}

/// Take the question down (answered, the call ended, or the main window came back). A mini
/// that was shown only for it hides again unless a session started.
pub fn hide_mic_prompt(app: &AppHandle) {
    let shell = app.state::<Shell>();
    if !shell.mic_prompt.swap(false, Ordering::SeqCst) {
        return;
    }
    if let Some(mini) = app.get_webview_window("mini") {
        let _ = mini.emit("mic-prompt", false);
        if shell.mini_for_prompt.swap(false, Ordering::SeqCst) && status_of(app) == "ready" {
            save_positions(app);
            let _ = mini.hide();
        }
    }
}

/// The user answered: Start begins a session (the mini stays as its controller), Dismiss just
/// takes the question down.
pub fn answer_mic_prompt(app: &AppHandle, start: bool) {
    if start {
        // A session is starting: the mini stays, now as the session's controller.
        app.state::<Shell>()
            .mini_for_prompt
            .store(false, Ordering::SeqCst);
        hide_mic_prompt(app);
        act(app, "start");
    } else {
        hide_mic_prompt(app);
    }
}

/// Show the mini when the main window is minimized and a session is in progress; keep it
/// until the main window is back; hide it then.
fn sync_mini(app: &AppHandle) {
    let (Some(main), Some(mini)) = (
        app.get_webview_window("main"),
        app.get_webview_window("mini"),
    ) else {
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
    if !away {
        // Back in the app: the question is moot, Start is right there.
        hide_mic_prompt(app);
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
/// The saved spot is the pill's box, so the pill comes back where it was whichever side the
/// drawer is on.
fn place_mini(app: &AppHandle, mini: &WebviewWindow) {
    let monitors = app.available_monitors().unwrap_or_default();
    let shell = app.state::<Shell>();
    let saved = shell.positions.lock().ok().and_then(|p| {
        p.values()
            .copied()
            .find(|&(x, y)| monitor_at(&monitors, x, y).is_some())
    });
    if let Some((x, box_top)) = saved {
        if let Some(m) = monitor_at(&monitors, x, box_top) {
            let scale = m.scale_factor();
            let h = mini.outer_size().map(|s| s.height as i32).unwrap_or(0);
            let flipped = wants_flip(
                box_top + (PILL_BOX * scale / 2.0) as i32,
                m.position().y,
                m.size().height as i32,
                false,
            );
            shell.flipped.store(flipped, Ordering::Relaxed);
            let _ = mini.set_position(PhysicalPosition::new(
                x,
                window_y(box_top, h, flipped, scale),
            ));
            let _ = mini.emit("mini-flip", flipped);
            return;
        }
    }
    shell.flipped.store(false, Ordering::Relaxed);
    let _ = mini.emit("mini-flip", false);
    if let Ok(Some(m)) = mini.primary_monitor() {
        let scale = m.scale_factor();
        let w = (MINI_DEFAULT_WIDTH * scale) as i32;
        let x = m.position().x + (m.size().width as i32 - w) / 2;
        let y = m.position().y + (24.0 * scale) as i32;
        let _ = mini.set_position(PhysicalPosition::new(x, y));
    }
}

fn monitor_at(monitors: &[tauri::Monitor], x: i32, y: i32) -> Option<&tauri::Monitor> {
    monitors.iter().find(|m| {
        let p = m.position();
        let s = m.size();
        x >= p.x && y >= p.y && x < p.x + s.width as i32 && y < p.y + s.height as i32
    })
}

/// Size the overlay to its content. The pill never moves: when the drawer is below it the window
/// grows right and down from its top-left corner, and when the drawer is above it the window grows
/// right and up from its bottom-left corner. It is only nudged when it would leave the screen.
pub fn resize_mini(app: &AppHandle, width: f64, height: f64, flipped: bool) {
    let Some(mini) = app.get_webview_window("mini") else {
        return;
    };
    app.state::<Shell>()
        .flipped
        .store(flipped, Ordering::Relaxed);
    let (mut max_w, mut max_h) = (1400.0, 900.0);
    let mut target = None;
    if let (Ok(pos), Ok(old), Ok(Some(m))) = (
        mini.outer_position(),
        mini.outer_size(),
        mini.current_monitor(),
    ) {
        let s = m.scale_factor();
        max_w = m.size().width as f64 / s;
        max_h = m.size().height as f64 / s;
        let (w, h) = (width.clamp(40.0, max_w), height.clamp(30.0, max_h));
        let (wp, hp) = ((w * s) as i32, (h * s) as i32);
        let right = m.position().x + m.size().width as i32;
        let bottom = m.position().y + m.size().height as i32;
        let want_y = if flipped {
            pos.y + old.height as i32 - hp
        } else {
            pos.y
        };
        let x = pos.x.min(right - wp).max(m.position().x);
        let y = want_y.min(bottom - hp).max(m.position().y);
        if (x, y) != (pos.x, pos.y) {
            target = Some(PhysicalPosition::new(x, y));
        }
    }
    let (w, h) = (width.clamp(40.0, max_w), height.clamp(30.0, max_h));
    let _ = mini.set_size(LogicalSize::new(w, h));
    if let Some(p) = target {
        let _ = mini.set_position(p);
    }
}

/// True while the drawer sits above the pill.
pub fn mini_flipped(app: &AppHandle) -> bool {
    app.state::<Shell>().flipped.load(Ordering::Relaxed)
}

/// The UI says which captions it wants, so the engine can skip the rest.
pub fn set_captions(app: &AppHandle, on: bool, me: bool) {
    let s = app.state::<Shell>();
    s.captions_on.store(on, Ordering::Relaxed);
    s.captions_me.store(me, Ordering::Relaxed);
}

/// (captions on, include Me). For the caption path in the session host.
#[allow(dead_code)]
pub fn captions_wanted(app: &AppHandle) -> (bool, bool) {
    let s = app.state::<Shell>();
    (
        s.captions_on.load(Ordering::Relaxed),
        s.captions_me.load(Ordering::Relaxed),
    )
}

/// True when a system tray icon exists, so Close may hide to it instead of minimizing.
#[allow(dead_code)]
pub fn tray_available(app: &AppHandle) -> bool {
    !app.state::<Shell>().no_tray.load(Ordering::Relaxed)
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
    let shell = app.state::<Shell>();
    let (Some(f), Ok(p)) = (positions_file(app), shell.positions.lock()) else {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drawer_flips_in_the_lower_half_with_a_dead_band() {
        // 1080 px monitor: middle 540, dead band 43 px.
        assert!(!wants_flip(300, 0, 1080, false));
        assert!(wants_flip(900, 0, 1080, false));
        assert!(
            !wants_flip(560, 0, 1080, false),
            "inside the band: stay unflipped"
        );
        assert!(
            wants_flip(520, 0, 1080, true),
            "inside the band: stay flipped"
        );
        assert!(!wants_flip(400, 0, 1080, true));
    }

    #[test]
    fn flipping_keeps_the_pill_where_it_was() {
        let (scale, h) = (1.5, 400);
        let top = 700;
        let flipped_y = window_y(top, h, true, scale);
        assert_eq!(pill_box_top(flipped_y, h, true, scale), top);
        assert_eq!(window_y(top, h, false, scale), top);
        assert_eq!(pill_box_top(top, h, false, scale), top);
    }
}

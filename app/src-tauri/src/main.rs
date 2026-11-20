#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clock;
mod library;
mod model_download;
mod models;
#[cfg(windows)]
mod net;
mod net_unix;
mod shell;

use serde::Serialize;
use sprk_audio::cpal_backend::CpalBackend;
use sprk_audio::live::Backend;
use sprk_audio::Source;
use sprk_pipeline::commit::Config;
use sprk_pipeline::host::{Events, Host, StartParams, UiLine, UiStatus};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Default)]
struct AppState {
    host: Mutex<Option<Host>>,
}

/// Payload shapes the UI expects (app/ui/src/lib/types.ts).
#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum LinePayload {
    Statement {
        id: u64,
        t: String,
        speaker: String,
        text: String,
    },
    Event {
        id: u64,
        t: String,
        text: String,
    },
}

/// Live caption payload (ADR-006). Transient: the UI shows it and forgets it.
#[derive(Serialize, Clone)]
struct CaptionPayload {
    speaker: String,
    text: String,
    /// Leading words that are settled; the rest are tentative and may change.
    stable: usize,
    #[serde(rename = "final")]
    is_final: bool,
    /// Rows that locked since the last caption (ADR-008).
    sealed: Vec<String>,
    utt: u64,
}

struct TauriEvents(AppHandle);

impl Events for TauriEvents {
    fn line(&self, l: UiLine) {
        let p = match l {
            UiLine::Statement {
                id,
                t,
                speaker,
                text,
            } => LinePayload::Statement {
                id,
                t,
                speaker,
                text,
            },
            UiLine::Event { id, t, text } => LinePayload::Event { id, t, text },
        };
        let _ = self.0.emit("transcript-line", p);
    }
    fn status(&self, s: UiStatus) {
        let v = match s {
            UiStatus::Ready => "ready",
            UiStatus::Transcribing => "transcribing",
            UiStatus::Paused => "paused",
        };
        let _ = self.0.emit("session-status", v);
        shell::on_status(&self.0, v);
    }
    fn problem(&self, kind: &str) {
        let _ = self.0.emit("session-problem", kind);
    }
    fn captions_wanted(&self) -> (bool, bool) {
        shell::captions_wanted(&self.0)
    }
    fn caption(&self, c: &sprk_pipeline::host::UiCaption) {
        let _ = self.0.emit(
            "caption",
            CaptionPayload {
                speaker: c.speaker.to_string(),
                text: c.text.clone(),
                stable: c.stable,
                is_final: c.is_final,
                sealed: c.sealed.clone(),
                utt: c.utt,
            },
        );
    }
}

/// Typed command: the UI shows the version it is running.
#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Admin policy (SPEC 10.2): machine then user registry keys. Nothing is set off Windows.
fn policy() -> sprk_config::AppPolicy {
    #[cfg(windows)]
    {
        sprk_config::AppPolicy::load(&sprk_config::registry::RegistryPolicy::layered())
    }
    #[cfg(not(windows))]
    {
        sprk_config::AppPolicy::default()
    }
}

fn transcripts_root(app: &AppHandle, policy: &sprk_config::AppPolicy) -> Result<PathBuf, String> {
    if let Some(root) = &policy.transcripts_root {
        return Ok(PathBuf::from(root));
    }
    let docs = app
        .path()
        .document_dir()
        .map_err(|_| "no Documents folder".to_string())?;
    Ok(docs.join("SprkNotes").join("Transcripts"))
}

/// Audio source and call context detection (SPEC 18, 19). Windows only; the inputs are built on
/// the session thread. Defaults until the policy settings are wired in (M14).
#[cfg(windows)]
fn context_config(p: &sprk_config::AppPolicy) -> Option<sprk_pipeline::host::ContextConfig> {
    use sprk_context::sources::AudioSessionSource;
    use sprk_context::win::{WasapiSessions, Windows};
    use sprk_context::WindowEnumerator;
    use sprk_pipeline::host::{ContextConfig, ContextInputs};
    Some(ContextConfig {
        make_inputs: Box::new(|| {
            let (w, s) = (Windows, WasapiSessions::default());
            ContextInputs {
                windows: Box::new(move || w.windows()),
                sessions: Box::new(move || s.sessions()),
            }
        }),
        audio_sources: p.audio_source_detection.unwrap_or(true),
        call_context: p.call_context.unwrap_or(true),
        name_one_to_one: p.name_one_to_one_speakers.unwrap_or(true),
        timing: {
            let d = sprk_context::activity::Timing::default();
            sprk_context::activity::Timing {
                merge_general_ms: p
                    .audio_merge_window_seconds
                    .map_or(d.merge_general_ms, |s| s * 1000),
                merge_call_ms: p
                    .call_merge_window_seconds
                    .map_or(d.merge_call_ms, |s| s * 1000),
                min_activity_ms: d.min_activity_ms,
            }
        },
        exclusions: p.audio_source_exclude_patterns.clone(),
        call_poll_ms: p.call_poll_seconds.unwrap_or(15).clamp(5, 120) * 1000,
    })
}

#[cfg(not(windows))]
fn context_config(_: &sprk_config::AppPolicy) -> Option<sprk_pipeline::host::ContextConfig> {
    None
}

fn commit_config(p: &sprk_config::AppPolicy) -> Config {
    let mut c = Config::default();
    if let Some(s) = p.commit_delay_seconds {
        c.delay_ms = s.clamp(1, 30) * 1000;
    }
    if let Some(f) = p.filler_removal {
        c.drop_fillers = f;
    }
    c
}

/// Settings pane: is a model present, and may this PC download one? No network.
#[tauri::command]
fn model_status() -> model_download::ModelStatus {
    model_download::status(&policy())
}

/// Ask the model server whether a newer model exists (network, model index only).
#[tauri::command]
async fn model_check_update() -> Result<model_download::UpdateInfo, String> {
    let p = policy();
    tauri::async_runtime::spawn_blocking(move || model_download::check_now(&p))
        .await
        .map_err(|_| "The check was interrupted.".to_string())?
}

/// "Download model": fetch and install the default model. Progress arrives as `model-download`.
#[tauri::command]
fn model_download(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let running = state.host.lock().map(|h| h.is_some()).unwrap_or(true);
    if running {
        return Err("End the current session before downloading a model.".into());
    }
    model_download::start_install(app, policy())
}

#[tauri::command]
fn start_session(app: AppHandle, state: State<AppState>, name: String) -> Result<(), String> {
    let mut slot = state.host.lock().map_err(|_| "busy".to_string())?;
    if slot.is_some() {
        return Err("A session is already running.".into());
    }
    // The model is chosen now and never swapped mid-session.
    let policy = policy();
    let loaded = models::load_active(env!("CARGO_PKG_VERSION"))?;
    let host = Host::start(
        StartParams {
            root: transcripts_root(&app, &policy)?,
            name,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            engine_label: loaded.label,
            commit: commit_config(&policy),
        },
        loaded.engine,
        loaded.make_vad,
        Box::new(|| Box::new(CpalBackend::default()) as Box<dyn Backend>),
        Box::new(clock::LocalClock),
        Arc::new(TauriEvents(app)),
        context_config(&policy),
    )
    .map_err(|e| e.to_string())?;
    *slot = Some(host);
    Ok(())
}

fn with_host(
    state: &State<AppState>,
    f: impl FnOnce(&Host) -> Result<(), sprk_pipeline::host::HostError>,
) -> Result<(), String> {
    let slot = state.host.lock().map_err(|_| "busy".to_string())?;
    match slot.as_ref() {
        Some(h) => f(h).map_err(|e| e.to_string()),
        None => Err("No session is running.".into()),
    }
}

#[tauri::command]
fn pause_session(state: State<AppState>) -> Result<(), String> {
    with_host(&state, |h| h.pause())
}

#[tauri::command]
fn resume_session(state: State<AppState>) -> Result<(), String> {
    with_host(&state, |h| h.resume())
}

#[tauri::command]
fn rename_session(state: State<AppState>, name: String) -> Result<(), String> {
    with_host(&state, |h| h.rename(&name))
}

#[tauri::command]
fn set_source(state: State<AppState>, source: String, on: bool) -> Result<(), String> {
    let s = match source.as_str() {
        "mic" => Source::Mic,
        "system" => Source::System,
        _ => return Err("unknown source".into()),
    };
    with_host(&state, |h| h.set_source(s, on))
}

#[tauri::command]
fn end_session(state: State<AppState>) -> Result<(), String> {
    let host = state.host.lock().map_err(|_| "busy".to_string())?.take();
    match host {
        Some(h) => h.end().map_err(|e| e.to_string()),
        None => Ok(()),
    }
}

/// The overlay sizes its window to its content (logical pixels). `flipped` says the caption
/// drawer is above the pill, so the window grows upward and the pill stays put.
#[tauri::command]
fn mini_resize(app: AppHandle, width: f64, height: f64, flipped: bool) {
    shell::resize_mini(&app, width, height, flipped);
}

/// Whether the caption drawer currently sits above the pill.
#[tauri::command]
fn mini_flipped(app: AppHandle) -> bool {
    shell::mini_flipped(&app)
}

/// Throwback Mode on or off: swaps the tray and window icons.
#[tauri::command]
fn set_brand(app: AppHandle, throwback: bool) {
    shell::set_brand(&app, throwback);
}

/// Which live captions the overlay wants: on or off, and whether to include "Me".
#[tauri::command]
fn set_captions(app: AppHandle, on: bool, me: bool) {
    shell::set_captions(&app, on, me);
}

/// Bring the main window back (also hides the mini controller).
#[tauri::command]
fn open_app(app: AppHandle) {
    shell::open_main(&app);
}

/// Wayland will not let a window place itself or stay on top, which the overlay needs. Run through
/// XWayland unless the user opts out with SPRKNOTES_WAYLAND=1 or sets GDK_BACKEND themselves.
#[cfg(target_os = "linux")]
fn prefer_x11() {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let chosen = std::env::var_os("GDK_BACKEND").is_some()
        || std::env::var_os("SPRKNOTES_WAYLAND").is_some();
    if wayland && !chosen {
        std::env::set_var("GDK_BACKEND", "x11");
    }
}

fn main() -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    prefer_x11();
    // Used by scripts/release.ps1 to check every artifact reports the same version.
    if std::env::args().any(|a| a == "--version") {
        // A Windows-subsystem build may have no stdout; never panic over that.
        use std::io::Write;
        let _ = writeln!(std::io::stdout(), "SprkNotes {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    // Panic messages can carry text from anywhere: report only where it happened (CLAUDE.md rule 4).
    // The session thread closes its transcript as interrupted on its own.
    std::panic::set_hook(Box::new(|info| {
        if let Some(l) = info.location() {
            eprintln!("panic at {}:{}", l.file(), l.line());
        }
    }));
    tauri::Builder::default()
        .manage(AppState::default())
        .manage(shell::Shell::default())
        .setup(|app| {
            shell::init(app.handle())?;
            model_download::launch_check(app.handle().clone(), policy());
            Ok(())
        })
        .on_window_event(|window, event| match window.label() {
            "main" => shell::on_main_event(window.app_handle(), event),
            "mini" => shell::on_mini_event(window.app_handle(), event),
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            app_version,
            start_session,
            pause_session,
            resume_session,
            end_session,
            rename_session,
            set_source,
            mini_resize,
            mini_flipped,
            set_captions,
            set_brand,
            open_app,
            model_status,
            model_check_update,
            model_download,
            library::library_list,
            library::library_open,
            library::library_reveal,
            library::library_export,
            library::latest_transcript,
            library::summarize
        ])
        .run(tauri::generate_context!())?;
    Ok(())
}

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assistant;
mod clock;
mod hotkey;
mod library;
mod mic_watch;
mod model_download;
mod models;
#[cfg(windows)]
mod net;
mod net_unix;
mod shell;
mod updater;

use serde::Serialize;
use sprk_audio::cpal_backend::CpalBackend;
use sprk_audio::live::Backend;
use sprk_audio::live::{Preferred, SharedChoice};
use sprk_audio::Source;
use sprk_pipeline::commit::Config;
use sprk_pipeline::host::{Events, Host, StartParams, UiLine, UiStatus};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Default)]
struct AppState {
    host: Mutex<Option<Host>>,
    /// The microphone and output the user picked in Settings (None: the system default).
    devices: SharedChoice,
    /// Sources the user switched off ([mic, system]). Kept between sessions so a stream turned
    /// off before Start stays off when the session begins (it used to start with both on).
    sources_off: Mutex<[bool; 2]>,
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
    /// Where a finished utterance began, so its row shows a time before it commits.
    #[serde(skip_serializing_if = "Option::is_none")]
    t: Option<String>,
}

/// A Me line shown live was dropped as microphone bleed (transient, never stored).
#[derive(Serialize, Clone)]
struct DroppedPayload {
    speaker: String,
    text: String,
}

struct TauriEvents(AppHandle);

impl Events for TauriEvents {
    fn line(&self, l: UiLine) {
        self.0.state::<assistant::Assistant>().feed(&l);
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
    fn dropped(&self, speaker: &'static str, text: &str) {
        let _ = self.0.emit(
            "transcript-dropped",
            DroppedPayload {
                speaker: speaker.to_string(),
                text: text.to_string(),
            },
        );
    }
    fn levels(&self, me: f32, others: f32) {
        // Only the main window has the source buttons.
        let _ = self.0.emit_to("main", "source-levels", [me, others]);
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
                t: c.t.clone(),
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
    default_transcripts_dir(app.path().document_dir().ok(), app.path().home_dir().ok())
        .ok_or_else(|| "no Documents folder".to_string())
}

/// Default transcripts folder: the Documents folder, or `~/Documents` when none is configured
/// (a fresh home on Linux has no user-dirs entry).
fn default_transcripts_dir(documents: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    let docs = documents.or_else(|| home.map(|h| h.join("Documents")))?;
    Some(docs.join("SprkNotes").join("Transcripts"))
}

#[cfg(test)]
mod default_dir_tests {
    use super::*;

    #[test]
    fn uses_documents_when_set() {
        let got = default_transcripts_dir(Some("/d".into()), Some("/h".into()));
        assert_eq!(got, Some(PathBuf::from("/d/SprkNotes/Transcripts")));
    }

    #[test]
    fn falls_back_to_home_documents() {
        let got = default_transcripts_dir(None, Some("/h".into()));
        assert_eq!(
            got,
            Some(PathBuf::from("/h/Documents/SprkNotes/Transcripts"))
        );
    }

    #[test]
    fn none_when_nothing_known() {
        assert_eq!(default_transcripts_dir(None, None), None);
    }
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

/// Ask Sprk model: which packs are on this PC, and may this PC download one? No network.
#[tauri::command]
fn assistant_model_status() -> model_download::AssistantModelStatus {
    model_download::assistant_status(&policy())
}

/// Ask whether a newer Ask Sprk model exists for `tier` (network, model index only).
#[tauri::command]
async fn assistant_model_check(tier: String) -> Result<model_download::UpdateInfo, String> {
    let p = policy();
    tauri::async_runtime::spawn_blocking(move || model_download::assistant_check_now(&p, &tier))
        .await
        .map_err(|_| "The check was interrupted.".to_string())?
}

/// "Download Ask Sprk model" for `tier`. Progress arrives as `assistant-model-download`.
#[tauri::command]
fn assistant_model_download(
    app: AppHandle,
    state: State<AppState>,
    tier: String,
) -> Result<(), String> {
    let running = state.host.lock().map(|h| h.is_some()).unwrap_or(true);
    if running {
        return Err("End the current session before downloading a model.".into());
    }
    model_download::assistant_start_install(app, policy(), tier)
}

/// Settings pane: update mode, channel and source, and which of them policy locks. No network.
#[tauri::command]
fn update_status() -> updater::UpdateStatus {
    updater::status(&policy())
}

/// Ask the release source whether a newer build exists on the chosen channel.
#[tauri::command]
async fn update_check() -> Result<updater::UpdateInfo, String> {
    let p = policy();
    tauri::async_runtime::spawn_blocking(move || updater::check_now(&p))
        .await
        .map_err(|_| "The check was interrupted.".to_string())?
}

/// Save the user's channel, source or mode. Anything policy locks is refused.
#[tauri::command]
fn update_set(
    channel: Option<String>,
    repo_url: Option<String>,
    mode: Option<String>,
) -> Result<(), String> {
    updater::set_choice(&policy(), channel, repo_url, mode)
}

/// "Update now": download and verify, then install when the app closes. Never mid-session.
#[tauri::command]
async fn update_install() -> Result<Option<String>, String> {
    let p = policy();
    let v = tauri::async_runtime::spawn_blocking(move || updater::fetch_update(&p))
        .await
        .map_err(|_| "The update was interrupted.".to_string())??;
    if v.is_some() {
        updater::request_install();
    }
    Ok(v)
}

/// "Update now": refuse mid-session, download and verify with progress in Settings, then close
/// SprkNotes and hand over to the installer, which shows its own progress and starts the app
/// again when it is done. Returns None when this is already the newest version.
#[tauri::command]
async fn update_now(app: AppHandle, state: State<'_, AppState>) -> Result<Option<String>, String> {
    let p = policy();
    updater::can_update_now(&p)?;
    if state.host.lock().map(|h| h.is_some()).unwrap_or(true) {
        return Err("End the transcript first, then update.".into());
    }
    let a = app.clone();
    let v = tauri::async_runtime::spawn_blocking(move || {
        let mut last: Option<u8> = None;
        updater::fetch_update_with(&p, &mut |done, total| {
            let pct = sprk_update::percent(done, total);
            if pct != last {
                last = pct;
                let _ = a.emit(
                    "app-update-progress",
                    updater::UpdateProgress {
                        stage: "download",
                        percent: pct,
                    },
                );
            }
        })
    })
    .await
    .map_err(|_| "The update was interrupted.".to_string())??;
    let Some(v) = v else {
        return Ok(None);
    };
    // A session may have started while the download ran.
    if state.host.lock().map(|h| h.is_some()).unwrap_or(true) {
        updater::request_install();
        return Err(
            "A transcript started, so the update installs when you close SprkNotes.".into(),
        );
    }
    let _ = app.emit(
        "app-update-progress",
        updater::UpdateProgress {
            stage: "install",
            percent: None,
        },
    );
    if !updater::install_and_relaunch() {
        updater::request_install();
        return Err(
            "The installer did not start. The update installs when you close SprkNotes.".into(),
        );
    }
    // Leave a moment for the window to show "Installing", then close for the installer.
    let a = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(600));
        a.exit(0);
    });
    Ok(Some(v))
}

/// What the window is busy doing while it works: `loading` (with the model's name) at Start,
/// `ending` while the transcript is closed off, `idle` otherwise.
#[derive(Serialize, Clone)]
struct Phase {
    phase: &'static str,
    model: Option<String>,
}

fn emit_phase(app: &AppHandle, phase: &'static str, model: Option<String>) {
    let _ = app.emit("session-phase", Phase { phase, model });
}

/// Async so Tauri runs it off the main thread: loading the model takes a moment and must not
/// freeze the window. The phase event lets the UI say what is happening.
#[tauri::command]
async fn start_session(app: AppHandle, name: String) -> Result<(), String> {
    emit_phase(&app, "loading", models::active_display_name());
    let a = app.clone();
    let r = tauri::async_runtime::spawn_blocking(move || start_session_blocking(a, name))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
    emit_phase(&app, "idle", None);
    r
}

fn start_session_blocking(app: AppHandle, name: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut slot = state.host.lock().map_err(|_| "busy".to_string())?;
    if slot.is_some() {
        return Err("A session is already running.".into());
    }
    app.state::<assistant::Assistant>().clear();
    let devices = state.devices.clone();
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
        Box::new(move || {
            Box::new(Preferred::new(CpalBackend::default(), devices)) as Box<dyn Backend>
        }),
        Box::new(clock::LocalClock),
        Arc::new(TauriEvents(app.clone())),
        context_config(&policy),
    )
    .map_err(|e| e.to_string())?;
    let off = state.sources_off.lock().map(|o| *o).unwrap_or_default();
    for (source, off) in [(Source::Mic, off[0]), (Source::System, off[1])] {
        if off {
            let _ = host.set_source(source, false);
        }
    }
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
    let s = source_of(&source)?;
    if let Ok(mut off) = state.sources_off.lock() {
        off[usize::from(s == Source::System)] = !on;
    }
    let running = state.host.lock().map(|h| h.is_some()).unwrap_or(false);
    if !running {
        // Applied when the next session starts.
        return Ok(());
    }
    with_host(&state, |h| h.set_source(s, on))
}

fn source_of(source: &str) -> Result<Source, String> {
    match source {
        "mic" => Ok(Source::Mic),
        "system" => Ok(Source::System),
        _ => Err("unknown source".into()),
    }
}

/// Devices a source can be switched to (Settings): microphones, or speakers that are looped back.
#[tauri::command]
fn list_devices(source: String) -> Result<Vec<String>, String> {
    use sprk_audio::live::Backend as _;
    Ok(CpalBackend::default().devices(source_of(&source)?))
}

/// Pick the device for a source, or None for the system default. Takes effect within a second,
/// also mid-session (a "Source changed" line is written then).
#[tauri::command]
fn set_device(state: State<AppState>, source: String, name: Option<String>) -> Result<(), String> {
    let name = name.filter(|n| !n.is_empty());
    let mut d = state.devices.lock().map_err(|_| "busy".to_string())?;
    match source_of(&source)? {
        Source::Mic => d.mic = name,
        Source::System => d.system = name,
    }
    Ok(())
}

/// End the session and wait for the file to be finalized. Blocks: never call it on the main
/// thread. The session thread's last status update is marshalled to the main thread (tray, mini),
/// so a main-thread join deadlocks (the Linux "Not Responding" hang).
fn end_session_blocking(state: &AppState) -> Result<(), String> {
    let host = state.host.lock().map_err(|_| "busy".to_string())?.take();
    match host {
        Some(h) => h.end().map_err(|e| e.to_string()),
        None => Ok(()),
    }
}

/// Async so Tauri runs it off the main thread; the join happens on a blocking-pool thread.
#[tauri::command]
async fn end_session(app: AppHandle) -> Result<(), String> {
    emit_phase(&app, "ending", None);
    let a = app.clone();
    let r =
        tauri::async_runtime::spawn_blocking(move || end_session_blocking(&a.state::<AppState>()))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
    emit_phase(&app, "idle", None);
    r
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

/// Whether "Start a transcript?" is up in the mini, and which apps it is about (SPEC 8.3a).
#[tauri::command]
fn mic_prompt_pending(app: AppHandle) -> shell::MicAsk {
    shell::mic_prompt_state(&app)
}

/// The background shortcut (SPEC 8.6): on or off, and its keys ("Ctrl+Alt+S"). Returns the keys
/// as registered; errors with "invalid", "taken", "failed" or "unsupported".
#[tauri::command]
fn set_hotkey(app: AppHandle, enabled: bool, keys: String) -> Result<String, String> {
    hotkey::set(&app, enabled, &keys)
}

/// Settings for "Start a transcript?": on or off, and the apps not to ask about.
#[tauri::command]
fn set_mic_prompt(app: AppHandle, enabled: bool, skipped: Vec<String>) {
    shell::set_mic_prompt(&app, enabled, skipped);
}

/// The answer to "Start a transcript?": Start begins a session, Dismiss takes it down.
#[tauri::command]
fn mic_prompt_answer(app: AppHandle, start: bool) {
    shell::answer_mic_prompt(&app, start);
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
    // The SYSTEM scheduled task of a per-machine install runs this: no window, exits when done.
    if std::env::args().any(|a| a == "--update") {
        updater::run_headless(&policy());
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
        // Must be the first plugin. A second launch (shortcut, Start menu, .desktop entry) exits
        // here and the running copy restores its window, the same path as the tray click.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            shell::open_main(app);
        }))
        .manage(AppState::default())
        .manage(assistant::Assistant::new())
        .manage(shell::Shell::default())
        .setup(|app| {
            shell::init(app.handle())?;
            model_download::launch_check(app.handle().clone(), policy());
            updater::launch_check(app.handle().clone(), policy());
            app.state::<assistant::Assistant>().start_idle_timer();
            mic_watch::start(app.handle().clone());
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
            list_devices,
            set_device,
            mini_resize,
            mini_flipped,
            set_captions,
            mic_prompt_pending,
            mic_prompt_answer,
            set_mic_prompt,
            set_hotkey,
            set_brand,
            open_app,
            model_status,
            model_check_update,
            model_download,
            assistant_model_status,
            assistant_model_check,
            assistant_model_download,
            update_status,
            update_check,
            update_set,
            update_install,
            update_now,
            assistant::assistant_status,
            assistant::assistant_set,
            assistant::assistant_cancel,
            assistant::assistant_run,
            library::library_list,
            library::library_open,
            library::library_reveal,
            library::recovery_scan,
            library::recovery_mark,
            library::library_export,
            library::latest_transcript,
            library::summarize,
            library::library_folders,
            library::library_rename,
            library::library_move,
            library::library_delete,
            library::folder_create,
            library::folder_rename
        ])
        .build(tauri::generate_context!())?
        .run(|_app, event| {
            // Exit is final: hand a verified installer to msiexec so the next launch is the new build.
            if let tauri::RunEvent::Exit = event {
                updater::on_exit(&policy());
            }
        });
    Ok(())
}

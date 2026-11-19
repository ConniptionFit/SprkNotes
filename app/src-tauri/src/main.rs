#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clock;
mod models;

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
    }
    fn problem(&self, kind: &str) {
        let _ = self.0.emit("session-problem", kind);
    }
}

/// Typed command: the UI shows the version it is running.
#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn transcripts_root(app: &AppHandle) -> Result<PathBuf, String> {
    let docs = app
        .path()
        .document_dir()
        .map_err(|_| "no Documents folder".to_string())?;
    Ok(docs.join("SprkNotes").join("Transcripts"))
}

#[tauri::command]
fn start_session(app: AppHandle, state: State<AppState>, name: String) -> Result<(), String> {
    let mut slot = state.host.lock().map_err(|_| "busy".to_string())?;
    if slot.is_some() {
        return Err("A session is already running.".into());
    }
    // The model is chosen now and never swapped mid-session.
    let loaded = models::load_active(env!("CARGO_PKG_VERSION"))?;
    let host = Host::start(
        StartParams {
            root: transcripts_root(&app)?,
            name,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            engine_label: loaded.label,
            commit: Config::default(),
        },
        loaded.engine,
        loaded.make_vad,
        Box::new(|| Box::new(CpalBackend::default()) as Box<dyn Backend>),
        Box::new(clock::LocalClock),
        Arc::new(TauriEvents(app)),
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

fn main() -> anyhow::Result<()> {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            app_version,
            start_session,
            pause_session,
            resume_session,
            end_session,
            rename_session,
            set_source
        ])
        .run(tauri::generate_context!())?;
    Ok(())
}

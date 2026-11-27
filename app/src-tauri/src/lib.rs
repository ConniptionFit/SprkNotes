//! Phone entry point (ADR-015). The desktop app starts from `main.rs`, which keeps the tray,
//! mini controller, shortcut, updater and Outlook/Teams pieces; none of that exists on a phone.
//!
//! Session control runs on the same `sprk-core` API as the desktop, so a start, pause or end
//! behaves the same on both. The phone has one source, the microphone (ADR-015 Decision 4):
//! Android gives apps no way to capture other apps' audio, so "Others" is switched off for good.

#[cfg(mobile)]
mod phone {
    use sprk_audio::cpal_backend::CpalBackend;
    use sprk_audio::live::{Backend, Preferred};
    use sprk_audio::Source;
    use sprk_core::download::{self, ModelStatus, Progress};
    use sprk_core::models::{self, Roots};
    use sprk_core::payload::{
        event, status_name, CaptionPayload, DroppedPayload, LinePayload, Phase,
    };
    use sprk_core::session::{Launch, Sessions};
    use sprk_core::settings::{commit_config, source_of};
    use sprk_pipeline::host::{EngineSource, Events, StartParams, UiCaption, UiLine, UiStatus};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tauri::{AppHandle, Emitter, Manager, State};

    const VERSION: &str = env!("CARGO_PKG_VERSION");

    /// The model index, compiled in from `distribution.toml` (the same one the desktop uses).
    const DEFAULT_INDEX_URL: &str = env!("SPRK_DEFAULT_MODELS_INDEX_URL");

    /// One model download at a time.
    static DOWNLOADING: AtomicBool = AtomicBool::new(false);

    /// Admin settings on phones will come from Android managed configuration, with the desktop
    /// key names (docs/android.md). Until then nothing is locked.
    fn policy() -> sprk_config::AppPolicy {
        sprk_config::AppPolicy::default()
    }

    /// The app's private storage. Removed when the app is uninstalled.
    fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
        app.path()
            .app_data_dir()
            .map_err(|_| "The app's storage is not available.".to_string())
    }

    /// One model store on a phone: models are downloaded, or pushed by the MDM, into private storage.
    fn roots(app: &AppHandle) -> Roots {
        Roots {
            bundled: None,
            machine: None,
            user: data_dir(app).ok().map(|d| d.join("models")),
        }
    }

    struct PhoneEvents(AppHandle);

    impl Events for PhoneEvents {
        fn line(&self, l: UiLine) {
            let _ = self.0.emit(event::LINE, LinePayload::from(l));
        }
        fn status(&self, s: UiStatus) {
            let _ = self.0.emit(event::STATUS, status_name(s));
        }
        fn problem(&self, kind: &str) {
            let _ = self.0.emit(event::PROBLEM, kind);
        }
        /// The conversation view shows the words as they are spoken, and the phone's only voice
        /// is the microphone ("Me"), so it always wants Me's live text.
        fn captions_wanted(&self) -> (bool, bool) {
            (true, true)
        }
        fn dropped(&self, speaker: &'static str, text: &str) {
            let _ = self.0.emit(
                event::DROPPED,
                DroppedPayload {
                    speaker: speaker.to_string(),
                    text: text.to_string(),
                },
            );
        }
        fn levels(&self, me: f32, others: f32) {
            let _ = self.0.emit(event::LEVELS, [me, others]);
        }
        fn caption(&self, c: &UiCaption) {
            let _ = self.0.emit(event::CAPTION, CaptionPayload::from(c));
        }
    }

    fn emit_phase(app: &AppHandle, phase: &'static str, model: Option<String>) {
        let _ = app.emit(event::PHASE, Phase { phase, model });
    }

    pub fn run() {
        let state = Sessions::default();
        // The phone never captures "Others" (see the module docs).
        let _ = state.set_source(Source::System, false);
        let result = tauri::Builder::default()
            .manage(state)
            .invoke_handler(tauri::generate_handler![
                platform,
                app_version,
                start_session,
                pause_session,
                resume_session,
                rename_session,
                set_source,
                list_devices,
                set_device,
                end_session,
                model_status,
                model_download,
            ])
            .run(tauri::generate_context!());
        if let Err(err) = result {
            // No PHI here: a startup failure, never transcript content (CLAUDE.md rule 4).
            eprintln!("SprkNotes could not start: {err}");
        }
    }

    /// Lets the UI pick the phone layout (ADR-015 Decision 6).
    #[tauri::command]
    fn platform() -> &'static str {
        "mobile"
    }

    #[tauri::command]
    fn app_version() -> &'static str {
        VERSION
    }

    /// Async so loading the model never freezes the screen; the phase event says what is happening.
    #[tauri::command]
    async fn start_session(app: AppHandle, name: String) -> Result<(), String> {
        emit_phase(&app, "loading", models::active_display_name(&roots(&app)));
        let a = app.clone();
        let r = tauri::async_runtime::spawn_blocking(move || start_blocking(a, name))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
        emit_phase(&app, "idle", None);
        r
    }

    fn start_blocking(app: AppHandle, name: String) -> Result<(), String> {
        let events = Arc::new(PhoneEvents(app.clone()));
        let roots = roots(&app);
        let root = data_dir(&app)?.join("Transcripts");
        app.state::<Sessions>().start(|devices| {
            let loaded = models::load_active(&roots, VERSION)?;
            Ok(Launch {
                params: StartParams {
                    root,
                    name,
                    app_version: VERSION.to_string(),
                    engine_label: loaded.label,
                    commit: commit_config(&policy()),
                    continue_from: None,
                },
                engine: EngineSource::Ready(loaded.engine),
                make_vad: loaded.make_vad,
                make_backend: Box::new(move || {
                    Box::new(Preferred::new(CpalBackend::default(), devices)) as Box<dyn Backend>
                }),
                events,
                context: None,
            })
        })
    }

    #[tauri::command]
    fn pause_session(state: State<Sessions>) -> Result<(), String> {
        state.pause()
    }

    #[tauri::command]
    fn resume_session(state: State<Sessions>) -> Result<(), String> {
        state.resume()
    }

    #[tauri::command]
    fn rename_session(state: State<Sessions>, name: String) -> Result<(), String> {
        state.rename(&name)
    }

    /// The microphone can be muted mid-session; "Others" stays off on a phone.
    #[tauri::command]
    fn set_source(state: State<Sessions>, source: String, on: bool) -> Result<(), String> {
        match source_of(&source)? {
            Source::Mic => state.set_source(Source::Mic, on),
            Source::System => Ok(()),
        }
    }

    /// Microphones the phone offers (built-in, wired or Bluetooth headsets). None for "Others".
    #[tauri::command]
    fn list_devices(source: String) -> Result<Vec<String>, String> {
        Ok(match source_of(&source)? {
            Source::Mic => CpalBackend::default().devices(Source::Mic),
            Source::System => Vec::new(),
        })
    }

    /// Pick the microphone, or None for the phone's default. Takes effect within a second.
    #[tauri::command]
    fn set_device(
        state: State<Sessions>,
        source: String,
        name: Option<String>,
    ) -> Result<(), String> {
        state.set_device(source_of(&source)?, name)
    }

    /// Whether the speech model and the voice activity pack are on the phone, and whether the
    /// admin allows downloading them.
    #[tauri::command]
    fn model_status(app: AppHandle) -> ModelStatus {
        download::status(&roots(&app), &policy(), DOWNLOADING.load(Ordering::SeqCst))
    }

    /// Download the default packs (the speech model and the voice activity pack, about 480 MB)
    /// into the app's storage, verified exactly as on the desktop (ADR-005). Returns at once;
    /// progress and the result arrive as `model-download` events.
    #[tauri::command]
    fn model_download(app: AppHandle) -> Result<(), String> {
        if DOWNLOADING.swap(true, Ordering::SeqCst) {
            return Err("A download is already running.".into());
        }
        std::thread::spawn(move || {
            let result = download::install(
                &sprk_core::net::Https,
                &roots(&app),
                &policy(),
                DEFAULT_INDEX_URL,
                VERSION,
                &mut |step| {
                    let _ = app.emit(event::MODEL_DOWNLOAD, step);
                },
            );
            DOWNLOADING.store(false, Ordering::SeqCst);
            let _ = app.emit(event::MODEL_DOWNLOAD, Progress::finished(&result));
        });
        Ok(())
    }

    /// Off the main thread: ending waits for the transcript file to be finalized.
    #[tauri::command]
    async fn end_session(app: AppHandle) -> Result<(), String> {
        emit_phase(&app, "ending", None);
        let a = app.clone();
        let r = tauri::async_runtime::spawn_blocking(move || a.state::<Sessions>().end())
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
        emit_phase(&app, "idle", None);
        r
    }
}

#[cfg(mobile)]
#[tauri::mobile_entry_point]
pub fn run() {
    phone::run();
}

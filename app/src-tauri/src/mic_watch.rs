//! Offer a transcript when another app starts using the microphone (SPEC 8.3a). While
//! SprkNotes runs in the background with no session, it looks every two seconds at whether some
//! other app holds a microphone (Windows microphone privacy data, read-only). After a few seconds of that,
//! the mini asks "<App> is using your microphone. Start a transcript?" with Start, Dismiss and
//! "Don't ask for <App>". The question can be turned off in Settings. Nothing is logged; the app
//! names stay in memory, and only the skip list the user builds is kept (in the app settings).
use sprk_context::mic_use::{MicPrompt, MicUse, MicWatch};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const POLL: Duration = Duration::from_secs(2);

/// Start the watcher. A platform without that data (Linux dev builds) never asks.
pub fn start(app: AppHandle) {
    let Some(source) = source() else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("mic-watch".into())
        .spawn(move || run(app, source));
}

#[cfg(windows)]
fn source() -> Option<Box<dyn MicUse>> {
    Some(Box::new(sprk_context::win_mic::RegistryMicUse::default()))
}

#[cfg(not(windows))]
fn source() -> Option<Box<dyn MicUse>> {
    None
}

fn run(app: AppHandle, source: Box<dyn MicUse>) {
    let began = Instant::now();
    let mut watch = MicWatch::new();
    loop {
        std::thread::sleep(POLL);
        let busy = app
            .state::<crate::AppState>()
            .host
            .lock()
            .map(|h| h.is_some())
            .unwrap_or(true);
        // Reading the data is cheap, but skip it entirely while a session runs.
        let apps = if busy {
            Vec::new()
        } else {
            crate::shell::mic_prompt_wanted(&app, source.apps_in_use())
        };
        let in_use = busy || !apps.is_empty();
        let now = began.elapsed().as_millis() as u64;
        match watch.poll(now, in_use, busy) {
            MicPrompt::Ask => {
                let a = app.clone();
                let _ = app.run_on_main_thread(move || crate::shell::show_mic_prompt(&a, apps));
            }
            MicPrompt::Withdraw => {
                let a = app.clone();
                let _ = app.run_on_main_thread(move || crate::shell::hide_mic_prompt(&a));
            }
            MicPrompt::Nothing => {}
        }
        if !crate::shell::mic_prompt_up(&app) {
            // Answered, or never shown because the app was in front: one question per hold.
            watch.answered();
        }
    }
}

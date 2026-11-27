//! One running session at a time, and the choices that carry over between sessions: the devices
//! picked in Settings and the sources switched off before Start. The shell supplies the platform
//! pieces (model, capture backend, events, call context) when a session starts.

use sprk_audio::live::{Backend, SharedChoice};
use sprk_audio::Source;
use sprk_pipeline::host::{ContextConfig, EngineSource, Events, Host, HostError, StartParams};
use sprk_pipeline::session::VadFactory;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Everything a shell hands over to start a session.
pub struct Launch {
    pub params: StartParams,
    /// The model for this session, loaded or still loading (Instant Start, SPEC 4.6). Fixed at
    /// Start and never swapped mid-session.
    pub engine: EngineSource,
    pub make_vad: VadFactory,
    /// Runs on the capture thread (platform streams stay on the thread that opened them).
    pub make_backend: Box<dyn FnOnce() -> Box<dyn Backend> + Send>,
    pub events: Arc<dyn Events>,
    /// Call context (Windows only today); `None` elsewhere.
    pub context: Option<ContextConfig>,
}

/// The app's session state. One per app; shells keep it in their managed state.
#[derive(Default)]
pub struct Sessions {
    host: Mutex<Option<Host>>,
    /// The microphone and output picked in Settings (None: the system default).
    devices: SharedChoice,
    /// Sources switched off ([mic, system]). Kept between sessions so a stream turned off before
    /// Start stays off when the session begins.
    sources_off: Mutex<[bool; 2]>,
    /// The transcript open in the Library, which retention (SPEC 6.6) leaves alone.
    viewing: Mutex<Option<PathBuf>>,
}

const BUSY: &str = "busy";

fn host_err(e: HostError) -> String {
    e.to_string()
}

impl Sessions {
    /// The device choice, for a shell that wraps its backend in `Preferred`.
    pub fn devices(&self) -> SharedChoice {
        self.devices.clone()
    }

    pub fn is_running(&self) -> bool {
        self.host.lock().map(|h| h.is_some()).unwrap_or(false)
    }

    /// Like [`Self::is_running`], but cautious: unreadable state counts as running. For steps that
    /// must never happen mid-session (installing an update or a model, moving files).
    pub fn busy(&self) -> bool {
        self.host.lock().map(|h| h.is_some()).unwrap_or(true)
    }

    /// The file the running session writes to, if any.
    pub fn live_path(&self) -> Option<PathBuf> {
        self.host.lock().ok()?.as_ref()?.path()
    }

    /// Note which transcript the Library has open (None: none).
    pub fn set_viewing(&self, path: Option<PathBuf>) {
        if let Ok(mut v) = self.viewing.lock() {
            *v = path;
        }
    }

    /// The transcript the Library has open, if any.
    pub fn viewing(&self) -> Option<PathBuf> {
        self.viewing.lock().ok().and_then(|v| v.clone())
    }

    /// Run `f` while holding the session slot, so no session can start on, or continue, a file
    /// while `f` works on the transcripts. `f` gets the live file, if a session is running.
    pub fn hold<R>(&self, f: impl FnOnce(Option<PathBuf>) -> R) -> R {
        let slot = self.host.lock().unwrap_or_else(|p| p.into_inner());
        let live = slot.as_ref().and_then(Host::path);
        let r = f(live);
        drop(slot);
        r
    }

    /// Start a session. `prepare` runs only when none is running, while the slot is held, so two
    /// quick presses of Start never load the model twice. It receives the device choice.
    pub fn start(
        &self,
        prepare: impl FnOnce(SharedChoice) -> Result<Launch, String>,
    ) -> Result<(), String> {
        let mut slot = self.host.lock().map_err(|_| BUSY.to_string())?;
        // A session that stopped by itself (the model never loaded, or a panic) is cleared here.
        if slot.as_ref().is_some_and(Host::is_finished) {
            if let Some(h) = slot.take() {
                let _ = h.end();
            }
        }
        if slot.is_some() {
            return Err("A session is already running.".into());
        }
        let l = prepare(self.devices.clone())?;
        let host = Host::start(
            l.params,
            l.engine,
            l.make_vad,
            l.make_backend,
            Box::new(crate::clock::LocalClock),
            l.events,
            l.context,
        )
        .map_err(host_err)?;
        let off = self.sources_off.lock().map(|o| *o).unwrap_or_default();
        for (source, off) in [(Source::Mic, off[0]), (Source::System, off[1])] {
            if off {
                let _ = host.set_source(source, false);
            }
        }
        *slot = Some(host);
        Ok(())
    }

    fn with_host(&self, f: impl FnOnce(&Host) -> Result<(), HostError>) -> Result<(), String> {
        let slot = self.host.lock().map_err(|_| BUSY.to_string())?;
        match slot.as_ref() {
            Some(h) => f(h).map_err(host_err),
            None => Err("No session is running.".into()),
        }
    }

    pub fn pause(&self) -> Result<(), String> {
        self.with_host(|h| h.pause())
    }

    pub fn resume(&self) -> Result<(), String> {
        self.with_host(|h| h.resume())
    }

    pub fn rename(&self, name: &str) -> Result<(), String> {
        self.with_host(|h| h.rename(name))
    }

    /// Drop a mark into the live transcript at this moment.
    pub fn mark(&self) -> Result<(), String> {
        self.with_host(|h| h.mark())
    }

    /// Save a short typed note into the live transcript at this moment.
    pub fn note(&self, text: &str) -> Result<(), String> {
        self.with_host(|h| h.note(text).map(|_| ()))
    }

    /// Turn a source on or off. Remembered for the next session; applied now if one is running.
    pub fn set_source(&self, s: Source, on: bool) -> Result<(), String> {
        if let Ok(mut off) = self.sources_off.lock() {
            off[usize::from(s == Source::System)] = !on;
        }
        if !self.is_running() {
            return Ok(());
        }
        self.with_host(|h| h.set_source(s, on))
    }

    /// Pick the device for a source, or None for the system default. Takes effect within a
    /// second, also mid-session.
    pub fn set_device(&self, s: Source, name: Option<String>) -> Result<(), String> {
        let name = name.filter(|n| !n.is_empty());
        let mut d = self.devices.lock().map_err(|_| BUSY.to_string())?;
        match s {
            Source::Mic => d.mic = name,
            Source::System => d.system = name,
        }
        Ok(())
    }

    /// End the session and wait for the file to be finalized. Blocks: never call it on a UI
    /// thread (the session's last status update may need that thread).
    pub fn end(&self) -> Result<(), String> {
        let host = self.host.lock().map_err(|_| BUSY.to_string())?.take();
        match host {
            Some(h) => h.end().map_err(host_err),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_without_a_session() {
        let s = Sessions::default();
        assert!(!s.is_running());
        assert_eq!(s.pause(), Err("No session is running.".to_string()));
        assert_eq!(s.end(), Ok(()));
        // Switching a source off before Start is remembered, not an error.
        assert_eq!(s.set_source(Source::System, false), Ok(()));
        assert_eq!(s.sources_off.lock().map(|o| *o).ok(), Some([false, true]));
        assert_eq!(s.mark(), Err("No session is running.".to_string()));
        assert_eq!(s.note("x"), Err("No session is running.".to_string()));
    }

    #[test]
    fn viewing_is_remembered_and_cleared() {
        let s = Sessions::default();
        assert_eq!(s.viewing(), None);
        s.set_viewing(Some(PathBuf::from("a.md")));
        assert_eq!(s.viewing(), Some(PathBuf::from("a.md")));
        s.set_viewing(None);
        assert_eq!(s.viewing(), None);
        assert_eq!(s.hold(|live| live), None);
    }

    #[test]
    fn device_choice_is_shared_and_blank_means_default() {
        let s = Sessions::default();
        assert_eq!(s.set_device(Source::Mic, Some("Headset".into())), Ok(()));
        assert_eq!(
            s.devices().lock().ok().and_then(|d| d.mic.clone()),
            Some("Headset".into())
        );
        assert_eq!(s.set_device(Source::Mic, Some(String::new())), Ok(()));
        assert_eq!(s.devices().lock().ok().and_then(|d| d.mic.clone()), None);
    }

    #[test]
    fn failed_prepare_leaves_no_session() {
        let s = Sessions::default();
        assert_eq!(
            s.start(|_| Err("No model".into())),
            Err("No model".to_string())
        );
        assert!(!s.is_running());
    }
}

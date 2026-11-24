//! Live capture that follows the default devices (SPEC 4.1). Portable: the platform sits
//! behind [`Backend`], so device changes, unplug and recovery are tested with a fake.
//! Audio lives in memory only. Device names are never logged or put in errors.
use crate::{AudioError, Frame, Source};
use std::collections::VecDeque;

/// One open capture stream already converted to 16 kHz mono.
/// Not `Send`: platform streams (cpal) must stay on the thread that opened them, so the
/// whole [`LiveCapture`] is created and driven on one dedicated capture thread.
pub trait PcmStream {
    /// Append whatever arrived since the last call. Never blocks.
    fn read(&mut self, out: &mut Vec<f32>);
}

pub trait Backend {
    /// Identifier of the current default device for this source, if any.
    /// Mic is the default input; System is the default render device (loopback).
    fn default_device(&self, source: Source) -> Option<String>;
    fn open(&mut self, source: Source, device: &str) -> Result<Box<dyn PcmStream>, AudioError>;
    /// Names of the input devices that can be chosen for the microphone (empty when unknown).
    fn input_devices(&self) -> Vec<String> {
        Vec::new()
    }
}

/// The microphone the user picked, shared between the settings screen and the capture thread.
pub type MicChoice = std::sync::Arc<std::sync::Mutex<Option<String>>>;

/// Uses the input device the user picked instead of the system default. A picked device that is
/// unplugged falls back to the default until it returns, so transcribing never stops for it.
pub struct PreferredInput<B: Backend> {
    inner: B,
    choice: MicChoice,
}

impl<B: Backend> PreferredInput<B> {
    pub fn new(inner: B, choice: MicChoice) -> Self {
        Self { inner, choice }
    }
}

impl<B: Backend> Backend for PreferredInput<B> {
    fn default_device(&self, source: Source) -> Option<String> {
        if source == Source::Mic {
            let picked = self.choice.lock().ok().and_then(|c| c.clone());
            if let Some(name) = picked {
                if self.inner.input_devices().contains(&name) {
                    return Some(name);
                }
            }
        }
        self.inner.default_device(source)
    }
    fn open(&mut self, source: Source, device: &str) -> Result<Box<dyn PcmStream>, AudioError> {
        self.inner.open(source, device)
    }
    fn input_devices(&self) -> Vec<String> {
        self.inner.input_devices()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureEvent {
    /// The default device changed and capture moved to the new one.
    DeviceChanged(Source),
    /// No device is available (unplugged); will retry.
    Unavailable(Source),
    /// A device came back after being unavailable.
    Restored(Source),
}

struct Slot {
    source: Source,
    enabled: bool,
    open: Option<(String, Box<dyn PcmStream>)>,
    was_unavailable: bool,
    /// Wall-clock ms when the current stream was opened, and the 16 kHz samples it has
    /// delivered since (including silence filled in by [`LiveCapture::poll`]).
    clock: Option<(u64, u64)>,
}

/// A stream that is this far behind the wall clock is treated as having heard silence.
/// WASAPI loopback delivers no packets while nothing is playing, so without this the system
/// timeline would run behind the microphone's (and bleed dedupe would compare lines that are
/// minutes apart in "stream time").
pub const STALL_FILL_AFTER_MS: u64 = 400;
/// How far behind the wall clock a filled stream is left, so late real audio does not push it ahead.
const STALL_FILL_MARGIN_MS: u64 = 200;

pub struct LiveCapture {
    backend: Box<dyn Backend>,
    slots: [Slot; 2],
    frames: VecDeque<Frame>,
    events: Vec<CaptureEvent>,
    last_check_ms: Option<u64>,
}

/// How often the default devices are re-checked.
pub const DEVICE_CHECK_MS: u64 = 1_000;

impl LiveCapture {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        let slot = |source| Slot {
            source,
            enabled: true,
            open: None,
            was_unavailable: false,
            clock: None,
        };
        LiveCapture {
            backend,
            slots: [slot(Source::Mic), slot(Source::System)],
            frames: VecDeque::new(),
            events: Vec::new(),
            last_check_ms: None,
        }
    }

    fn idx(source: Source) -> usize {
        match source {
            Source::Mic => 0,
            Source::System => 1,
        }
    }

    pub fn set_enabled(&mut self, source: Source, on: bool) {
        let s = &mut self.slots[Self::idx(source)];
        s.enabled = on;
        if !on {
            s.open = None; // closing the stream stops capture for that source
            s.clock = None;
            s.was_unavailable = false;
        }
    }

    /// Close every open stream and drop frames not yet taken; the next [`poll`](Self::poll)
    /// reopens the default devices without writing a device-change event. Used on resume:
    /// a stream that ran through a pause still holds audio captured before the resume
    /// (the sound server's buffer, about 2 s on Linux), and fed as new audio it would stamp
    /// every later line late.
    pub fn restart(&mut self) {
        for s in &mut self.slots {
            s.open = None;
            s.clock = None;
        }
        self.frames.clear();
        self.last_check_ms = None;
    }

    pub fn take_events(&mut self) -> Vec<CaptureEvent> {
        std::mem::take(&mut self.events)
    }

    /// Pump: re-check devices about once a second, then collect audio from open streams.
    pub fn poll(&mut self, now_ms: u64) {
        let due = self
            .last_check_ms
            .is_none_or(|t| now_ms.saturating_sub(t) >= DEVICE_CHECK_MS);
        if due {
            self.last_check_ms = Some(now_ms);
            for i in 0..self.slots.len() {
                self.refresh(i, now_ms);
            }
        }
        for s in &mut self.slots {
            if let Some((_, stream)) = s.open.as_mut() {
                let mut buf = Vec::new();
                stream.read(&mut buf);
                if let Some((opened, delivered)) = s.clock.as_mut() {
                    // Silence the platform did not deliver: fill it in ahead of the real audio.
                    let expected = now_ms.saturating_sub(*opened) * 16;
                    let lag = expected.saturating_sub(*delivered + buf.len() as u64);
                    if lag > STALL_FILL_AFTER_MS * 16 {
                        let fill = (lag - STALL_FILL_MARGIN_MS * 16) as usize;
                        *delivered += fill as u64;
                        self.frames.push_back(Frame {
                            source: s.source,
                            samples: vec![0.0; fill],
                        });
                    }
                    *delivered += buf.len() as u64;
                }
                if !buf.is_empty() {
                    self.frames.push_back(Frame {
                        source: s.source,
                        samples: buf,
                    });
                }
            }
        }
    }

    fn refresh(&mut self, i: usize, now_ms: u64) {
        let source = self.slots[i].source;
        if !self.slots[i].enabled {
            return;
        }
        let want = self.backend.default_device(source);
        let have = self.slots[i].open.as_ref().map(|(d, _)| d.clone());
        match (want, have) {
            (None, None) => {}
            (None, Some(_)) => {
                self.slots[i].open = None;
                self.slots[i].clock = None;
                self.slots[i].was_unavailable = true;
                self.events.push(CaptureEvent::Unavailable(source));
            }
            (Some(w), Some(h)) if w == h => {}
            (Some(w), have) => match self.backend.open(source, &w) {
                Ok(stream) => {
                    let slot = &mut self.slots[i];
                    let restored = slot.was_unavailable;
                    slot.open = Some((w, stream));
                    slot.clock = Some((now_ms, 0));
                    slot.was_unavailable = false;
                    if restored {
                        self.events.push(CaptureEvent::Restored(source));
                    } else if have.is_some() {
                        self.events.push(CaptureEvent::DeviceChanged(source));
                    }
                }
                Err(_) => {
                    // Keep the old stream if it still exists; otherwise report once.
                    if have.is_none() && !self.slots[i].was_unavailable {
                        self.slots[i].was_unavailable = true;
                        self.events.push(CaptureEvent::Unavailable(source));
                    }
                }
            },
        }
    }
}

impl LiveCapture {
    /// Next frame collected by [`poll`](Self::poll), oldest first.
    pub fn next_frame(&mut self) -> Option<Frame> {
        self.frames.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct World {
        mic: Option<String>,
        sys: Option<String>,
        fail_open: bool,
        pending: Vec<(String, Vec<f32>)>,
        opened: Vec<String>,
    }
    type Shared = Arc<Mutex<World>>;

    struct FakeStream {
        device: String,
        w: Shared,
    }
    impl PcmStream for FakeStream {
        fn read(&mut self, out: &mut Vec<f32>) {
            let mut w = self.w.lock().unwrap();
            let dev = self.device.clone();
            w.pending.retain(|(d, s)| {
                if *d == dev {
                    out.extend_from_slice(s);
                    false
                } else {
                    true
                }
            });
        }
    }
    struct FakeBackend(Shared);
    impl Backend for FakeBackend {
        fn default_device(&self, s: Source) -> Option<String> {
            let w = self.0.lock().unwrap();
            match s {
                Source::Mic => w.mic.clone(),
                Source::System => w.sys.clone(),
            }
        }
        fn open(&mut self, _s: Source, d: &str) -> Result<Box<dyn PcmStream>, AudioError> {
            let mut w = self.0.lock().unwrap();
            if w.fail_open {
                return Err(AudioError::Unavailable);
            }
            w.opened.push(d.to_string());
            Ok(Box::new(FakeStream {
                device: d.to_string(),
                w: self.0.clone(),
            }))
        }
    }

    #[test]
    fn picked_input_is_used_until_it_is_unplugged() {
        let w: Shared = Arc::new(Mutex::new(World {
            mic: Some("mic-a".into()),
            sys: Some("spk-a".into()),
            ..World::default()
        }));
        struct Listing(FakeBackend, Arc<Mutex<Vec<String>>>);
        impl Backend for Listing {
            fn default_device(&self, s: Source) -> Option<String> {
                self.0.default_device(s)
            }
            fn open(&mut self, s: Source, d: &str) -> Result<Box<dyn PcmStream>, AudioError> {
                self.0.open(s, d)
            }
            fn input_devices(&self) -> Vec<String> {
                self.1.lock().unwrap().clone()
            }
        }
        let present = Arc::new(Mutex::new(vec!["mic-a".to_string(), "usb".to_string()]));
        let choice: MicChoice = Arc::new(Mutex::new(None));
        let mut c = LiveCapture::new(Box::new(PreferredInput::new(
            Listing(FakeBackend(w.clone()), present.clone()),
            choice.clone(),
        )));
        c.poll(0);
        assert_eq!(w.lock().unwrap().opened, vec!["mic-a", "spk-a"]);
        // The user picks the USB mic: capture moves to it within a second.
        *choice.lock().unwrap() = Some("usb".into());
        c.poll(1_000);
        assert_eq!(w.lock().unwrap().opened.last().unwrap(), "usb");
        assert_eq!(
            c.take_events(),
            vec![CaptureEvent::DeviceChanged(Source::Mic)]
        );
        // Unplugged: back to the system default, not silence.
        *present.lock().unwrap() = vec!["mic-a".to_string()];
        c.poll(2_000);
        assert_eq!(w.lock().unwrap().opened.last().unwrap(), "mic-a");
        // Plugged back in: the pick is used again.
        *present.lock().unwrap() = vec!["mic-a".to_string(), "usb".to_string()];
        c.poll(3_000);
        assert_eq!(w.lock().unwrap().opened.last().unwrap(), "usb");
        // Back to "System default".
        *choice.lock().unwrap() = None;
        c.poll(4_000);
        assert_eq!(w.lock().unwrap().opened.last().unwrap(), "mic-a");
    }

    /// Next frame that is not stall-fill silence.
    fn real(c: &mut LiveCapture) -> Option<Frame> {
        while let Some(f) = c.next_frame() {
            if f.samples.iter().any(|x| *x != 0.0) {
                return Some(f);
            }
        }
        None
    }

    fn setup() -> (LiveCapture, Shared) {
        let w: Shared = Arc::new(Mutex::new(World {
            mic: Some("mic-a".into()),
            sys: Some("spk-a".into()),
            ..World::default()
        }));
        (LiveCapture::new(Box::new(FakeBackend(w.clone()))), w)
    }

    #[test]
    fn both_sources_produce_labeled_frames() {
        let (mut c, w) = setup();
        w.lock().unwrap().pending = vec![("mic-a".into(), vec![0.1]), ("spk-a".into(), vec![0.2])];
        c.poll(0);
        let a = c.next_frame().unwrap();
        let b = c.next_frame().unwrap();
        assert_eq!((a.source, b.source), (Source::Mic, Source::System));
        assert!(c.next_frame().is_none());
        assert!(c.take_events().is_empty());
    }

    #[test]
    fn follows_a_default_device_change_without_stopping() {
        let (mut c, w) = setup();
        c.poll(0);
        w.lock().unwrap().mic = Some("headset".into());
        c.poll(500); // too soon to re-check
        assert_eq!(w.lock().unwrap().opened, vec!["mic-a", "spk-a"]);
        c.poll(1_000);
        assert_eq!(w.lock().unwrap().opened.last().unwrap(), "headset");
        assert_eq!(
            c.take_events(),
            vec![CaptureEvent::DeviceChanged(Source::Mic)]
        );
        w.lock().unwrap().pending = vec![("headset".into(), vec![0.3])];
        c.poll(1_100);
        assert_eq!(real(&mut c).unwrap().samples, vec![0.3]);
    }

    #[test]
    fn restart_drops_stale_audio_and_reopens_quietly() {
        let (mut c, w) = setup();
        c.poll(0);
        // Audio buffered by the old streams while paused.
        w.lock().unwrap().pending = vec![("mic-a".into(), vec![0.5]), ("spk-a".into(), vec![0.6])];
        c.restart();
        assert!(c.next_frame().is_none());
        c.poll(10);
        assert_eq!(
            w.lock().unwrap().opened,
            vec!["mic-a", "spk-a", "mic-a", "spk-a"]
        );
        assert!(c.take_events().is_empty());
    }

    #[test]
    fn unplug_reports_once_and_recovers() {
        let (mut c, w) = setup();
        c.poll(0);
        w.lock().unwrap().sys = None;
        c.poll(1_000);
        c.poll(2_000);
        assert_eq!(
            c.take_events(),
            vec![CaptureEvent::Unavailable(Source::System)]
        );
        w.lock().unwrap().sys = Some("spk-b".into());
        c.poll(3_000);
        assert_eq!(
            c.take_events(),
            vec![CaptureEvent::Restored(Source::System)]
        );
    }

    #[test]
    fn failed_open_keeps_old_stream_and_retries() {
        let (mut c, w) = setup();
        c.poll(0);
        {
            let mut g = w.lock().unwrap();
            g.mic = Some("bt".into());
            g.fail_open = true;
        }
        c.poll(1_000);
        assert!(c.take_events().is_empty());
        w.lock().unwrap().pending = vec![("mic-a".into(), vec![0.4])];
        c.poll(1_100);
        assert_eq!(real(&mut c).unwrap().samples, vec![0.4]);
        w.lock().unwrap().fail_open = false;
        c.poll(2_100);
        assert_eq!(
            c.take_events(),
            vec![CaptureEvent::DeviceChanged(Source::Mic)]
        );
    }

    #[test]
    fn disabled_source_is_closed_and_silent() {
        let (mut c, w) = setup();
        c.poll(0);
        c.set_enabled(Source::System, false);
        w.lock().unwrap().pending = vec![("spk-a".into(), vec![0.9])];
        c.poll(1_000);
        while let Some(f) = c.next_frame() {
            assert_eq!(f.source, Source::Mic);
        }
        c.set_enabled(Source::System, true);
        w.lock().unwrap().pending = vec![("spk-a".into(), vec![0.9])];
        c.poll(2_000);
        assert_eq!(real(&mut c).unwrap().source, Source::System);
    }

    #[test]
    fn a_silent_stream_keeps_pace_with_the_wall_clock() {
        // Loopback delivers nothing while nothing plays: its timeline must still advance.
        let (mut c, w) = setup();
        c.poll(0);
        let mut sys = 0usize;
        for t in (100..=10_000).step_by(100) {
            c.poll(t);
            while let Some(f) = c.next_frame() {
                if f.source == Source::System {
                    sys += f.samples.len();
                }
            }
        }
        let ms = sys as u64 / 16;
        assert!((9_500..=10_000).contains(&ms), "system timeline {ms} ms");
        // Then real audio arrives: it follows the filled silence, not a minute-long skew.
        w.lock().unwrap().pending = vec![("spk-a".into(), vec![0.5; 1_600])];
        c.poll(10_100);
        let f = real(&mut c).unwrap();
        assert_eq!(f.samples.len(), 1_600);
    }

    #[test]
    fn a_steady_stream_is_never_padded() {
        let (mut c, w) = setup();
        c.poll(0);
        let mut total = 0usize;
        for t in (20..=5_000).step_by(20) {
            w.lock().unwrap().pending = vec![("mic-a".into(), vec![0.1; 320])];
            c.poll(t);
            while let Some(f) = c.next_frame() {
                if f.source == Source::Mic {
                    assert!(f.samples.iter().all(|x| *x == 0.1));
                    total += f.samples.len();
                }
            }
        }
        assert_eq!(total, 250 * 320);
    }
}

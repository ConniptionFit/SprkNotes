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
}

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
            s.was_unavailable = false;
        }
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
                self.refresh(i);
            }
        }
        for s in &mut self.slots {
            if let Some((_, stream)) = s.open.as_mut() {
                let mut buf = Vec::new();
                stream.read(&mut buf);
                if !buf.is_empty() {
                    self.frames.push_back(Frame {
                        source: s.source,
                        samples: buf,
                    });
                }
            }
        }
    }

    fn refresh(&mut self, i: usize) {
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
                self.slots[i].was_unavailable = true;
                self.events.push(CaptureEvent::Unavailable(source));
            }
            (Some(w), Some(h)) if w == h => {}
            (Some(w), have) => match self.backend.open(source, &w) {
                Ok(stream) => {
                    let slot = &mut self.slots[i];
                    let restored = slot.was_unavailable;
                    slot.open = Some((w, stream));
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
        assert_eq!(c.next_frame().unwrap().samples, vec![0.3]);
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
        assert_eq!(c.next_frame().unwrap().samples, vec![0.4]);
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
        assert!(c.next_frame().is_none());
        c.set_enabled(Source::System, true);
        c.poll(2_000);
        assert_eq!(c.next_frame().unwrap().source, Source::System);
    }
}

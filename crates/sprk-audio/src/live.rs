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
    /// The same audio as [`read`](Self::read), split into the packets the device handed over,
    /// each with how long before this call it arrived. Streams that cannot tell return one packet
    /// of unknown age. Never blocks.
    fn read_packets(&mut self, out: &mut Vec<Packet>) {
        let mut samples = Vec::new();
        self.read(&mut samples);
        if !samples.is_empty() {
            out.push(Packet {
                samples,
                age_ms: None,
            });
        }
    }
}

/// Audio a device delivered in one go (16 kHz mono).
#[derive(Debug, Clone, PartialEq)]
pub struct Packet {
    pub samples: Vec<f32>,
    /// Milliseconds between the device delivering this audio and the read that returned it.
    /// None when the stream cannot tell.
    pub age_ms: Option<u64>,
}

pub trait Backend {
    /// Identifier of the current default device for this source, if any.
    /// Mic is the default input; System is the default render device (loopback).
    fn default_device(&self, source: Source) -> Option<String>;
    fn open(&mut self, source: Source, device: &str) -> Result<Box<dyn PcmStream>, AudioError>;
    /// Names of the devices that can be chosen for a source: input devices for the mic, render
    /// devices (looped back) for the system stream. Empty when unknown.
    fn devices(&self, _source: Source) -> Vec<String> {
        Vec::new()
    }
}

/// The devices the user picked (None: follow the system default), shared between the settings
/// screen and the capture thread.
#[derive(Debug, Default)]
pub struct DeviceChoice {
    pub mic: Option<String>,
    pub system: Option<String>,
}
pub type SharedChoice = std::sync::Arc<std::sync::Mutex<DeviceChoice>>;

/// Uses the devices the user picked instead of the system defaults. A picked device that is
/// unplugged falls back to the default until it returns, so transcribing never stops for it.
pub struct Preferred<B: Backend> {
    inner: B,
    choice: SharedChoice,
}

impl<B: Backend> Preferred<B> {
    pub fn new(inner: B, choice: SharedChoice) -> Self {
        Self { inner, choice }
    }
}

impl<B: Backend> Backend for Preferred<B> {
    fn default_device(&self, source: Source) -> Option<String> {
        let picked = self.choice.lock().ok().and_then(|c| match source {
            Source::Mic => c.mic.clone(),
            Source::System => c.system.clone(),
        });
        if let Some(name) = picked {
            if self.inner.devices(source).contains(&name) {
                return Some(name);
            }
        }
        self.inner.default_device(source)
    }
    fn open(&mut self, source: Source, device: &str) -> Result<Box<dyn PcmStream>, AudioError> {
        self.inner.open(source, device)
    }
    fn devices(&self, source: Source) -> Vec<String> {
        self.inner.devices(source)
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
    /// Timed streams: when the last packet arrived (wall-clock ms), and how far (16 kHz samples)
    /// the audio normally runs behind its arrival while it flows (the device's buffering).
    last_arrival: Option<u64>,
    latency: Option<u64>,
}

impl Slot {
    fn fill(&mut self, n: u64, frames: &mut VecDeque<Frame>) {
        if n == 0 {
            return;
        }
        if let Some((_, delivered)) = self.clock.as_mut() {
            *delivered += n;
        }
        frames.push_back(Frame {
            source: self.source,
            samples: vec![0.0; n as usize],
        });
    }

    /// Samples behind the clock at `at_ms` once `incoming` more samples are counted.
    fn lag_at(&self, at_ms: u64, incoming: usize) -> Option<u64> {
        let (opened, delivered) = self.clock?;
        let expected = at_ms.saturating_sub(opened) * 16;
        Some(expected.saturating_sub(delivered + incoming as u64))
    }

    /// A packet whose arrival time is unknown (or no packet: `samples` empty). If the stream is
    /// far behind the clock it heard silence: fill most of it, leaving a margin so late real
    /// audio does not run ahead.
    fn place_untimed(&mut self, now_ms: u64, samples: &[f32], frames: &mut VecDeque<Frame>) {
        if samples.is_empty() && self.last_arrival.is_some() {
            // Timed stream: only fill once the last packet is older than the stall limit.
            let quiet = self
                .last_arrival
                .is_some_and(|a| now_ms.saturating_sub(a) > STALL_FILL_AFTER_MS);
            if !quiet {
                return;
            }
        }
        if let Some(lag) = self.lag_at(now_ms, samples.len()) {
            if lag > STALL_FILL_AFTER_MS * 16 {
                self.fill(lag - STALL_FILL_MARGIN_MS * 16, frames);
            }
        }
        if let Some((_, delivered)) = self.clock.as_mut() {
            *delivered += samples.len() as u64;
        }
    }

    /// A packet that arrived at `arrived_ms`. After a silence (or as the first packet) it is placed
    /// where it was heard, so the two streams stay lined up sample for sample: the microphone's
    /// echo is matched against the loopback at a fixed offset, and audio placed a few hundred ms
    /// off puts the echo where the canceller cannot see it. While audio flows, the device's
    /// usual delay is learned.
    fn place_timed(&mut self, arrived_ms: u64, samples: &[f32], frames: &mut VecDeque<Frame>) {
        let latency = self.latency.unwrap_or(DEFAULT_LATENCY_MS * 16);
        let after_gap = self
            .last_arrival
            .is_none_or(|a| arrived_ms.saturating_sub(a) >= GAP_MS);
        self.last_arrival = Some(self.last_arrival.map_or(arrived_ms, |a| a.max(arrived_ms)));
        let Some(lag) = self.lag_at(arrived_ms, samples.len()) else {
            return;
        };
        if after_gap {
            if lag > latency + PLACE_SLACK_MS * 16 {
                self.fill(lag - latency, frames);
            }
        } else {
            self.latency = Some(self.latency.map_or(lag, |l| (l * 15 + lag) / 16));
            if lag > STALL_FILL_AFTER_MS * 16 {
                self.fill(lag - latency, frames);
            }
        }
        if let Some((_, delivered)) = self.clock.as_mut() {
            *delivered += samples.len() as u64;
        }
    }
}

/// A stream that is this far behind the wall clock is treated as having heard silence.
/// WASAPI loopback delivers no packets while nothing is playing, so without this the system
/// timeline would run behind the microphone's (and bleed dedupe would compare lines that are
/// minutes apart in "stream time").
pub const STALL_FILL_AFTER_MS: u64 = 400;
/// How far behind the wall clock a filled stream is left, so late real audio does not push it ahead.
const STALL_FILL_MARGIN_MS: u64 = 200;
/// Timed streams: packets this far apart have silence between them (WASAPI loopback sends
/// nothing while nothing plays), and the packet after the gap goes where it was heard.
const GAP_MS: u64 = 60;
/// Device buffering assumed until a timed stream has been seen flowing.
const DEFAULT_LATENCY_MS: u64 = 10;
/// A packet within this much of where it belongs is not moved.
const PLACE_SLACK_MS: u64 = 10;

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
            last_arrival: None,
            latency: None,
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
            s.last_arrival = None;
            s.latency = None;
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
            s.last_arrival = None;
            s.latency = None;
        }
        self.frames.clear();
        self.last_check_ms = None;
    }

    /// Name of the device a source is capturing from now (None while it has none). It may carry a
    /// platform tag after a control character; callers show only the text before it.
    pub fn device_name(&self, source: Source) -> Option<&str> {
        self.slots[Self::idx(source)]
            .open
            .as_ref()
            .map(|(d, _)| d.as_str())
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
        // System audio first: the microphone's echo canceller needs the speaker audio for a
        // moment before the microphone audio of that moment, and after the session thread was
        // busy (decoding) both arrive at once. Mic first would process seconds of microphone
        // audio with no reference and let the speakers through.
        for s in self.slots.iter_mut().rev() {
            let Some((_, stream)) = s.open.as_mut() else {
                continue;
            };
            let mut packets = Vec::new();
            stream.read_packets(&mut packets);
            for p in packets {
                match p.age_ms {
                    Some(age) => {
                        s.place_timed(now_ms.saturating_sub(age), &p.samples, &mut self.frames)
                    }
                    None => s.place_untimed(now_ms, &p.samples, &mut self.frames),
                }
                if !p.samples.is_empty() {
                    self.frames.push_back(Frame {
                        source: s.source,
                        samples: p.samples,
                    });
                }
            }
            // Silence the platform did not deliver: keep up with the clock while it lasts.
            s.place_untimed(now_ms, &[], &mut self.frames);
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
                self.slots[i].last_arrival = None;
                self.slots[i].latency = None;
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
                    slot.last_arrival = None;
                    slot.latency = None;
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
            fn devices(&self, s: Source) -> Vec<String> {
                match s {
                    Source::Mic => self.1.lock().unwrap().clone(),
                    Source::System => vec!["spk-a".into(), "spk-b".into()],
                }
            }
        }
        let present = Arc::new(Mutex::new(vec!["mic-a".to_string(), "usb".to_string()]));
        let choice: SharedChoice = Arc::new(Mutex::new(DeviceChoice::default()));
        let mut c = LiveCapture::new(Box::new(Preferred::new(
            Listing(FakeBackend(w.clone()), present.clone()),
            choice.clone(),
        )));
        c.poll(0);
        assert_eq!(w.lock().unwrap().opened, vec!["mic-a", "spk-a"]);
        // The user picks the USB mic: capture moves to it within a second.
        choice.lock().unwrap().mic = Some("usb".into());
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
        choice.lock().unwrap().mic = None;
        c.poll(4_000);
        assert_eq!(w.lock().unwrap().opened.last().unwrap(), "mic-a");
        // The system stream can be picked too, and the name is visible for the transcript.
        choice.lock().unwrap().system = Some("spk-b".into());
        c.poll(5_000);
        assert_eq!(c.device_name(Source::System), Some("spk-b"));
        assert_eq!(c.device_name(Source::Mic), Some("mic-a"));
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
    fn each_source_gets_exactly_its_own_samples() {
        // Two different signals, delivered in uneven bursts as a busy machine would: every
        // frame must carry only its own source's samples, unchanged and in order. Mic samples
        // are positive and System samples negative, so any mix-up shows.
        let (mut c, w) = setup();
        let mic: Vec<f32> = (1..=40_000).map(|k| k as f32 * 1e-5).collect();
        let sys: Vec<f32> = (1..=40_000).map(|k| -(k as f32) * 1e-5).collect();
        let (mut mi, mut si, mut now) = (0usize, 0usize, 0u64);
        let (mut got_mic, mut got_sys) = (Vec::new(), Vec::new());
        let mut step = 0usize;
        while mi < mic.len() || si < sys.len() {
            step += 1;
            let m = (step * 37 % 900 + 1).min(mic.len() - mi);
            let k = (step * 53 % 1_300 + 1).min(sys.len() - si);
            {
                let mut w = w.lock().unwrap();
                w.pending.push(("mic-a".into(), mic[mi..mi + m].to_vec()));
                w.pending.push(("spk-a".into(), sys[si..si + k].to_vec()));
            }
            mi += m;
            si += k;
            now += 20;
            c.poll(now);
            while let Some(f) = c.next_frame() {
                for &v in &f.samples {
                    match f.source {
                        Source::Mic => assert!(v >= 0.0, "System sample in a Mic frame"),
                        Source::System => assert!(v <= 0.0, "Mic sample in a System frame"),
                    }
                }
                let keep = f.samples.iter().copied().filter(|v| *v != 0.0);
                match f.source {
                    Source::Mic => got_mic.extend(keep),
                    Source::System => got_sys.extend(keep),
                }
            }
        }
        assert_eq!(got_mic, mic, "Mic samples were changed, lost or reordered");
        assert_eq!(
            got_sys, sys,
            "System samples were changed, lost or reordered"
        );
    }

    #[test]
    fn both_sources_produce_labeled_frames() {
        let (mut c, w) = setup();
        w.lock().unwrap().pending = vec![("mic-a".into(), vec![0.1]), ("spk-a".into(), vec![0.2])];
        c.poll(0);
        let a = c.next_frame().unwrap();
        let b = c.next_frame().unwrap();
        // System audio comes first: the echo canceller needs it before the mic audio.
        assert_eq!((a.source, b.source), (Source::System, Source::Mic));
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

    /// A device that reports when each packet arrived.
    #[derive(Default)]
    struct Timed {
        now: u64,
        packets: Vec<(Source, u64, Vec<f32>)>,
    }
    type SharedTimed = Arc<Mutex<Timed>>;
    struct TimedStream(SharedTimed, Source);
    impl PcmStream for TimedStream {
        fn read(&mut self, _out: &mut Vec<f32>) {}
        fn read_packets(&mut self, out: &mut Vec<Packet>) {
            let mut t = self.0.lock().unwrap();
            let now = t.now;
            let me = self.1;
            let mut keep = Vec::new();
            for (src, at, samples) in t.packets.drain(..) {
                if src == me {
                    out.push(Packet {
                        samples,
                        age_ms: Some(now - at),
                    });
                } else {
                    keep.push((src, at, samples));
                }
            }
            t.packets = keep;
        }
    }
    struct TimedBackend(SharedTimed);
    impl Backend for TimedBackend {
        fn default_device(&self, s: Source) -> Option<String> {
            Some(format!("{s:?}"))
        }
        fn open(&mut self, s: Source, _d: &str) -> Result<Box<dyn PcmStream>, AudioError> {
            Ok(Box::new(TimedStream(self.0.clone(), s)))
        }
    }

    /// Collect the system stream's time line (16 kHz samples, filled silence included).
    fn system_line(c: &mut LiveCapture, line: &mut Vec<f32>) {
        while let Some(f) = c.next_frame() {
            if f.source == Source::System {
                line.extend(f.samples);
            }
        }
    }

    #[test]
    fn loopback_that_resumes_while_the_thread_was_busy_lands_where_it_was_heard() {
        let t: SharedTimed = Arc::default();
        let mut c = LiveCapture::new(Box::new(TimedBackend(t.clone())));
        let mut line = Vec::new();
        c.poll(0);
        // Quiet for 3 s (loopback sends nothing), polled every 20 ms.
        for ms in (20..=3_000).step_by(20) {
            t.lock().unwrap().now = ms;
            c.poll(ms);
            system_line(&mut c, &mut line);
        }
        // Playback starts at 3.2 s while the session thread is busy until 4.0 s.
        {
            let mut w = t.lock().unwrap();
            for k in 0..80u64 {
                w.packets
                    .push((Source::System, 3_210 + k * 10, vec![0.5; 160]));
            }
            w.now = 4_000;
        }
        c.poll(4_000);
        system_line(&mut c, &mut line);
        let first = line.iter().position(|x| *x != 0.0).unwrap() as u64 / 16;
        // Heard 3200 to 3210 ms (default device delay 10 ms): within one packet of that.
        assert!((3_190..=3_215).contains(&first), "placed at {first} ms");
    }

    #[test]
    fn audio_heard_before_a_quiet_stretch_is_not_pushed_later() {
        let t: SharedTimed = Arc::default();
        let mut c = LiveCapture::new(Box::new(TimedBackend(t.clone())));
        let mut line = Vec::new();
        c.poll(0);
        // Playback flows from the start: 10 ms packets, 10 ms device delay.
        for ms in (10..=2_000).step_by(10) {
            let mut w = t.lock().unwrap();
            w.packets.push((Source::System, ms, vec![0.25; 160]));
            w.now = ms;
            drop(w);
            if ms % 20 == 0 {
                c.poll(ms);
                system_line(&mut c, &mut line);
            }
        }
        // A last phrase plays until 2.5 s, then nothing; the thread was busy until 3.4 s.
        {
            let mut w = t.lock().unwrap();
            for k in 1..=50u64 {
                w.packets
                    .push((Source::System, 2_000 + k * 10, vec![0.5; 160]));
            }
            w.now = 3_400;
        }
        c.poll(3_400);
        system_line(&mut c, &mut line);
        let first = line.iter().position(|x| *x == 0.5).unwrap() as u64 / 16;
        assert!(
            (1_990..=2_010).contains(&first),
            "phrase placed at {first} ms"
        );
        // The silence after it is filled behind the phrase, not in front of it.
        let last = line.iter().rposition(|x| *x == 0.5).unwrap() as u64 / 16;
        assert!((2_480..=2_510).contains(&last), "phrase ends at {last} ms");
    }
}

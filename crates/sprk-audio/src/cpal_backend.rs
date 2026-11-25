//! cpal implementation of [`Backend`] (WASAPI on Windows). The System source opens the default
//! render device as an input, which cpal turns into WASAPI loopback on Windows.
//! The callback only pushes into a lock-free ring buffer: no allocation, no locks.
//! Compiled with the `cpal-backend` feature.
use crate::live::{Backend, Packet, PcmStream};
use crate::{AudioError, Resampler, Source};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::HeapRb;
use std::time::Instant;

pub struct CpalBackend {
    host: cpal::Host,
}

impl Default for CpalBackend {
    fn default() -> Self {
        Self {
            host: cpal::default_host(),
        }
    }
}

struct CpalStream {
    // Dropping the stream stops capture.
    _stream: cpal::Stream,
    cons: ringbuf::HeapCons<f32>,
    /// One mark per device callback: device samples pushed so far, and when the callback ran.
    marks: ringbuf::HeapCons<(u64, Instant)>,
    /// Device samples read so far.
    taken: u64,
    resampler: Resampler,
    scratch: Vec<f32>,
}

impl PcmStream for CpalStream {
    fn read(&mut self, out: &mut Vec<f32>) {
        let mut packets = Vec::new();
        self.read_packets(&mut packets);
        for p in packets {
            out.extend(p.samples);
        }
    }

    /// Packets as the device delivered them, each with its arrival time. WASAPI loopback sends
    /// nothing while nothing plays, and the session thread may be busy decoding when playback
    /// starts again; the arrival time says where the audio belongs on the time line.
    fn read_packets(&mut self, out: &mut Vec<Packet>) {
        let now = Instant::now();
        while let Some((end, at)) = self.marks.try_pop() {
            let mut want = end.saturating_sub(self.taken) as usize;
            let mut samples = Vec::new();
            while want > 0 {
                let k = want.min(self.scratch.len());
                let n = self.cons.pop_slice(&mut self.scratch[..k]);
                if n == 0 {
                    break;
                }
                samples.extend(self.resampler.process(&self.scratch[..n]));
                self.taken += n as u64;
                want -= n;
            }
            if !samples.is_empty() {
                out.push(Packet {
                    samples,
                    age_ms: Some(now.saturating_duration_since(at).as_millis() as u64),
                });
            }
        }
    }
}

impl Backend for CpalBackend {
    fn default_device(&self, source: Source) -> Option<String> {
        // Android: apps cannot capture other apps' audio, so a phone has the microphone only
        // (ADR-015 Decision 4).
        #[cfg(target_os = "android")]
        if source == Source::System {
            return None;
        }
        #[cfg(all(not(windows), not(target_os = "android")))]
        if source == Source::System {
            return monitor::default_monitor();
        }
        // Windows: the communications default first. A headset's hands-free mic and the call's
        // audio are only on those endpoints, and the console default can be the laptop's own mic
        // and speakers (see endpoints.rs). Falls back to the console default.
        #[cfg(windows)]
        {
            let capture = source == Source::Mic;
            if let Some(n) =
                crate::endpoints::default_name(capture, crate::endpoints::Role::Communications)
            {
                if self.devices(source).contains(&n) {
                    return Some(n);
                }
            }
        }
        let d = match source {
            Source::Mic => self.host.default_input_device(),
            Source::System => self.host.default_output_device(),
        };
        let name = d.and_then(|d| d.name().ok())?;
        // ALSA's "default" device keeps its name when the user switches the default input, so
        // append the sound server's current default source. A switch then looks like a new
        // device and the stream is reopened (and a "Source changed" line is written).
        #[cfg(all(not(windows), not(target_os = "android")))]
        if source == Source::Mic {
            return Some(monitor::tag_device(
                &name,
                monitor::pactl_default("get-default-source"),
            ));
        }
        Some(name)
    }

    fn devices(&self, source: Source) -> Vec<String> {
        // Android: apps cannot capture other apps' audio, so a phone has the microphone only
        // (ADR-015 Decision 4).
        #[cfg(target_os = "android")]
        if source == Source::System {
            return Vec::new();
        }
        #[cfg(all(not(windows), not(target_os = "android")))]
        if source == Source::System {
            return Vec::new(); // Linux system audio is the default output's monitor
        }
        let list = match source {
            Source::Mic => self
                .host
                .input_devices()
                .map(|it| it.filter_map(|d| d.name().ok()).collect::<Vec<_>>()),
            Source::System => self
                .host
                .output_devices()
                .map(|it| it.filter_map(|d| d.name().ok()).collect::<Vec<_>>()),
        };
        let mut names = list.unwrap_or_default();
        names.sort();
        names.dedup();
        names
    }

    fn open(&mut self, source: Source, name: &str) -> Result<Box<dyn PcmStream>, AudioError> {
        // Android: apps cannot capture other apps' audio, so a phone has the microphone only
        // (ADR-015 Decision 4).
        #[cfg(target_os = "android")]
        if source == Source::System {
            return Err(AudioError::Unavailable);
        }
        #[cfg(all(not(windows), not(target_os = "android")))]
        if source == Source::System {
            return monitor::open(name);
        }
        let name = monitor_strip(name);
        let device = match source {
            Source::Mic => self
                .host
                .input_devices()
                .ok()
                .and_then(|mut it| it.find(|d| d.name().is_ok_and(|n| n == name))),
            Source::System => self
                .host
                .output_devices()
                .ok()
                .and_then(|mut it| it.find(|d| d.name().is_ok_and(|n| n == name))),
        }
        .ok_or(AudioError::Unavailable)?;
        let cfg = match source {
            Source::Mic => device.default_input_config(),
            // For loopback the render format is the capture format.
            Source::System => device.default_output_config(),
        }
        .map_err(|_| AudioError::Unavailable)?;
        let (rate, channels) = (cfg.sample_rate().0, usize::from(cfg.channels()).max(1));
        let (mut prod, cons) = HeapRb::<f32>::new(rate as usize * 10).split();
        // Room for 10 s of callbacks at the usual 10 ms period; when full, the next mark covers more.
        let (mut mark_prod, marks) = HeapRb::<(u64, Instant)>::new(1_024).split();
        let mut pushed = 0u64;
        let err_fn = |_e| {}; // errors carry device text; the poll loop notices a dead device instead
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &cfg.config(),
                move |data: &[f32], _| {
                    for f in data.chunks_exact(channels) {
                        if prod
                            .try_push(f.iter().sum::<f32>() / channels as f32)
                            .is_ok()
                        {
                            pushed += 1;
                        }
                    }
                    let _ = mark_prod.try_push((pushed, Instant::now()));
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &cfg.config(),
                move |data: &[i16], _| {
                    for f in data.chunks_exact(channels) {
                        let s: f32 = f.iter().map(|x| f32::from(*x) / 32768.0).sum();
                        if prod.try_push(s / channels as f32).is_ok() {
                            pushed += 1;
                        }
                    }
                    let _ = mark_prod.try_push((pushed, Instant::now()));
                },
                err_fn,
                None,
            ),
            _ => return Err(AudioError::Unavailable),
        }
        .map_err(|_| AudioError::Unavailable)?;
        stream.play().map_err(|_| AudioError::Unavailable)?;
        Ok(Box::new(CpalStream {
            _stream: stream,
            cons,
            marks,
            taken: 0,
            resampler: Resampler::new(rate, 16_000),
            scratch: vec![0.0; 8192],
        }))
    }
}

/// The device name without the Linux default-source tag (see `monitor::tag_device`).
fn monitor_strip(name: &str) -> &str {
    name.split(TAG_SEP).next().unwrap_or(name)
}

const TAG_SEP: char = '\u{1f}';

/// Linux system audio: the default output's monitor source, read through `parec` (PipeWire and
/// PulseAudio both serve it). cpal cannot open a render device as an input off Windows.
#[cfg(all(not(windows), not(target_os = "android")))]
mod monitor {
    use super::*;
    use std::io::Read;
    use std::process::{Child, Command, Stdio};
    use std::sync::{Arc, Mutex};

    pub const DEFAULT_MONITOR: &str = "@DEFAULT_MONITOR@";

    pub fn available() -> bool {
        Command::new("parec")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    /// Ask the sound server (`pactl`, served by PipeWire and PulseAudio) for the current default
    /// sink or source name. None when pactl is missing or answers nothing useful.
    pub fn pactl_default(which: &str) -> Option<String> {
        let out = Command::new("pactl")
            .arg(which)
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!name.is_empty()).then_some(name)
    }

    /// The system source: the default sink's monitor, named explicitly so that switching the
    /// default output changes the name. Falls back to parec's own `@DEFAULT_MONITOR@`.
    pub fn default_monitor() -> Option<String> {
        if !available() {
            return None;
        }
        Some(pactl_default("get-default-sink").map_or_else(
            || DEFAULT_MONITOR.to_string(),
            |sink| format!("{sink}.monitor"),
        ))
    }

    /// `name` plus the default source, joined so a change in either shows up.
    pub fn tag_device(name: &str, source: Option<String>) -> String {
        match source {
            Some(s) => format!("{name}{TAG_SEP}{s}"),
            None => name.to_string(),
        }
    }

    struct ParecStream {
        child: Child,
        buf: Arc<Mutex<Vec<f32>>>,
    }

    impl PcmStream for ParecStream {
        fn read(&mut self, out: &mut Vec<f32>) {
            if let Ok(mut b) = self.buf.try_lock() {
                out.append(&mut b);
            }
        }
    }

    impl Drop for ParecStream {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    pub fn open(device: &str) -> Result<Box<dyn PcmStream>, AudioError> {
        let mut child = Command::new("parec")
            .args([
                "-d",
                device,
                "--raw",
                "--format=float32le",
                "--rate=16000",
                "--channels=1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| AudioError::Unavailable)?;
        let mut out = child.stdout.take().ok_or(AudioError::Unavailable)?;
        let buf = Arc::new(Mutex::new(Vec::<f32>::new()));
        let sink = Arc::clone(&buf);
        std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            let mut pending: Vec<u8> = Vec::new();
            while let Ok(n) = out.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                pending.extend_from_slice(&chunk[..n]);
                let whole = pending.len() / 4 * 4;
                if let Ok(mut b) = sink.lock() {
                    b.extend(
                        pending[..whole]
                            .chunks_exact(4)
                            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])),
                    );
                }
                pending.drain(..whole);
            }
        });
        Ok(Box::new(ParecStream { child, buf }))
    }
}

#[cfg(all(test, not(windows), not(target_os = "android")))]
mod tests {
    use super::*;

    #[test]
    fn tag_changes_with_the_default_source_and_strips_back() {
        let a = monitor::tag_device("default", Some("src-a".into()));
        let b = monitor::tag_device("default", Some("src-b".into()));
        assert_ne!(a, b);
        assert_eq!(monitor_strip(&a), "default");
        assert_eq!(monitor::tag_device("default", None), "default");
    }
}

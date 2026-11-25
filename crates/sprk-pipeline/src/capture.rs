//! Capture on its own thread (SPEC 3.2). The session thread decodes speech and live captions,
//! and a decode can take hundreds of milliseconds. Polling the streams on that same thread
//! meant no audio was read while a decode ran: the device buffers filled, the speaker stream's
//! silent stretches were measured from late reads, and Pause or End waited for the decode.
//! Here the streams are polled every 20 ms whatever the session thread is doing, and the
//! 16 kHz frames are handed over in order through a channel (memory only; nothing is written).
//!
//! The backend is built on this thread because platform streams must stay on the thread that
//! opened them. A panic here is passed back to the session thread, which closes the file as
//! interrupted exactly as it did when capture ran on that thread.
use sprk_audio::live::{Backend, CaptureEvent, LiveCapture};
use sprk_audio::{Frame, Source};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How often the streams are read.
const POLL: Duration = Duration::from_millis(20);
/// Longest wait for the capture thread to answer a sync. It answers within one poll; this only
/// keeps Pause and End from hanging if a device call never returns.
const SYNC_WAIT: Duration = Duration::from_secs(2);

enum ToCapture {
    /// Read now, send everything, then a `Synced` marker.
    Sync,
    /// Reopen the streams and send a `Synced` marker before any audio read after it.
    Restart,
    SetEnabled(Source, bool),
    Stop,
}

/// What capture produced, in the order it happened.
pub enum Captured {
    Frame(Frame),
    Event(CaptureEvent),
    /// The device in use for a source changed (None: none open).
    Device(Source, Option<String>),
}

enum FromCapture {
    Out(Captured),
    Synced,
}

pub struct CaptureThread {
    tx: Sender<ToCapture>,
    rx: Receiver<FromCapture>,
    join: Option<JoinHandle<()>>,
    names: [Option<String>; 2],
}

fn slot(s: Source) -> usize {
    usize::from(s == Source::System)
}

impl CaptureThread {
    /// Start capturing. `start` is the session's time origin, so both threads share one clock.
    pub fn spawn(
        make_backend: Box<dyn FnOnce() -> Box<dyn Backend> + Send>,
        start: Instant,
    ) -> std::io::Result<Self> {
        let (tx, cmd_rx) = channel();
        let (out_tx, rx) = channel();
        let join = std::thread::Builder::new()
            .name("sprk-capture".into())
            .spawn(move || run(make_backend, start, &cmd_rx, &out_tx))?;
        Ok(Self {
            tx,
            rx,
            join: Some(join),
            names: [None, None],
        })
    }

    /// Everything captured since the last call, without waiting.
    pub fn take(&mut self) -> Vec<Captured> {
        let mut out = Vec::new();
        while let Ok(m) = self.rx.try_recv() {
            if let FromCapture::Out(c) = m {
                self.note(&c);
                out.push(c);
            }
        }
        out
    }

    /// Read the streams now and return everything up to this moment (Pause, End).
    pub fn sync(&mut self) -> Vec<Captured> {
        if self.tx.send(ToCapture::Sync).is_err() {
            return self.take();
        }
        self.until_synced(true)
    }

    /// Fresh streams (Resume): audio buffered before this call is dropped, not fed as new.
    /// Device and event notices that were waiting are still returned.
    pub fn restart(&mut self) -> Vec<Captured> {
        if self.tx.send(ToCapture::Restart).is_err() {
            return Vec::new();
        }
        self.until_synced(false)
    }

    pub fn set_enabled(&mut self, s: Source, on: bool) {
        let _ = self.tx.send(ToCapture::SetEnabled(s, on));
    }

    pub fn device_name(&self, s: Source) -> Option<&str> {
        self.names[slot(s)].as_deref()
    }

    /// Stop and release the devices. If the capture thread panicked, the panic carries on
    /// here so the session closes the same way as before.
    pub fn stop(mut self) {
        if let Err(p) = self.finish() {
            std::panic::resume_unwind(p);
        }
    }

    fn finish(&mut self) -> std::thread::Result<()> {
        let _ = self.tx.send(ToCapture::Stop);
        self.join.take().map_or(Ok(()), JoinHandle::join)
    }

    /// Whether the capture thread has died (it only exits on Stop, so this means a panic).
    pub fn failed(&self) -> bool {
        self.join.as_ref().is_some_and(JoinHandle::is_finished)
    }

    /// If the capture thread panicked, continue that panic on the calling thread.
    pub fn rethrow_if_failed(&mut self) {
        if self.failed() {
            if let Err(p) = self.finish() {
                std::panic::resume_unwind(p);
            }
        }
    }

    fn note(&mut self, c: &Captured) {
        if let Captured::Device(s, n) = c {
            self.names[slot(*s)].clone_from(n);
        }
    }

    fn until_synced(&mut self, keep_frames: bool) -> Vec<Captured> {
        let mut out = Vec::new();
        let deadline = Instant::now() + SYNC_WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(FromCapture::Synced) => break,
                Ok(FromCapture::Out(c)) => {
                    self.note(&c);
                    if keep_frames || !matches!(c, Captured::Frame(_)) {
                        out.push(c);
                    }
                }
                Err(_) => break,
            }
        }
        out
    }
}

impl Drop for CaptureThread {
    /// Dropped without `stop` (the session thread is unwinding): release the devices quietly.
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

fn run(
    make_backend: Box<dyn FnOnce() -> Box<dyn Backend> + Send>,
    start: Instant,
    rx: &Receiver<ToCapture>,
    tx: &Sender<FromCapture>,
) {
    let mut cap = LiveCapture::new(make_backend());
    let mut names: [Option<String>; 2] = [None, None];
    loop {
        let cmd = rx.recv_timeout(POLL);
        let synced_first = match &cmd {
            Ok(ToCapture::Restart) => {
                cap.restart();
                true
            }
            Ok(ToCapture::SetEnabled(s, on)) => {
                cap.set_enabled(*s, *on);
                false
            }
            _ => false,
        };
        if synced_first && tx.send(FromCapture::Synced).is_err() {
            return;
        }
        let now = start.elapsed().as_millis() as u64;
        if pump(&mut cap, now, &mut names, tx).is_err() {
            return;
        }
        match cmd {
            Ok(ToCapture::Sync) => {
                if tx.send(FromCapture::Synced).is_err() {
                    return;
                }
            }
            Ok(ToCapture::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            _ => {}
        }
    }
}

/// One read, in the order the session thread used to see it: notices, then device names,
/// then audio.
fn pump(
    cap: &mut LiveCapture,
    now_ms: u64,
    names: &mut [Option<String>; 2],
    tx: &Sender<FromCapture>,
) -> Result<(), ()> {
    let send = |c: Captured| tx.send(FromCapture::Out(c)).map_err(|_| ());
    cap.poll(now_ms);
    for ev in cap.take_events() {
        send(Captured::Event(ev))?;
    }
    for s in [Source::Mic, Source::System] {
        let now = cap.device_name(s).map(str::to_owned);
        if now != names[slot(s)] {
            names[slot(s)].clone_from(&now);
            send(Captured::Device(s, now))?;
        }
    }
    while let Some(f) = cap.next_frame() {
        send(Captured::Frame(f))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprk_audio::live::PcmStream;
    use sprk_audio::AudioError;
    use std::sync::{Arc, Mutex};

    /// Hands out 20 ms of a steady tone per read and logs when each read happened.
    struct Tone {
        reads: Arc<Mutex<Vec<Instant>>>,
        last: Option<Instant>,
        level: f32,
    }
    impl PcmStream for Tone {
        fn read(&mut self, out: &mut Vec<f32>) {
            let now = Instant::now();
            self.reads.lock().unwrap().push(now);
            let ms = self
                .last
                .map_or(20, |l| now.duration_since(l).as_millis() as usize);
            self.last = Some(now);
            out.extend(std::iter::repeat_n(self.level, ms * 16));
        }
    }
    struct ToneBackend {
        reads: Arc<Mutex<Vec<Instant>>>,
        opened: Arc<Mutex<u32>>,
    }
    impl Backend for ToneBackend {
        fn default_device(&self, s: Source) -> Option<String> {
            (s == Source::Mic).then(|| "mic".to_string())
        }
        fn open(&mut self, _: Source, _: &str) -> Result<Box<dyn PcmStream>, AudioError> {
            let mut n = self.opened.lock().unwrap();
            *n += 1;
            Ok(Box::new(Tone {
                reads: self.reads.clone(),
                last: None,
                level: *n as f32,
            }))
        }
    }

    fn spawn() -> (CaptureThread, Arc<Mutex<Vec<Instant>>>) {
        let reads = Arc::new(Mutex::new(Vec::new()));
        let r = reads.clone();
        let cap = CaptureThread::spawn(
            Box::new(move || {
                Box::new(ToneBackend {
                    reads: r,
                    opened: Arc::new(Mutex::new(0)),
                })
            }),
            Instant::now(),
        )
        .unwrap();
        (cap, reads)
    }

    fn frames(c: &[Captured]) -> Vec<&Frame> {
        c.iter()
            .filter_map(|c| match c {
                Captured::Frame(f) => Some(f),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn streams_are_read_while_the_session_thread_is_busy() {
        let (mut cap, reads) = spawn();
        // The session thread is stuck in a long decode and takes nothing for 600 ms.
        std::thread::sleep(Duration::from_millis(600));
        let got = cap.take();
        let r = reads.lock().unwrap().clone();
        assert!(r.len() >= 15, "only {} reads in 600 ms", r.len());
        let worst = r.windows(2).map(|w| w[1] - w[0]).max().unwrap_or_default();
        assert!(
            worst < Duration::from_millis(150),
            "a read waited {worst:?}"
        );
        let n: usize = frames(&got).iter().map(|f| f.samples.len()).sum();
        assert!(n >= 16 * 450, "only {n} samples handed over");
        assert!(cap.device_name(Source::Mic) == Some("mic"));
        cap.stop();
    }

    #[test]
    fn sync_returns_everything_read_up_to_now() {
        let (mut cap, _) = spawn();
        std::thread::sleep(Duration::from_millis(100));
        let got = cap.sync();
        assert!(!frames(&got).is_empty());
        assert!(matches!(
            got.first(),
            Some(Captured::Device(Source::Mic, Some(_)))
        ));
        cap.stop();
    }

    #[test]
    fn restart_drops_audio_from_before_it() {
        let (mut cap, _) = spawn();
        std::thread::sleep(Duration::from_millis(100));
        let dropped = cap.restart();
        assert!(frames(&dropped).is_empty());
        std::thread::sleep(Duration::from_millis(100));
        let after = cap.take();
        let f = frames(&after);
        assert!(!f.is_empty());
        // Only the reopened stream (level 2) is heard after the restart.
        assert!(f
            .iter()
            .all(|f| f.samples.iter().all(|s| *s == 2.0 || *s == 0.0)));
        assert!(f.iter().any(|f| f.samples.contains(&2.0)));
        cap.stop();
    }

    #[test]
    fn a_disabled_source_stops_and_comes_back() {
        let (mut cap, _) = spawn();
        cap.set_enabled(Source::Mic, false);
        let _ = cap.sync();
        std::thread::sleep(Duration::from_millis(100));
        assert!(frames(&cap.take()).is_empty());
        cap.set_enabled(Source::Mic, true);
        // It reopens at the next device check, within a second (as before this thread).
        std::thread::sleep(Duration::from_millis(1_200));
        assert!(!frames(&cap.sync()).is_empty());
        cap.stop();
    }

    struct Boom;
    impl Backend for Boom {
        fn default_device(&self, _: Source) -> Option<String> {
            panic!("device call failed");
        }
        fn open(&mut self, _: Source, _: &str) -> Result<Box<dyn PcmStream>, AudioError> {
            Err(AudioError::Unavailable)
        }
    }

    #[test]
    fn a_capture_panic_reaches_the_session_thread() {
        let cap = CaptureThread::spawn(Box::new(|| Box::new(Boom)), Instant::now()).unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        while !cap.failed() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(cap.failed());
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cap.stop()));
        assert!(r.is_err());
    }
}

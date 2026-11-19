//! `sprkctl capture-check [--seconds N]`: opens the default mic and the default render device
//! (loopback), follows device changes, and prints sample counts, levels and events per source.
//! Audio is only measured in memory and discarded. Nothing is written to disk and no device
//! names are printed.
use anyhow::Result;
use sprk_audio::cpal_backend::CpalBackend;
use sprk_audio::live::{CaptureEvent, LiveCapture};
use sprk_audio::Source;
use std::time::{Duration, Instant};

pub fn run(rest: &[&str]) -> Result<()> {
    let seconds: u64 = rest
        .iter()
        .position(|a| *a == "--seconds")
        .and_then(|i| rest.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    // The capture is created on this thread and stays here (platform streams are not Send).
    let mut cap = LiveCapture::new(Box::new(CpalBackend::default()));
    let start = Instant::now();
    let (mut n, mut sq, mut peak) = ([0u64; 2], [0f64; 2], [0f32; 2]);
    while start.elapsed() < Duration::from_secs(seconds) {
        cap.poll(start.elapsed().as_millis() as u64);
        while let Some(f) = cap.next_frame() {
            let i = usize::from(f.source == Source::System);
            for s in &f.samples {
                n[i] += 1;
                sq[i] += f64::from(*s) * f64::from(*s);
                peak[i] = peak[i].max(s.abs());
            }
        }
        for e in cap.take_events() {
            let t = start.elapsed().as_secs();
            match e {
                CaptureEvent::DeviceChanged(s) => println!("t={t}s device changed: {s:?}"),
                CaptureEvent::Unavailable(s) => println!("t={t}s unavailable: {s:?}"),
                CaptureEvent::Restored(s) => println!("t={t}s restored: {s:?}"),
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    for (i, name) in ["mic", "system"].iter().enumerate() {
        let rms = if n[i] > 0 {
            (sq[i] / n[i] as f64).sqrt()
        } else {
            0.0
        };
        println!(
            "{name}: {} samples at 16 kHz ({:.1} s), rms {:.4}, peak {:.4}",
            n[i],
            n[i] as f64 / 16_000.0,
            rms,
            peak[i]
        );
    }
    Ok(())
}

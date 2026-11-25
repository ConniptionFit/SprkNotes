//! `sprkctl transcribe <model> --mic --vad <dir> [--seconds N] [--device NAME]`: live input from
//! the default (or named) capture device. Samples stay in memory: cpal callback -> lock-free
//! ring -> resample -> VAD -> engine. Nothing is written to disk. Only compiled with `--features live`.
use crate::engine::{fmt_ts, load_engine, load_vad, Args};
use anyhow::{bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::HeapRb;
use sprk_audio::Resampler;
use std::time::{Duration, Instant};

pub fn list_devices() -> Result<()> {
    let host = cpal::default_host();
    for d in host.input_devices()? {
        println!("{}", d.name().unwrap_or_else(|_| "?".into()));
    }
    Ok(())
}

fn opt(rest: &[String], flag: &str) -> Option<String> {
    rest.iter()
        .position(|a| a == flag)
        .and_then(|i| rest.get(i + 1).cloned())
}

pub fn transcribe_mic(a: &Args) -> Result<()> {
    let vad_dir = a
        .vad
        .as_deref()
        .context("--mic needs --vad <silero model folder>")?;
    let seconds: u64 = opt(&a.rest, "--seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let (m, mut engine) = load_engine(&a.model)?;
    let mut vad = load_vad(vad_dir)?;

    let host = cpal::default_host();
    let device = match opt(&a.rest, "--device") {
        Some(want) => host
            .input_devices()?
            .find(|d| d.name().is_ok_and(|n| n.contains(&want)))
            .with_context(|| format!("no input device containing '{want}' (see --list-devices)"))?,
        None => host
            .default_input_device()
            .context("no default input device")?,
    };
    let cfg = device.default_input_config()?;
    let (rate, channels) = (cfg.sample_rate().0, usize::from(cfg.channels()));
    eprintln!(
        "model {} {}; capturing {} s from a {} Hz x{} device",
        m.id, m.version, seconds, rate, channels
    );

    let (mut prod, mut cons) = HeapRb::<f32>::new(rate as usize * 10).split();
    let err_fn = |e| eprintln!("capture error: {e}");
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &cfg.config(),
            move |data: &[f32], _| {
                for f in data.chunks_exact(channels) {
                    let _ = prod.try_push(f.iter().sum::<f32>() / channels as f32);
                }
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            &cfg.config(),
            move |data: &[i16], _| {
                for f in data.chunks_exact(channels) {
                    let s: f32 = f.iter().map(|x| f32::from(*x) / 32768.0).sum();
                    let _ = prod.try_push(s / channels as f32);
                }
            },
            err_fn,
            None,
        )?,
        other => bail!("unsupported capture sample format {other:?}"),
    };
    stream.play()?;

    let mut resampler = Resampler::new(rate, 16_000);
    let start = Instant::now();
    let mut buf = vec![0.0f32; 4096];
    while start.elapsed() < Duration::from_secs(seconds) {
        let n = cons.pop_slice(&mut buf);
        if n == 0 {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        for u in vad.accept(&resampler.process(&buf[..n])) {
            for s in sprk_asr::transcribe_utterance(&mut *engine, &u)? {
                println!("[{}] {}", fmt_ts(s.start_ms), s.text);
            }
        }
    }
    drop(stream);
    for u in vad.flush() {
        for s in sprk_asr::transcribe_utterance(&mut *engine, &u)? {
            println!("[{}] {}", fmt_ts(s.start_ms), s.text);
        }
    }
    Ok(())
}

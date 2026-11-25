//! `sprkctl transcribe` and `sprkctl bench` (SPEC 14 M1). Only compiled with `--features engine`.
//! Models are always loaded from a folder through its verified `model.json`; no model name or
//! path is built in. WAV files are read, never written.
use anyhow::{bail, Context, Result};
use sprk_asr::{wav, wer, Engine, Registry, Segment, Vad};
use sprk_models::Manifest;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Offline models have a hard input limit (Whisper: 30 s). Longer audio needs a VAD.
const MAX_UNSEGMENTED_S: f64 = 28.0;
/// VAD caps utterances a little under that limit.
const VAD_MAX_SPEECH_S: f32 = 20.0;

pub struct Args {
    pub model: PathBuf,
    pub vad: Option<PathBuf>,
    pub rest: Vec<String>,
}

pub fn load_manifest(dir: &Path) -> Result<Manifest> {
    let text = std::fs::read_to_string(dir.join("model.json"))
        .with_context(|| format!("reading model.json in {}", dir.display()))?;
    Ok(Manifest::from_json(&text)?)
}

pub fn load_engine(dir: &Path) -> Result<(Manifest, Box<dyn Engine>)> {
    let manifest = load_manifest(dir)?;
    let mut reg = Registry::default();
    sprk_asr::sherpa::register(&mut reg);
    let engine = reg.load(&manifest, dir, env!("CARGO_PKG_VERSION"))?;
    Ok((manifest, engine))
}

pub fn load_vad(dir: &Path) -> Result<Box<dyn Vad>> {
    let m = load_manifest(dir)?;
    Ok(Box::new(sprk_asr::sherpa::SileroVad::load(
        &m,
        dir,
        VAD_MAX_SPEECH_S,
    )?))
}

pub fn fmt_ts(ms: u64) -> String {
    format!("{:02}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

/// Decode a 16 kHz buffer, with VAD segmentation when a VAD is supplied.
pub fn decode(
    engine: &mut dyn Engine,
    vad: Option<&mut Box<dyn Vad>>,
    samples: &[f32],
) -> Result<Vec<Segment>> {
    let secs = samples.len() as f64 / 16_000.0;
    Ok(match vad {
        Some(v) => sprk_asr::transcribe_with_vad(engine, &mut **v, samples)?,
        None if secs > MAX_UNSEGMENTED_S => {
            bail!("audio is {secs:.0} s; pass --vad <silero model folder> to segment it")
        }
        None => engine.transcribe(samples)?,
    })
}

pub fn transcribe_file(a: &Args) -> Result<()> {
    let wav_path = a.rest.first().context("usage: transcribe <model> <wav>")?;
    let (m, mut engine) = load_engine(&a.model)?;
    let mut vad = a.vad.as_deref().map(load_vad).transpose()?;
    let audio = wav::read_file(Path::new(wav_path))?;
    let samples = sprk_audio::Resampler::convert(audio.sample_rate, 16_000, &audio.samples);
    eprintln!("model {} {} ({})", m.id, m.version, m.engine);
    let t = Instant::now();
    let segs = decode(&mut *engine, vad.as_mut(), &samples)?;
    let took = t.elapsed().as_secs_f64();
    for s in &segs {
        println!("[{}] {}", fmt_ts(s.start_ms), s.text);
    }
    let audio_s = samples.len() as f64 / 16_000.0;
    eprintln!(
        "audio {audio_s:.1} s, decode {took:.2} s, RTF {:.3} (dev host: informational, not a gate)",
        took / audio_s.max(0.001)
    );
    Ok(())
}

/// Collect (wav path, reference text) pairs from a fixtures folder: either `<name>.txt` next
/// to each `<name>.wav`, or a `trans.txt` with lines `<name>.wav <text>`.
fn fixtures(dir: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut out = Vec::new();
    let trans = dir.join("trans.txt");
    let listed: Vec<(String, String)> = std::fs::read_to_string(&trans)
        .map(|t| {
            t.lines()
                .filter_map(|l| {
                    let (n, txt) = l.split_once(char::is_whitespace)?;
                    Some((n.to_owned(), txt.trim().to_owned()))
                })
                .collect()
        })
        .unwrap_or_default();
    for (name, text) in listed {
        let name = if name.ends_with(".wav") {
            name
        } else {
            format!("{name}.wav")
        };
        out.push((dir.join(name), text));
    }
    if out.is_empty() {
        let mut wavs: Vec<PathBuf> = std::fs::read_dir(dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "wav"))
            .collect();
        wavs.sort();
        for w in wavs {
            if let Ok(t) = std::fs::read_to_string(w.with_extension("txt")) {
                out.push((w, t.trim().to_owned()));
            }
        }
    }
    if out.is_empty() {
        bail!(
            "no fixtures found in {} (need trans.txt or <name>.txt beside <name>.wav)",
            dir.display()
        );
    }
    Ok(out)
}

pub fn bench_accuracy(a: &Args) -> Result<()> {
    let dir = a
        .rest
        .first()
        .context("usage: bench accuracy <model> <fixtures-dir>")?;
    let (m, mut engine) = load_engine(&a.model)?;
    let mut vad = a.vad.as_deref().map(load_vad).transpose()?;
    let mut total = wer::Wer::default();
    let (mut audio_s, mut decode_s) = (0.0f64, 0.0f64);
    println!("model {} {} ({})", m.id, m.version, m.engine);
    for (path, reference) in fixtures(Path::new(dir))? {
        let audio = wav::read_file(&path)?;
        let samples = sprk_audio::Resampler::convert(audio.sample_rate, 16_000, &audio.samples);
        let t = Instant::now();
        let segs = decode(&mut *engine, vad.as_mut(), &samples)?;
        decode_s += t.elapsed().as_secs_f64();
        audio_s += samples.len() as f64 / 16_000.0;
        let hyp = segs
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let w = wer::wer(&reference, &hyp);
        total.add(&w);
        println!(
            "{:<14} WER {:5.1}%  ({} errors / {} words)",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
            w.rate() * 100.0,
            w.errors(),
            w.ref_words
        );
    }
    println!(
        "TOTAL          WER {:5.1}%  (S {} D {} I {} / {} words)",
        total.rate() * 100.0,
        total.substitutions,
        total.deletions,
        total.insertions,
        total.ref_words
    );
    println!(
        "speed          RTF {:.3}  ({decode_s:.1} s decode for {audio_s:.1} s audio; dev host, informational only)",
        decode_s / audio_s.max(0.001)
    );
    Ok(())
}

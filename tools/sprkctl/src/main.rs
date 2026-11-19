//! sprkctl: model pack admin CLI (SPEC 16.7). `pack`, `list`, `import` etc. arrive in M10;
//! `verify` and `inspect` work on a model folder now. With `--features engine`, `transcribe`
//! and `bench accuracy` run a model folder through its engine (M1).
use anyhow::{bail, Context, Result};
use sprk_models::Manifest;
use std::path::Path;

#[cfg(feature = "engine")]
mod engine;
#[cfg(feature = "live")]
mod live;

fn load(dir: &Path) -> Result<Manifest> {
    let text = std::fs::read_to_string(dir.join("model.json"))
        .with_context(|| format!("reading model.json in {}", dir.display()))?;
    Ok(Manifest::from_json(&text)?)
}

#[cfg(feature = "engine")]
fn engine_args(args: &[String]) -> Result<engine::Args> {
    // args: <model-dir> [positional...] [--vad <dir>] [flags...]
    let model = args.first().context("missing <model-folder>")?.into();
    let mut vad = None;
    let mut rest = Vec::new();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        if a == "--vad" {
            vad = Some(it.next().context("--vad needs a folder")?.into());
        } else {
            rest.push(a.clone());
        }
    }
    Ok(engine::Args { model, vad, rest })
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["model", "inspect", dir] => {
            let m = load(Path::new(dir))?;
            println!("{}", serde_json::to_string_pretty(&m)?);
        }
        ["model", "verify", dir] => {
            let dir = Path::new(dir);
            load(dir)?.verify_files(dir)?;
            println!("ok");
        }
        #[cfg(feature = "engine")]
        ["transcribe", ..] => {
            let a = engine_args(&args[1..])?;
            #[cfg(feature = "live")]
            if a.rest.iter().any(|r| r == "--mic") {
                return live::transcribe_mic(&a);
            }
            engine::transcribe_file(&a)?;
        }
        #[cfg(feature = "live")]
        ["devices"] => live::list_devices()?,
        #[cfg(feature = "engine")]
        ["bench", "accuracy", ..] => engine::bench_accuracy(&engine_args(&args[2..])?)?,
        [] | ["--version"] => println!("sprkctl {}", env!("CARGO_PKG_VERSION")),
        _ => bail!(
            "usage: sprkctl model <inspect|verify> <model-folder>\n       \
             sprkctl transcribe <model-folder> <wav> [--vad <vad-folder>]    (--features engine)\n       \
             sprkctl transcribe <model-folder> --mic --vad <vad-folder> [--seconds N] [--device NAME]  (--features live)\n       \
             sprkctl devices                                                  (--features live)\n       \
             sprkctl bench accuracy <model-folder> <fixtures-dir> [--vad <vad-folder>]  (--features engine)"
        ),
    }
    Ok(())
}

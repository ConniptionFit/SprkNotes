//! sprkctl: model pack admin CLI (SPEC 16.7). `pack`, `list`, `import` etc. arrive in M10;
//! `verify` and `inspect` work on a model folder now. With `--features engine`, `transcribe`
//! and `bench accuracy` run a model folder through its engine (M1).
use anyhow::{bail, Context, Result};
use sprk_models::Manifest;
use std::path::Path;

#[cfg(feature = "live")]
mod capture_check;
#[cfg(any(feature = "engine", feature = "engine-shared"))]
mod engine;
#[cfg(all(feature = "live", any(feature = "engine", feature = "engine-shared")))]
mod live;
mod models_cmd;
#[cfg(all(feature = "live", windows))]
mod sources_check;

fn load(dir: &Path) -> Result<Manifest> {
    let text = std::fs::read_to_string(dir.join("model.json"))
        .with_context(|| format!("reading model.json in {}", dir.display()))?;
    Ok(Manifest::from_json(&text)?)
}

#[cfg(any(feature = "engine", feature = "engine-shared"))]
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
        ["model", rest @ ..] if !matches!(rest.first(), Some(&"inspect")) => {
            std::process::exit(models_cmd::run(rest)?);
        }
        ["model", "inspect", dir] => {
            let m = load(Path::new(dir))?;
            println!("{}", serde_json::to_string_pretty(&m)?);
        }
        #[cfg(any(feature = "engine", feature = "engine-shared"))]
        ["transcribe", ..] => {
            let a = engine_args(&args[1..])?;
            #[cfg(all(feature = "live", any(feature = "engine", feature = "engine-shared")))]
            if a.rest.iter().any(|r| r == "--mic") {
                return live::transcribe_mic(&a);
            }
            engine::transcribe_file(&a)?;
        }
        #[cfg(all(feature = "live", any(feature = "engine", feature = "engine-shared")))]
        ["devices"] => live::list_devices()?,
        #[cfg(any(feature = "engine", feature = "engine-shared"))]
        ["bench", "accuracy", ..] => engine::bench_accuracy(&engine_args(&args[2..])?)?,
        #[cfg(feature = "live")]
        ["capture-check", rest @ ..] => capture_check::run(rest)?,
        #[cfg(all(feature = "live", windows))]
        ["sources-check", rest @ ..] => sources_check::run(rest)?,
        [] | ["--version"] => println!("sprkctl {}", env!("CARGO_PKG_VERSION")),
        _ => bail!(
            "usage: sprkctl model inspect <model-folder>\n       \
             sprkctl model <pack|verify|hash|install|import|list|activate|rollback|remove> ...   (SPEC 16.7)\n       \
             sprkctl transcribe <model-folder> <wav> [--vad <vad-folder>]    (--features engine)\n       \
             sprkctl transcribe <model-folder> --mic --vad <vad-folder> [--seconds N] [--device NAME]  (--features live)\n       \
             sprkctl capture-check [--seconds N]                              (--features live)\n       \
             sprkctl sources-check [--seconds N]                              (--features live, Windows)\n       \
             sprkctl devices                                                  (--features live)\n       \
             sprkctl bench accuracy <model-folder> <fixtures-dir> [--vad <vad-folder>]  (--features engine)"
        ),
    }
    Ok(())
}

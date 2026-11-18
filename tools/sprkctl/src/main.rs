//! sprkctl: model pack admin CLI (SPEC 16.7). `pack`, `list`, `import` etc. arrive in M10;
//! `verify` and `inspect` work on a model folder now so M1 can validate packs.
use anyhow::{bail, Context, Result};
use sprk_models::Manifest;
use std::path::Path;

fn load(dir: &Path) -> Result<Manifest> {
    let text = std::fs::read_to_string(dir.join("model.json"))
        .with_context(|| format!("reading model.json in {}", dir.display()))?;
    Ok(Manifest::from_json(&text)?)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["model", "inspect", dir] => {
            let m = load(Path::new(dir))?;
            println!("{}", serde_json::to_string_pretty(&m)?);
        }
        ["model", "verify", dir] => {
            let dir = Path::new(dir);
            load(dir)?.verify_files(dir)?;
            println!("ok");
        }
        [] | ["--version"] => println!("sprkctl {}", env!("CARGO_PKG_VERSION")),
        _ => bail!("usage: sprkctl model <inspect|verify> <model-folder>"),
    }
    Ok(())
}

//! sprkctl: model pack admin CLI. Subcommands arrive in M10.
fn main() -> anyhow::Result<()> {
    println!("sprkctl {}", env!("CARGO_PKG_VERSION"));
    Ok(())
}

//! Tiny launcher for the portable zip: `SprkNotes.exe` next to `x64\` and `arm64\` folders.
//! Starts the native build for the PC (this launcher itself is x64 and runs emulated on ARM64),
//! passing its arguments through, and creates `portable.flag` beside the app so settings stay in the folder.
use std::path::PathBuf;
use std::process::{Command, ExitCode};

/// ARM64 Windows reports the real architecture in PROCESSOR_ARCHITEW6432 for emulated x64 processes.
fn native_arch(w6432: Option<&str>, arch: Option<&str>) -> &'static str {
    let real = w6432.or(arch).unwrap_or("AMD64");
    if real.eq_ignore_ascii_case("ARM64") {
        "arm64"
    } else {
        "x64"
    }
}

fn main() -> ExitCode {
    if std::env::args().any(|a| a == "--version") {
        println!("SprkNotes launcher {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let dir: PathBuf = match std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(PathBuf::from))
    {
        Some(d) => d,
        None => return ExitCode::from(2),
    };
    let arch = native_arch(
        std::env::var("PROCESSOR_ARCHITEW6432").ok().as_deref(),
        std::env::var("PROCESSOR_ARCHITECTURE").ok().as_deref(),
    );
    let app_dir = dir.join(arch);
    let _ = std::fs::write(app_dir.join("portable.flag"), b"");
    match Command::new(app_dir.join("SprkNotes.exe"))
        .args(std::env::args().skip(1))
        .current_dir(&app_dir)
        .spawn()
    {
        Ok(_) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_native_arch() {
        assert_eq!(native_arch(Some("ARM64"), Some("AMD64")), "arm64");
        assert_eq!(native_arch(None, Some("ARM64")), "arm64");
        assert_eq!(native_arch(None, Some("AMD64")), "x64");
        assert_eq!(native_arch(None, None), "x64");
    }
}

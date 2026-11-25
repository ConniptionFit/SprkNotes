//! Linux (and other Unix) [`Launcher`]: `xdg-open` for links, a clipboard tool for text, and the
//! file manager for "show in folder". SprkNotes makes no request itself. When no clipboard tool
//! is installed the text is handed back in `uncopied` so the window can copy it instead.
use crate::Launcher;
use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Default)]
pub struct ShellLauncher {
    pub toasts: Vec<String>,
    /// Text the OS clipboard tools could not take; the UI copies it.
    pub uncopied: Option<String>,
}

fn run_with_stdin(cmd: &str, args: &[&str], text: &str) -> bool {
    let Ok(mut child) = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut si) = child.stdin.take() {
        if si.write_all(text.as_bytes()).is_err() {
            return false;
        }
    }
    // wl-copy and xclip stay alive to serve the paste; do not wait for them.
    std::thread::sleep(std::time::Duration::from_millis(150));
    match child.try_wait() {
        Ok(Some(st)) => st.success(),
        Ok(None) => true,
        Err(_) => false,
    }
}

impl Launcher for ShellLauncher {
    fn open_url(&mut self, url: &str) -> bool {
        Command::new("xdg-open")
            .arg(url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Without a clipboard tool the window copies the text, so this still counts as copied.
    fn copy(&mut self, text: &str) -> bool {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        let ok = (wayland && run_with_stdin("wl-copy", &[], text))
            || run_with_stdin("xclip", &["-selection", "clipboard"], text)
            || run_with_stdin("xsel", &["--clipboard", "--input"], text)
            || (!wayland && run_with_stdin("wl-copy", &[], text));
        if !ok {
            self.uncopied = Some(text.to_string());
        }
        true
    }

    fn reveal(&mut self, path: &str) {
        // Most Linux file managers cannot highlight a file from a plain command, so open its folder.
        let dir = std::path::Path::new(path)
            .parent()
            .map_or_else(|| path.to_string(), |p| p.to_string_lossy().into_owned());
        let _ = Command::new("xdg-open")
            .arg(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }

    fn toast(&mut self, text: &str) {
        self.toasts.push(text.to_string());
    }
}

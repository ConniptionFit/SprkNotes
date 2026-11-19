//! Windows [`Launcher`]: opens a link with the shell, copies text to the clipboard and reveals
//! a file in Explorer. SprkNotes makes no request itself; the registered app (or the user's
//! browser) handles the link. Toasts are collected for the UI to show.
#![allow(unsafe_code)]
use crate::Launcher;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// `CF_UNICODETEXT`
const CF_UNICODETEXT: u32 = 13;

#[derive(Default)]
pub struct ShellLauncher {
    /// Toasts to display, oldest first. The app drains this after `run`.
    pub toasts: Vec<String>,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set_clipboard(text: &str) -> bool {
    let data = wide(text);
    let bytes = data.len() * 2;
    // SAFETY: standard clipboard sequence. The global block is allocated movable, locked only
    // while copying `bytes` bytes into it (its size), then unlocked and handed to the clipboard,
    // which owns it after a successful SetClipboardData. The clipboard is always closed.
    unsafe {
        if OpenClipboard(HWND::default()).is_err() {
            return false;
        }
        let ok = (|| {
            EmptyClipboard().ok()?;
            let h = GlobalAlloc(GMEM_MOVEABLE, bytes).ok()?;
            let p = GlobalLock(h);
            if p.is_null() {
                return None;
            }
            std::ptr::copy_nonoverlapping(data.as_ptr().cast::<u8>(), p.cast::<u8>(), bytes);
            let _ = GlobalUnlock(h);
            SetClipboardData(CF_UNICODETEXT, HANDLE(h.0)).ok()?;
            Some(())
        })()
        .is_some();
        let _ = CloseClipboard();
        ok
    }
}

impl Launcher for ShellLauncher {
    fn open_url(&mut self, url: &str) -> bool {
        let u = wide(url);
        // SAFETY: `u` is NUL-terminated and outlives the call; the other pointers are null or static.
        // ShellExecuteW returns a value greater than 32 on success.
        let r = unsafe {
            ShellExecuteW(
                HWND::default(),
                w!("open"),
                PCWSTR(u.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        r.0 as isize > 32
    }

    fn copy(&mut self, text: &str) {
        let _ = set_clipboard(text);
    }

    fn reveal(&mut self, path: &str) {
        // `explorer /select,<path>` highlights the file. Arguments are passed as one string so a
        // path with spaces stays together; nothing here goes through a shell.
        let _ = std::process::Command::new("explorer.exe")
            .arg(format!("/select,{path}"))
            .spawn();
    }

    fn toast(&mut self, text: &str) {
        self.toasts.push(text.to_string());
    }
}

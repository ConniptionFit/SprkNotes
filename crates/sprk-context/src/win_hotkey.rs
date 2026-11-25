//! Windows: the background shortcut (SPEC 8.6), registered with RegisterHotKey on a thread of
//! its own that waits for WM_HOTKEY. Changing or removing the shortcut ends that thread, which
//! unregisters the key, and starts a new one. Nothing is logged.
#![allow(unsafe_code)]
use crate::hotkey::{Combo, GlobalHotkey, HotkeyError};
use std::sync::mpsc;
use std::thread::JoinHandle;
use windows::Win32::Foundation::{ERROR_HOTKEY_ALREADY_REGISTERED, HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetMessageW, PeekMessageW, PostThreadMessageW, MSG, PM_NOREMOVE, WM_HOTKEY, WM_QUIT,
};

const ID: i32 = 1;

/// The registered shortcut: the thread that holds it, by id, so it can be told to stop.
#[derive(Default)]
pub struct WinHotkey {
    thread: Option<(u32, JoinHandle<()>)>,
}

impl WinHotkey {
    fn stop(&mut self) {
        if let Some((tid, handle)) = self.thread.take() {
            // SAFETY: posting a message to a thread id has no memory effects; a thread that has
            // already ended makes the call fail, which is fine.
            unsafe {
                let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            let _ = handle.join();
        }
    }
}

impl Drop for WinHotkey {
    fn drop(&mut self) {
        self.stop();
    }
}

impl GlobalHotkey for WinHotkey {
    fn set(
        &mut self,
        combo: Option<&Combo>,
        on_press: Box<dyn Fn() + Send + 'static>,
    ) -> Result<(), HotkeyError> {
        self.stop();
        let Some(c) = combo else {
            return Ok(());
        };
        let (mods, vk) = (c.mods(), c.vk());
        let (tx, rx) = mpsc::channel::<(u32, Result<(), HotkeyError>)>();
        let handle = std::thread::Builder::new()
            .name("hotkey".into())
            .spawn(move || {
                // SAFETY: plain Win32 calls on this thread's own message queue; `msg` is a local
                // MSG that GetMessageW and PeekMessageW write into.
                unsafe {
                    let tid = GetCurrentThreadId();
                    let mut msg = MSG::default();
                    // Create the queue before anyone can post WM_QUIT to it.
                    let _ = PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_NOREMOVE);
                    let r = RegisterHotKey(HWND::default(), ID, HOT_KEY_MODIFIERS(mods), vk);
                    let r = r.map_err(|e| {
                        if e.code() == ERROR_HOTKEY_ALREADY_REGISTERED.to_hresult() {
                            HotkeyError::Taken
                        } else {
                            HotkeyError::Failed
                        }
                    });
                    let ok = r.is_ok();
                    let _ = tx.send((tid, r));
                    if !ok {
                        return;
                    }
                    while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
                        if msg.message == WM_HOTKEY && msg.wParam.0 == ID as usize {
                            on_press();
                        }
                    }
                    let _ = UnregisterHotKey(HWND::default(), ID);
                }
            })
            .map_err(|_| HotkeyError::Failed)?;
        match rx.recv() {
            Ok((tid, Ok(()))) => {
                self.thread = Some((tid, handle));
                Ok(())
            }
            Ok((_, Err(e))) => {
                let _ = handle.join();
                Err(e)
            }
            Err(_) => {
                let _ = handle.join();
                Err(HotkeyError::Failed)
            }
        }
    }
}

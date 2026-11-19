//! Windows implementations of the read-only inspection seams: window enumeration and audio
//! session metering. Nothing here injects input, clicks or changes another app. Titles are only
//! returned to the caller; they are never logged.
#![allow(unsafe_code)]
use crate::sources::{AudioSession, AudioSessionSource};
use crate::{Window, WindowEnumerator};
use windows::core::{Interface, PWSTR};
use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, MAX_PATH};
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{
    eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume,
    MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
};

/// Executable stem (`firefox`) for a process id, if it can be queried.
pub fn exe_stem(pid: u32) -> Option<String> {
    // SAFETY: limited-query handle, closed before returning; the buffer is MAX_PATH wide chars
    // and its length is passed in `len`, which the call updates.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; MAX_PATH as usize];
        let mut len = buf.len() as u32;
        let ok =
            QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
                .is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let file = path.rsplit(['\\', '/']).next()?;
        Some(file.strip_suffix(".exe").unwrap_or(file).to_string())
    }
}

/// Parent process id table, one snapshot.
fn parents() -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    // SAFETY: snapshot handle closed at the end; `entry.dwSize` is set as the API requires.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut e = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                out.push((e.th32ProcessID, e.th32ParentProcessID));
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// Browsers play audio from helper processes: walk up while the parent is the same program.
pub fn owner_pid(pid: u32) -> u32 {
    let table = parents();
    let me = exe_stem(pid);
    let mut cur = pid;
    for _ in 0..8 {
        let Some(&(_, parent)) = table.iter().find(|(p, _)| *p == cur) else {
            break;
        };
        if parent != 0 && me.is_some() && exe_stem(parent) == me {
            cur = parent;
        } else {
            break;
        }
    }
    cur
}

pub struct Windows;

impl WindowEnumerator for Windows {
    fn windows(&self) -> Vec<Window> {
        unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
            // SAFETY: `lparam` is the pointer to the Vec we passed to EnumWindows below, which
            // outlives the enumeration; the callback runs on the calling thread.
            let out = unsafe { &mut *(lparam.0 as *mut Vec<Window>) };
            if unsafe { IsWindowVisible(hwnd) }.as_bool() {
                let mut buf = [0u16; 512];
                let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
                if n > 0 {
                    let mut pid = 0u32;
                    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
                    out.push(Window {
                        pid,
                        title: String::from_utf16_lossy(&buf[..n as usize]),
                    });
                }
            }
            BOOL(1)
        }
        let mut out: Vec<Window> = Vec::new();
        // SAFETY: see `each`; the Vec lives until EnumWindows returns.
        unsafe {
            let _ = EnumWindows(Some(each), LPARAM(&mut out as *mut Vec<Window> as isize));
        }
        out
    }
}

/// Audio sessions on every active render endpoint, with peak and mute state.
pub struct WasapiSessions {
    self_pid: u32,
}

impl Default for WasapiSessions {
    fn default() -> Self {
        Self {
            self_pid: std::process::id(),
        }
    }
}

impl WasapiSessions {
    fn read(&self) -> Option<Vec<AudioSession>> {
        // SAFETY: COM is initialized for this thread (an already-initialized apartment returns
        // an error we ignore). All interface pointers are owned `windows` wrappers released on drop.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let en: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let devices = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE).ok()?;
            let mut out = Vec::new();
            for d in 0..devices.GetCount().ok()? {
                let Ok(dev) = devices.Item(d) else { continue };
                let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                    continue;
                };
                let Ok(list) = mgr.GetSessionEnumerator() else {
                    continue;
                };
                for i in 0..list.GetCount().unwrap_or(0) {
                    let Ok(ctl) = list.GetSession(i) else {
                        continue;
                    };
                    let Ok(c2) = ctl.cast::<IAudioSessionControl2>() else {
                        continue;
                    };
                    let system = c2.IsSystemSoundsSession().0 == 0;
                    let pid = c2.GetProcessId().unwrap_or(0);
                    let peak = ctl
                        .cast::<IAudioMeterInformation>()
                        .ok()
                        .and_then(|m| m.GetPeakValue().ok())
                        .unwrap_or(0.0);
                    let muted = ctl
                        .cast::<ISimpleAudioVolume>()
                        .ok()
                        .and_then(|v| v.GetMute().ok())
                        .is_some_and(|b| b.as_bool());
                    let owner = if pid == 0 { 0 } else { owner_pid(pid) };
                    let exe = exe_stem(owner).unwrap_or_default();
                    let ignorable = system || pid == 0 || owner == self.self_pid || exe.is_empty();
                    out.push(AudioSession {
                        owner_pid: owner,
                        exe,
                        peak,
                        muted,
                        ignorable,
                    });
                }
            }
            Some(out)
        }
    }
}

impl AudioSessionSource for WasapiSessions {
    fn sessions(&self) -> Vec<AudioSession> {
        self.read().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_process_has_an_exe_stem_and_is_its_own_owner() {
        let me = std::process::id();
        assert!(exe_stem(me).is_some());
        assert!(owner_pid(me) != 0);
    }

    #[test]
    fn enumerators_never_panic_without_a_desktop() {
        let _ = Windows.windows();
        let _ = WasapiSessions::default().sessions();
    }
}

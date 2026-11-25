//! Windows: is another app using a microphone? Reads the per-user privacy data Windows keeps
//! for the microphone (`CapabilityAccessManager\ConsentStore\microphone`, the same data behind the
//! microphone icon in the taskbar). Read-only registry access; nothing is opened, changed or
//! logged; app ids go only to the prompt.
#![allow(unsafe_code)]
use crate::mic_use::{app_id, entry_in_use, filetime_from_unix, MicUse, MAX_USE_SECS};
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    RRF_RT_REG_QWORD,
};

const STORE: PCWSTR =
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone");

/// Reads the microphone privacy data. Entries for SprkNotes itself are skipped.
pub struct RegistryMicUse {
    /// This executable's path in the form the privacy data uses (`C:#Program Files#...#SprkNotes.exe`), lower case.
    own: Option<String>,
}

impl Default for RegistryMicUse {
    fn default() -> Self {
        let own = std::env::current_exe()
            .ok()
            .map(|p| p.to_string_lossy().replace('\\', "#").to_lowercase());
        Self { own }
    }
}

impl RegistryMicUse {
    fn is_own(&self, name: &str) -> bool {
        let n = name.to_lowercase();
        // Packaged (MSIX) entries are named by package family; unpackaged ones by path.
        n.contains("sprknotes") || self.own.as_deref() == Some(n.as_str())
    }
}

/// Owned registry key, closed on drop.
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful RegOpenKeyExW and is closed exactly once.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn open(parent: HKEY, sub: PCWSTR) -> Option<Key> {
    let mut key = HKEY::default();
    // SAFETY: `sub` is a NUL-terminated wide string that outlives the call; `key` is written on
    // success and wrapped so it is closed.
    let r = unsafe { RegOpenKeyExW(parent, sub, 0, KEY_READ, &mut key) };
    (r == ERROR_SUCCESS).then_some(Key(key))
}

/// Names of the direct subkeys of `key`.
fn subkeys(key: &Key) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0.. {
        // Registry key names are at most 255 characters.
        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        // SAFETY: `buf` holds `len` wide chars; the call writes at most that many and updates
        // `len` to the name's length. Unused optional outputs are None.
        let r = unsafe {
            RegEnumKeyExW(
                key.0,
                i,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
                None,
                PWSTR::null(),
                None,
                None,
            )
        };
        if r != ERROR_SUCCESS {
            break;
        }
        out.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    out
}

fn qword(key: &Key, sub: &str, value: PCWSTR) -> u64 {
    let sub: Vec<u16> = sub.encode_utf16().chain(std::iter::once(0)).collect();
    let mut data = 0u64;
    let mut size = std::mem::size_of::<u64>() as u32;
    // SAFETY: `sub` is NUL-terminated; `data` is a u64 and `size` says so; RRF_RT_REG_QWORD makes
    // the call fail rather than write anything that is not exactly a QWORD.
    let r = unsafe {
        RegGetValueW(
            key.0,
            PCWSTR(sub.as_ptr()),
            value,
            RRF_RT_REG_QWORD,
            None,
            Some((&mut data as *mut u64).cast()),
            Some(&mut size),
        )
    };
    if r == ERROR_SUCCESS {
        data
    } else {
        0
    }
}

fn in_use(key: &Key, now: u64, packaged: bool, skip: &dyn Fn(&str) -> bool, out: &mut Vec<String>) {
    for name in subkeys(key) {
        if name != "NonPackaged"
            && !skip(&name)
            && entry_in_use(
                qword(key, &name, w!("LastUsedTimeStart")),
                qword(key, &name, w!("LastUsedTimeStop")),
                now,
                MAX_USE_SECS,
            )
        {
            let id = app_id(&name, packaged);
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
}

impl MicUse for RegistryMicUse {
    fn apps_in_use(&self) -> Vec<String> {
        let mut out = Vec::new();
        let Some(store) = open(HKEY_CURRENT_USER, STORE) else {
            return out;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| filetime_from_unix(d.as_secs()))
            .unwrap_or(0);
        let skip = |n: &str| self.is_own(n);
        in_use(&store, now, true, &skip, &mut out);
        if let Some(np) = open(store.0, w!("NonPackaged")) {
            in_use(&np, now, false, &skip, &mut out);
        }
        out
    }
}

//! Policy values from `HKLM\Software\Policies\SprkNotes` and `HKCU\Software\Policies\SprkNotes`.
//! Read-only; a missing key or value is simply "not set".
use crate::{Layered, PolicyStore};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
    KEY_READ, REG_DWORD, REG_MULTI_SZ, REG_SZ, REG_VALUE_TYPE,
};

pub struct RegistryPolicy {
    machine: bool,
    /// Installer defaults (`Software\SprkNotes\Defaults`) instead of policy. Defaults never lock anything.
    defaults: bool,
}

impl RegistryPolicy {
    pub fn machine() -> Self {
        Self {
            machine: true,
            defaults: false,
        }
    }
    pub fn user() -> Self {
        Self {
            machine: false,
            defaults: false,
        }
    }
    /// Machine policy first, then user policy.
    pub fn layered() -> Layered<RegistryPolicy, RegistryPolicy> {
        Layered(Self::machine(), Self::user())
    }

    /// Installer defaults: machine first, then user. Written by the MSI switches (`UPDATE_CHANNEL`
    /// and friends); the user can still change what they allow.
    pub fn defaults_layered() -> Layered<RegistryPolicy, RegistryPolicy> {
        Layered(
            Self {
                machine: true,
                defaults: true,
            },
            Self {
                machine: false,
                defaults: true,
            },
        )
    }

    fn raw(&self, name: &str) -> Option<(REG_VALUE_TYPE, Vec<u8>)> {
        let hive = if self.machine {
            HKEY_LOCAL_MACHINE
        } else {
            HKEY_CURRENT_USER
        };
        let subkey = if self.defaults {
            w!("Software\\SprkNotes\\Defaults")
        } else {
            w!("Software\\Policies\\SprkNotes")
        };
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let mut key = HKEY::default();
        // SAFETY: the subkey literal and `key` outlive the call; the handle is closed below.
        let opened = unsafe { RegOpenKeyExW(hive, subkey, 0, KEY_READ, &mut key) };
        if opened != ERROR_SUCCESS {
            return None;
        }
        let mut kind = REG_VALUE_TYPE(0);
        let mut len = 0u32;
        // SAFETY: `wide` is NUL-terminated; a null data pointer asks only for type and size.
        let probe = unsafe {
            RegQueryValueExW(
                key,
                PCWSTR(wide.as_ptr()),
                None,
                Some(&mut kind),
                None,
                Some(&mut len),
            )
        };
        let out = if probe == ERROR_SUCCESS && len <= 64 * 1024 {
            let mut buf = vec![0u8; len as usize];
            // SAFETY: `buf` is `len` bytes, the size the first call reported.
            let got = unsafe {
                RegQueryValueExW(
                    key,
                    PCWSTR(wide.as_ptr()),
                    None,
                    Some(&mut kind),
                    Some(buf.as_mut_ptr()),
                    Some(&mut len),
                )
            };
            (got == ERROR_SUCCESS).then(|| {
                buf.truncate(len as usize);
                (kind, buf)
            })
        } else {
            None
        };
        // SAFETY: `key` was opened above and is not used after this.
        unsafe {
            let _ = RegCloseKey(key);
        }
        out
    }
}

fn utf16(bytes: &[u8]) -> Vec<u16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect()
}

impl PolicyStore for RegistryPolicy {
    fn get(&self, key: &str) -> Option<String> {
        let (kind, bytes) = self.raw(key)?;
        if kind == REG_DWORD && bytes.len() >= 4 {
            return Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]).to_string());
        }
        if kind == REG_SZ {
            let w = utf16(&bytes);
            let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
            return Some(String::from_utf16_lossy(&w[..end]));
        }
        None
    }

    fn get_multi(&self, key: &str) -> Vec<String> {
        match self.raw(key) {
            Some((kind, bytes)) if kind == REG_MULTI_SZ => utf16(&bytes)
                .split(|&c| c == 0)
                .map(String::from_utf16_lossy)
                .filter(|s| !s.is_empty())
                .collect(),
            _ => self.get(key).into_iter().collect(),
        }
    }
}

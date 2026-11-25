//! Reads the owner and access list of a file or folder for [`crate::trust`].
use crate::trust::{Ace, SecurityInfo, SecurityReader};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{LocalFree, ERROR_SUCCESS, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT,
};
use windows::Win32::Security::{
    GetAce, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
};

const ACCESS_ALLOWED: u8 = 0;
/// Deny entries only take rights away.
const DENY_TYPES: [u8; 4] = [1, 6, 10, 12];
const INHERIT_ONLY: u8 = 0x08;

pub struct WindowsSecurity;

fn sid_text(sid: PSID) -> Option<String> {
    let mut s = PWSTR::null();
    // SAFETY: `sid` points into a security descriptor that is alive for this call; the string
    // the system allocates is copied and freed here.
    unsafe {
        ConvertSidToStringSidW(sid, &mut s).ok()?;
        let text = s.to_string().ok();
        let _ = LocalFree(HLOCAL(s.0.cast()));
        text
    }
}

impl SecurityReader for WindowsSecurity {
    fn read(&self, path: &Path) -> Result<SecurityInfo, String> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut owner = PSID::default();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: every out pointer is a valid local; the descriptor is freed before returning and
        // nothing read from it outlives that.
        unsafe {
            let r = GetNamedSecurityInfoW(
                PCWSTR(wide.as_ptr()),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                Some(&mut owner),
                None,
                Some(&mut dacl),
                None,
                &mut sd,
            );
            if r != ERROR_SUCCESS {
                return Err(format!("its permissions could not be read (error {})", r.0));
            }
            let info = (|| {
                let owner = sid_text(owner).ok_or("its owner could not be read")?;
                if dacl.is_null() {
                    return Ok(SecurityInfo { owner, dacl: None });
                }
                let mut aces = Vec::new();
                for i in 0..u32::from((*dacl).AceCount) {
                    let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
                    if GetAce(dacl, i, &mut p).is_err() || p.is_null() {
                        return Err("its access list could not be read".to_string());
                    }
                    let h = &*(p as *const ACE_HEADER);
                    if DENY_TYPES.contains(&h.AceType) {
                        continue;
                    }
                    if h.AceType != ACCESS_ALLOWED {
                        // An unusual allow entry: count it as a grant to someone unknown.
                        aces.push(Ace {
                            sid: format!("an entry of type {}", h.AceType),
                            mask: u32::MAX,
                            inherit_only: false,
                        });
                        continue;
                    }
                    let a = &*(p as *const ACCESS_ALLOWED_ACE);
                    let sid = PSID(std::ptr::addr_of!(a.SidStart) as *mut core::ffi::c_void);
                    aces.push(Ace {
                        sid: sid_text(sid).unwrap_or_else(|| "an unreadable account".into()),
                        mask: a.Mask,
                        inherit_only: h.AceFlags & INHERIT_ONLY != 0,
                    });
                }
                Ok(SecurityInfo {
                    owner,
                    dacl: Some(aces),
                })
            })();
            let _ = LocalFree(HLOCAL(sd.0));
            info
        }
    }
}

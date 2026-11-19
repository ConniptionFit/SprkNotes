//! WinHTTP client for model downloads (ADR-005). The only network code in the app. It speaks https
//! only, uses the system proxy and certificate store (company proxies and internal CAs work), refuses
//! https-to-http redirects (the WinHTTP default) and enforces size limits.
#![cfg(windows)]

use sprk_models::remote::{check_https, Fetcher, RemoteError};
use std::ffi::c_void;
use std::io::Write;
use std::path::Path;
use windows::core::{w, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpCrackUrl, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetTimeouts, URL_COMPONENTS, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the handle came from a WinHttp* open call and is closed exactly once here.
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

pub struct WinHttp;

impl WinHttp {
    /// Open the request and read the response headers. Returns the request handle, the status and
    /// the declared length. The connection and session handles live as long as the request.
    fn open(url: &str) -> Result<(Vec<Handle>, u32, Option<u64>), RemoteError> {
        check_https(url)?;
        let wurl: Vec<u16> = url.encode_utf16().collect();
        let mut host = [0u16; 256];
        let mut path = vec![0u16; url.len() + 8];
        let mut extra = vec![0u16; url.len() + 8];
        let mut uc = URL_COMPONENTS {
            dwStructSize: std::mem::size_of::<URL_COMPONENTS>() as u32,
            lpszHostName: windows::core::PWSTR(host.as_mut_ptr()),
            dwHostNameLength: host.len() as u32,
            lpszUrlPath: windows::core::PWSTR(path.as_mut_ptr()),
            dwUrlPathLength: path.len() as u32,
            lpszExtraInfo: windows::core::PWSTR(extra.as_mut_ptr()),
            dwExtraInfoLength: extra.len() as u32,
            ..Default::default()
        };
        // SAFETY: `uc` points at buffers that outlive the call and carry their own lengths.
        unsafe { WinHttpCrackUrl(&wurl, 0, &mut uc) }.map_err(|_| RemoteError::NotHttps)?;
        let host_len = uc.dwHostNameLength as usize;
        let path_len = uc.dwUrlPathLength as usize;
        let mut whost = host[..host_len].to_vec();
        whost.push(0);
        let mut wpath = path[..path_len].to_vec();
        // The query string is returned separately; the request target needs both.
        wpath.extend_from_slice(&extra[..uc.dwExtraInfoLength as usize]);
        wpath.push(0);

        // SAFETY: plain WinHTTP calls with NUL-terminated strings that outlive them; every handle
        // is wrapped so it is closed on every path.
        unsafe {
            let session = Handle(WinHttpOpen(
                w!("SprkNotes"),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            ));
            if session.0.is_null() {
                return Err(RemoteError::Network);
            }
            let _ = WinHttpSetTimeouts(session.0, 15_000, 15_000, 30_000, 60_000);
            let conn = Handle(WinHttpConnect(
                session.0,
                PCWSTR(whost.as_ptr()),
                uc.nPort,
                0,
            ));
            if conn.0.is_null() {
                return Err(RemoteError::Network);
            }
            let req = Handle(WinHttpOpenRequest(
                conn.0,
                w!("GET"),
                PCWSTR(wpath.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ));
            if req.0.is_null() {
                return Err(RemoteError::Network);
            }
            WinHttpSendRequest(req.0, None, None, 0, 0, 0).map_err(|_| RemoteError::Network)?;
            WinHttpReceiveResponse(req.0, std::ptr::null_mut())
                .map_err(|_| RemoteError::Network)?;

            let mut status = 0u32;
            let mut len = std::mem::size_of::<u32>() as u32;
            WinHttpQueryHeaders(
                req.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some(&mut status as *mut u32 as *mut c_void),
                &mut len,
                std::ptr::null_mut(),
            )
            .map_err(|_| RemoteError::Network)?;
            let mut total = 0u32;
            let mut tlen = std::mem::size_of::<u32>() as u32;
            let declared = WinHttpQueryHeaders(
                req.0,
                WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some(&mut total as *mut u32 as *mut c_void),
                &mut tlen,
                std::ptr::null_mut(),
            )
            .ok()
            .map(|_| u64::from(total));
            // Order matters for drop: request, then connection, then session.
            Ok((vec![req, conn, session], status, declared))
        }
    }

    fn read_all(
        req: &Handle,
        max: u64,
        declared: Option<u64>,
        out: &mut dyn Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<(), RemoteError> {
        if declared.is_some_and(|d| d > max) {
            return Err(RemoteError::TooLarge);
        }
        let mut buf = vec![0u8; 256 * 1024];
        let mut done = 0u64;
        loop {
            let mut n = 0u32;
            // SAFETY: `buf` is valid for `buf.len()` bytes and `n` receives the count read.
            unsafe {
                WinHttpReadData(
                    req.0,
                    buf.as_mut_ptr() as *mut c_void,
                    buf.len() as u32,
                    &mut n,
                )
            }
            .map_err(|_| RemoteError::Network)?;
            if n == 0 {
                return Ok(());
            }
            done += u64::from(n);
            if done > max {
                return Err(RemoteError::TooLarge);
            }
            out.write_all(&buf[..n as usize])
                .map_err(|_| RemoteError::Disk)?;
            progress(done, declared);
        }
    }
}

impl Fetcher for WinHttp {
    fn get_bytes(&self, url: &str, max: u64) -> Result<Vec<u8>, RemoteError> {
        let (handles, status, declared) = Self::open(url)?;
        if status != 200 {
            return Err(RemoteError::Network);
        }
        let mut out = Vec::new();
        Self::read_all(&handles[0], max, declared, &mut out, &mut |_, _| {})?;
        Ok(out)
    }

    fn get_to_file(
        &self,
        url: &str,
        dest: &Path,
        max: u64,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<(), RemoteError> {
        let (handles, status, declared) = Self::open(url)?;
        if status != 200 {
            return Err(RemoteError::Network);
        }
        let mut f = std::fs::File::create(dest).map_err(|_| RemoteError::Disk)?;
        Self::read_all(&handles[0], max, declared, &mut f, progress)?;
        f.flush().map_err(|_| RemoteError::Disk)
    }
}

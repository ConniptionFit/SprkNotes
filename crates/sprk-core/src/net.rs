//! HTTPS client for model downloads off Windows (ADR-005 equivalent of the WinHTTP client),
//! shared by the Linux desktop and the phone. https only, size limits enforced while streaming.
//! Certificates: the system store on Linux; on Android the bundled Mozilla roots, because
//! Android's store is reached through Java, which this client does not use. An internal mirror with a private
//! certificate authority therefore needs a public certificate to serve phones.

use sprk_models::remote::{check_https, Fetcher, RemoteError};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

pub struct Https;

impl Https {
    fn open(url: &str) -> Result<(Box<dyn Read + Send + Sync>, Option<u64>), RemoteError> {
        check_https(url)?;
        let agent = ureq::AgentBuilder::new()
            .redirects(5)
            .timeout_connect(Duration::from_secs(20))
            .timeout_read(Duration::from_secs(60))
            .build();
        let resp = agent.get(url).call().map_err(|_| RemoteError::Network)?;
        // A redirect must not have downgraded to http.
        check_https(resp.get_url())?;
        if resp.status() != 200 {
            return Err(RemoteError::Network);
        }
        let declared = resp
            .header("Content-Length")
            .and_then(|v| v.parse::<u64>().ok());
        Ok((resp.into_reader(), declared))
    }

    fn pump(
        mut r: impl Read,
        max: u64,
        declared: Option<u64>,
        out: &mut dyn Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<(), RemoteError> {
        if declared.is_some_and(|d| d > max) {
            return Err(RemoteError::TooLarge);
        }
        let mut buf = vec![0u8; 64 * 1024];
        let mut done = 0u64;
        loop {
            let n = r.read(&mut buf).map_err(|_| RemoteError::Network)?;
            if n == 0 {
                return Ok(());
            }
            done += n as u64;
            if done > max {
                return Err(RemoteError::TooLarge);
            }
            out.write_all(&buf[..n]).map_err(|_| RemoteError::Disk)?;
            progress(done, declared);
        }
    }
}

impl Fetcher for Https {
    fn get_bytes(&self, url: &str, max: u64) -> Result<Vec<u8>, RemoteError> {
        let (r, declared) = Self::open(url)?;
        let mut out = Vec::new();
        Self::pump(r, max, declared, &mut out, &mut |_, _| {})?;
        Ok(out)
    }

    fn get_to_file(
        &self,
        url: &str,
        dest: &Path,
        max: u64,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<(), RemoteError> {
        let (r, declared) = Self::open(url)?;
        let mut f = std::fs::File::create(dest).map_err(|_| RemoteError::Disk)?;
        Self::pump(r, max, declared, &mut f, progress)?;
        f.flush().map_err(|_| RemoteError::Disk)
    }
}

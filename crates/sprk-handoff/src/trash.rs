//! Move files to the Recycle Bin (Windows) or the user's trash (Linux, freedesktop.org Trash
//! spec), so a Library delete (SPEC 8.4) can be undone from there. Never deletes permanently on
//! its own: when the trash cannot take a file, the call fails and the file stays.
use std::path::PathBuf;

/// Move each path (absolute) to the trash, in order. Stops at the first failure.
pub fn to_trash(paths: &[PathBuf]) -> std::io::Result<()> {
    imp::to_trash(paths)
}

#[cfg(windows)]
mod imp {
    #![allow(unsafe_code)]
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
        FOF_WANTNUKEWARNING, FO_DELETE, SHFILEOPSTRUCTW,
    };

    pub fn to_trash(paths: &[PathBuf]) -> std::io::Result<()> {
        for p in paths {
            // pFrom is a list of NUL-terminated paths ending with an extra NUL.
            let mut from: Vec<u16> = p.as_os_str().encode_wide().collect();
            if from.contains(&0) {
                return Err(std::io::ErrorKind::InvalidInput.into());
            }
            from.extend([0, 0]);
            // ALLOWUNDO sends the file to the Recycle Bin. NOCONFIRMATION skips the "are you
            // sure" (the window already asked); WANTNUKEWARNING brings back a prompt when the
            // file cannot be recycled and would be destroyed, so that is never silent.
            let flags = FOF_ALLOWUNDO
                | FOF_NOCONFIRMATION
                | FOF_NOERRORUI
                | FOF_SILENT
                | FOF_WANTNUKEWARNING;
            let mut op = SHFILEOPSTRUCTW {
                wFunc: FO_DELETE,
                pFrom: PCWSTR(from.as_ptr()),
                fFlags: flags.0 as u16,
                ..Default::default()
            };
            // SAFETY: `op` is a fully initialised SHFILEOPSTRUCTW; `pFrom` points into `from`,
            // which is double-NUL-terminated and outlives the call; the other pointers are null.
            let r = unsafe { SHFileOperationW(&mut op) };
            if r != 0 {
                return Err(std::io::Error::other(
                    "the Recycle Bin did not take the file",
                ));
            }
            if op.fAnyOperationsAborted.as_bool() {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::PathBuf;

    pub fn to_trash(paths: &[PathBuf]) -> std::io::Result<()> {
        let dir = super::home_trash().ok_or(std::io::ErrorKind::NotFound)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        for p in paths {
            super::trash_into(&dir, p, now)?;
        }
        Ok(())
    }
}

/// `$XDG_DATA_HOME/Trash`, else `~/.local/share/Trash`.
#[cfg(not(windows))]
fn home_trash() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(data.join("Trash"))
}

/// Percent-encode a path for `Path=` in a `.trashinfo` file (unreserved bytes and `/` stay).
#[cfg(any(not(windows), test))]
fn encode_path(p: &std::path::Path) -> String {
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        p.as_os_str().as_bytes().to_vec()
    };
    #[cfg(not(unix))]
    let bytes = p.to_string_lossy().into_owned().into_bytes();
    let mut s = String::new();
    for b in bytes {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

/// `YYYY-MM-DDThh:mm:ss` (UTC) for `DeletionDate=`.
#[cfg(any(not(windows), test))]
fn deletion_date(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}",
        rem / 3600,
        (rem / 60) % 60,
        rem % 60
    )
}

/// Move `path` into the trash folder `trash` (`files/` plus an `info/*.trashinfo` note, which
/// is written first and claims the name). A file on another drive cannot be moved there; that
/// fails and the file stays where it was.
#[cfg(any(not(windows), test))]
fn trash_into(trash: &std::path::Path, path: &std::path::Path, now: u64) -> std::io::Result<()> {
    use std::io::Write;
    let files = trash.join("files");
    let info = trash.join("info");
    std::fs::create_dir_all(&files)?;
    std::fs::create_dir_all(&info)?;
    let name = path
        .file_name()
        .ok_or(std::io::ErrorKind::InvalidInput)?
        .to_string_lossy()
        .into_owned();
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    for n in 1..10_000u32 {
        let candidate = if n == 1 {
            name.clone()
        } else {
            format!("{stem}.{n}{ext}")
        };
        if files.join(&candidate).exists() {
            continue;
        }
        let note = info.join(format!("{candidate}.trashinfo"));
        let mut f = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&note)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        let body = format!(
            "[Trash Info]\nPath={}\nDeletionDate={}\n",
            encode_path(path),
            deletion_date(now)
        );
        let moved = f
            .write_all(body.as_bytes())
            .and_then(|()| f.sync_all())
            .and_then(|()| std::fs::rename(path, files.join(&candidate)));
        if let Err(e) = moved {
            let _ = std::fs::remove_file(&note);
            return Err(e);
        }
        return Ok(());
    }
    Err(std::io::ErrorKind::AlreadyExists.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_paths_follow_the_trash_spec() {
        assert_eq!(deletion_date(0), "1970-01-01T00:00:00");
        assert_eq!(deletion_date(1_795_569_640), "2026-11-25T01:20:40");
        assert_eq!(deletion_date(951_782_400), "2000-02-29T00:00:00");
        assert_eq!(
            encode_path(std::path::Path::new("/home/a/2026-11-19 0900 Q&A.md")),
            "/home/a/2026-11-19%200900%20Q%26A.md"
        );
    }

    #[test]
    fn moves_into_files_with_a_note_and_unique_names() {
        let base = std::env::temp_dir().join(format!("sprk-trash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let lib = base.join("lib");
        let trash = base.join("Trash");
        std::fs::create_dir_all(&lib).unwrap();
        for _ in 0..2 {
            let p = lib.join("t.md");
            std::fs::write(&p, "x").unwrap();
            trash_into(&trash, &p, 0).unwrap();
            assert!(!p.exists());
        }
        assert!(trash.join("files/t.md").exists());
        assert!(trash.join("files/t.2.md").exists());
        let rec = std::fs::read_to_string(trash.join("info/t.2.md.trashinfo")).unwrap();
        assert!(rec.starts_with("[Trash Info]\nPath="), "{rec}");
        // The trash spec wants an absolute path; on Windows (tests only) it is the encoded drive path.
        #[cfg(unix)]
        assert!(rec.starts_with("[Trash Info]\nPath=/"), "{rec}");
        assert!(rec.contains("t.md\nDeletionDate=1970-01-01T00:00:00\n"));

        // A file that cannot be moved stays, and leaves no note behind.
        let missing = lib.join("gone.md");
        assert!(trash_into(&trash, &missing, 0).is_err());
        assert!(!trash.join("info/gone.md.trashinfo").exists());
        std::fs::remove_dir_all(&base).unwrap();
    }
}

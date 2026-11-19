//! Model download and update check (ADR-005). The only network use SprkNotes has, and only for
//! model packs: an index file and the `.sprkmodel` packs it lists. Packs are verified twice: the
//! download must match the index SHA-256, then the pack goes through the normal import flow
//! (manifest hashes, signature from 1.0, smoke test). Audio and transcripts never go near it.
use crate::import::{import_pack, ImportOptions, ImportOutcome, SmokeTester};
use crate::store::ModelStore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const INDEX_SCHEMA: u32 = 1;
/// Upper bounds so a bad or hostile server cannot fill the disk or memory.
pub const MAX_INDEX_BYTES: u64 = 256 * 1024;
pub const MAX_PACK_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RemoteError {
    #[error("downloads are turned off by policy")]
    Disabled,
    #[error("only https addresses are allowed")]
    NotHttps,
    #[error("could not reach the model server")]
    Network,
    #[error("the model server sent something unexpected")]
    BadIndex,
    #[error("the download did not match its checksum")]
    Checksum,
    #[error("the download was too large")]
    TooLarge,
    #[error("not enough disk space or the folder is not writable")]
    Disk,
    #[error("the downloaded model was rejected: {0}")]
    Rejected(String),
}

/// One slice of a pack, for hosts that cap file size (the pack is the parts joined in order).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

pub const MAX_PARTS: usize = 64;

/// One downloadable pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub id: String,
    pub version: String,
    /// `transcription` or `vad`.
    pub role: String,
    /// The whole pack. May be empty when `parts` is given.
    #[serde(default)]
    pub url: String,
    /// Whole-pack SHA-256 and size (checked after joining the parts).
    pub sha256: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<Part>,
    /// Part of the "Download model" set.
    #[serde(default)]
    pub default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub schema: u32,
    pub models: Vec<IndexEntry>,
}

impl Index {
    pub fn parse(bytes: &[u8]) -> Result<Index, RemoteError> {
        let i: Index = serde_json::from_slice(bytes).map_err(|_| RemoteError::BadIndex)?;
        if i.schema != INDEX_SCHEMA {
            return Err(RemoteError::BadIndex);
        }
        for m in &i.models {
            let hex = |h: &str| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit());
            if m.parts.is_empty() {
                check_https(&m.url)?;
            } else {
                if m.parts.len() > MAX_PARTS {
                    return Err(RemoteError::BadIndex);
                }
                let mut total = 0u64;
                for part in &m.parts {
                    check_https(&part.url)?;
                    if !hex(&part.sha256) || part.size == 0 {
                        return Err(RemoteError::BadIndex);
                    }
                    total = total.saturating_add(part.size);
                }
                if total != m.size {
                    return Err(RemoteError::BadIndex);
                }
            }
            let hex_ok = hex(&m.sha256);
            if m.id.is_empty() || m.version.is_empty() || !hex_ok || m.size > MAX_PACK_BYTES {
                return Err(RemoteError::BadIndex);
            }
            // Ids and versions become folder names: no separators or traversal.
            for part in [&m.id, &m.version] {
                if part.contains(['/', '\\', '@', ':']) || part.contains("..") {
                    return Err(RemoteError::BadIndex);
                }
            }
        }
        Ok(i)
    }

    /// The set the "Download model" button installs.
    pub fn defaults(&self) -> Vec<&IndexEntry> {
        self.models.iter().filter(|m| m.default).collect()
    }
}

pub fn check_https(url: &str) -> Result<(), RemoteError> {
    if url.len() > 2048 || !url.to_ascii_lowercase().starts_with("https://") {
        return Err(RemoteError::NotHttps);
    }
    Ok(())
}

/// The platform HTTP client (WinHTTP in the app, a fake in tests). It must follow redirects
/// only to https, and honor the system proxy and certificate store.
pub trait Fetcher {
    fn get_bytes(&self, url: &str, max: u64) -> Result<Vec<u8>, RemoteError>;
    /// Stream into `dest`; `progress(done, total)` is called as bytes arrive.
    fn get_to_file(
        &self,
        url: &str,
        dest: &Path,
        max: u64,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<(), RemoteError>;
}

fn parts(v: &str) -> Vec<u64> {
    v.split(['.', '-'])
        .map(|p| p.parse::<u64>().unwrap_or(0))
        .collect()
}

/// Numeric dotted comparison ("2026.11.0" < "2026.12.0" < "2026.12.1").
pub fn newer(candidate: &str, installed: &str) -> bool {
    let (a, b) = (parts(candidate), parts(installed));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub id: String,
    pub version: String,
    /// Installed version, if any (None: not installed yet).
    pub installed: Option<String>,
}

/// Which default packs are missing or older than the index's.
pub fn pending(index: &Index, stores: &[&ModelStore]) -> Vec<Update> {
    let mut out = Vec::new();
    for e in index.defaults() {
        let mut best: Option<String> = None;
        for s in stores {
            for i in s.list().into_iter().filter(|i| i.id == e.id) {
                if best.as_deref().is_none_or(|b| newer(&i.version, b)) {
                    best = Some(i.version);
                }
            }
        }
        if best.as_deref().is_none_or(|b| newer(&e.version, b)) {
            out.push(Update {
                id: e.id.clone(),
                version: e.version.clone(),
                installed: best,
            });
        }
    }
    out
}

pub fn fetch_index(f: &dyn Fetcher, url: &str) -> Result<Index, RemoteError> {
    check_https(url)?;
    Index::parse(&f.get_bytes(url, MAX_INDEX_BYTES)?)
}

fn sha256_file(p: &Path) -> Result<String, RemoteError> {
    let mut f = std::fs::File::open(p).map_err(|_| RemoteError::Disk)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|_| RemoteError::Disk)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Fetch the pack into `partial`: one request, or each part checked and appended in order.
fn fetch_whole(
    f: &dyn Fetcher,
    entry: &IndexEntry,
    partial: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), RemoteError> {
    let total = Some(entry.size);
    if entry.parts.is_empty() {
        let limit = entry.size.clamp(1, MAX_PACK_BYTES) + 1024;
        return f.get_to_file(&entry.url, partial, limit, progress);
    }
    let mut out = std::fs::File::create(partial).map_err(|_| RemoteError::Disk)?;
    let mut base = 0u64;
    for (n, part) in entry.parts.iter().enumerate() {
        let tmp = partial.with_extension(format!("part{n}"));
        let _ = std::fs::remove_file(&tmp);
        let b = base;
        let r = f.get_to_file(&part.url, &tmp, part.size + 1024, &mut |done, _| {
            progress(b + done, total)
        });
        let r = r.and_then(|_| {
            if sha256_file(&tmp)?.eq_ignore_ascii_case(&part.sha256) {
                Ok(())
            } else {
                Err(RemoteError::Checksum)
            }
        });
        let r = r.and_then(|_| {
            let mut src = std::fs::File::open(&tmp).map_err(|_| RemoteError::Disk)?;
            std::io::copy(&mut src, &mut out).map_err(|_| RemoteError::Disk)?;
            Ok(())
        });
        let _ = std::fs::remove_file(&tmp);
        r?;
        base += part.size;
    }
    out.sync_all().map_err(|_| RemoteError::Disk)
}

/// Download one pack into `store/incoming/<id>@<version>.sprkmodel.partial`, check it against
/// the index checksum, then run the normal import. Nothing is activated unless the import says so.
pub fn download_and_install(
    f: &dyn Fetcher,
    entry: &IndexEntry,
    store: &ModelStore,
    opts: &ImportOptions,
    smoke: &dyn SmokeTester,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<ImportOutcome, RemoteError> {
    if entry.parts.is_empty() {
        check_https(&entry.url)?;
    }
    let incoming: PathBuf = store.root().join("incoming");
    std::fs::create_dir_all(&incoming).map_err(|_| RemoteError::Disk)?;
    let name = format!("{}@{}.sprkmodel", entry.id, entry.version);
    let partial = incoming.join(format!("{name}.partial"));
    let _ = std::fs::remove_file(&partial);
    if let Err(e) = fetch_whole(f, entry, &partial, progress) {
        let _ = std::fs::remove_file(&partial);
        return Err(e);
    }
    let got = sha256_file(&partial)?;
    if !got.eq_ignore_ascii_case(&entry.sha256) {
        let _ = std::fs::remove_file(&partial);
        return Err(RemoteError::Checksum);
    }
    let pack = incoming.join(&name);
    std::fs::rename(&partial, &pack).map_err(|_| RemoteError::Disk)?;
    Ok(import_pack(store, &pack, opts, smoke))
}

/// Build the index line for a pack file (maintainer tool: `sprkctl model index`). `base_url` is the
/// https folder the file is published under; `url_suffix` is appended to the file name (for hosts
/// that only accept certain extensions, such as `.zip`).
pub fn entry_for_pack(
    pack: &Path,
    base_url: &str,
    url_suffix: &str,
    default: bool,
) -> Result<IndexEntry, RemoteError> {
    let bytes =
        crate::pack::read_manifest_bytes(pack).map_err(|e| RemoteError::Rejected(e.to_string()))?;
    let m = crate::manifest::Manifest::from_json(&String::from_utf8_lossy(&bytes))
        .map_err(|e| RemoteError::Rejected(e.to_string()))?;
    let name = pack
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or(RemoteError::Disk)?;
    let url = format!("{}/{}{}", base_url.trim_end_matches('/'), name, url_suffix);
    check_https(&url)?;
    let size = std::fs::metadata(pack)
        .map_err(|_| RemoteError::Disk)?
        .len();
    Ok(IndexEntry {
        role: if m.engine.contains("vad") {
            "vad"
        } else {
            "transcription"
        }
        .to_string(),
        id: m.id,
        version: m.version,
        url,
        sha256: sha256_file(pack)?,
        size,
        default,
        parts: Vec::new(),
    })
}

/// Slice a pack into files of at most `max_bytes` for hosts with a file size cap and return the
/// index parts. Files are written to `dir` as `<pack name>.partNNN<url_suffix>`, ready to upload.
pub fn split_pack(
    pack: &Path,
    dir: &Path,
    max_bytes: u64,
    base_url: &str,
    url_suffix: &str,
) -> Result<Vec<Part>, RemoteError> {
    use std::io::Write;
    std::fs::create_dir_all(dir).map_err(|_| RemoteError::Disk)?;
    let name = pack
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or(RemoteError::Disk)?;
    let mut src = std::fs::File::open(pack).map_err(|_| RemoteError::Disk)?;
    let mut parts = Vec::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut n = 1;
    loop {
        let file = format!("{name}.part{n:03}{url_suffix}");
        let path = dir.join(&file);
        let mut out = std::fs::File::create(&path).map_err(|_| RemoteError::Disk)?;
        let mut written = 0u64;
        let mut h = Sha256::new();
        while written < max_bytes {
            let want = (max_bytes - written).min(buf.len() as u64) as usize;
            let got = src.read(&mut buf[..want]).map_err(|_| RemoteError::Disk)?;
            if got == 0 {
                break;
            }
            out.write_all(&buf[..got]).map_err(|_| RemoteError::Disk)?;
            h.update(&buf[..got]);
            written += got as u64;
        }
        drop(out);
        if written == 0 {
            let _ = std::fs::remove_file(&path);
            break;
        }
        let url = format!("{}/{}", base_url.trim_end_matches('/'), file);
        check_https(&url)?;
        parts.push(Part {
            url,
            sha256: hex::encode(h.finalize()),
            size: written,
        });
        n += 1;
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Manifest;
    use crate::pack::VerifyOptions;
    use crate::store::StoreKind;
    use std::cell::RefCell;

    struct Fake {
        index: Vec<u8>,
        pack: Vec<u8>,
        calls: RefCell<Vec<String>>,
    }
    impl Fetcher for Fake {
        fn get_bytes(&self, url: &str, _: u64) -> Result<Vec<u8>, RemoteError> {
            self.calls.borrow_mut().push(url.into());
            Ok(self.index.clone())
        }
        fn get_to_file(
            &self,
            url: &str,
            dest: &Path,
            max: u64,
            progress: &mut dyn FnMut(u64, Option<u64>),
        ) -> Result<(), RemoteError> {
            self.calls.borrow_mut().push(url.into());
            if self.pack.len() as u64 > max {
                return Err(RemoteError::TooLarge);
            }
            std::fs::write(dest, &self.pack).map_err(|_| RemoteError::Disk)?;
            progress(self.pack.len() as u64, Some(self.pack.len() as u64));
            Ok(())
        }
    }

    struct Ok1;
    impl SmokeTester for Ok1 {
        fn smoke(&self, _: &Path, _: &Manifest) -> Result<(), String> {
            Ok(())
        }
    }

    fn index_json(sha: &str, size: u64) -> Vec<u8> {
        format!(
            r#"{{"schema":1,"models":[{{"id":"m","version":"2026.12.0","role":"transcription","url":"https://h/m.zip","sha256":"{sha}","size":{size},"default":true}}]}}"#
        )
        .into_bytes()
    }

    #[test]
    fn version_order_is_numeric() {
        assert!(newer("2026.12.0", "2026.11.0"));
        assert!(newer("2026.11.10", "2026.11.9"));
        assert!(!newer("2026.11.0", "2026.11.0"));
        assert!(!newer("2026.11.0", "2026.12.0"));
    }

    #[test]
    fn index_rejects_http_bad_hash_and_path_tricks() {
        let ok = index_json(&"a".repeat(64), 10);
        assert!(Index::parse(&ok).is_ok());
        let http = String::from_utf8(ok.clone())
            .unwrap()
            .replace("https://", "http://");
        assert_eq!(Index::parse(http.as_bytes()), Err(RemoteError::NotHttps));
        assert_eq!(
            Index::parse(&index_json("zz", 10)),
            Err(RemoteError::BadIndex)
        );
        let evil = String::from_utf8(ok)
            .unwrap()
            .replace("\"id\":\"m\"", "\"id\":\"..\\\\x\"");
        assert_eq!(Index::parse(evil.as_bytes()), Err(RemoteError::BadIndex));
        assert_eq!(Index::parse(b"not json"), Err(RemoteError::BadIndex));
    }

    #[test]
    fn pending_lists_missing_and_older_only() {
        let d = tempfile::tempdir().unwrap();
        let s = ModelStore::new(d.path().to_path_buf(), StoreKind::User);
        let idx = Index::parse(&index_json(&"a".repeat(64), 10)).unwrap();
        assert_eq!(pending(&idx, &[&s]).len(), 1);
        std::fs::create_dir_all(d.path().join("m@2026.11.0")).unwrap();
        std::fs::write(d.path().join("m@2026.11.0").join("model.json"), "{}").unwrap();
        // An unreadable model.json does not count as installed.
        assert_eq!(pending(&idx, &[&s]).len(), 1);
    }

    #[test]
    fn wrong_checksum_is_refused_and_cleaned_up() {
        let d = tempfile::tempdir().unwrap();
        let s = ModelStore::new(d.path().to_path_buf(), StoreKind::User);
        let f = Fake {
            index: vec![],
            pack: b"hello".to_vec(),
            calls: RefCell::default(),
        };
        let e = Index::parse(&index_json(&"b".repeat(64), 5))
            .unwrap()
            .models
            .remove(0);
        let o = ImportOptions {
            verify: VerifyOptions {
                supported_engines: &[],
                app_version: "0.0.1",
                allowed_hashes: &[],
                require_signature: false,
                verifier: None,
            },
            activate: false,
            keep_versions: 2,
            now: "t",
        };
        let r = download_and_install(&f, &e, &s, &o, &Ok1, &mut |_, _| {});
        assert_eq!(r.unwrap_err(), RemoteError::Checksum);
        assert!(!d
            .path()
            .join("incoming/m@2026.12.0.sprkmodel.partial")
            .exists());
        assert!(!d.path().join("incoming/m@2026.12.0.sprkmodel").exists());
    }

    #[test]
    fn right_checksum_goes_through_import_and_a_bad_pack_is_rejected() {
        let d = tempfile::tempdir().unwrap();
        let s = ModelStore::new(d.path().to_path_buf(), StoreKind::User);
        let body = b"not a zip".to_vec();
        let sha = hex::encode(Sha256::digest(&body));
        let f = Fake {
            index: vec![],
            pack: body,
            calls: RefCell::default(),
        };
        let e = Index::parse(&index_json(&sha, 9)).unwrap().models.remove(0);
        let o = ImportOptions {
            verify: VerifyOptions {
                supported_engines: &[],
                app_version: "0.0.1",
                allowed_hashes: &[],
                require_signature: false,
                verifier: None,
            },
            activate: false,
            keep_versions: 2,
            now: "t",
        };
        let r = download_and_install(&f, &e, &s, &o, &Ok1, &mut |_, _| {}).unwrap();
        assert!(matches!(r, ImportOutcome::Rejected { .. }));
        assert!(s.list().is_empty());
    }

    /// Serves different bytes per URL, so parts can be told apart.
    struct Multi(Vec<(String, Vec<u8>)>);
    impl Fetcher for Multi {
        fn get_bytes(&self, _: &str, _: u64) -> Result<Vec<u8>, RemoteError> {
            Err(RemoteError::Network)
        }
        fn get_to_file(
            &self,
            url: &str,
            dest: &Path,
            _: u64,
            progress: &mut dyn FnMut(u64, Option<u64>),
        ) -> Result<(), RemoteError> {
            let (_, b) = self
                .0
                .iter()
                .find(|(u, _)| u == url)
                .ok_or(RemoteError::Network)?;
            std::fs::write(dest, b).map_err(|_| RemoteError::Disk)?;
            progress(b.len() as u64, None);
            Ok(())
        }
    }

    fn opts<'a>() -> ImportOptions<'a> {
        ImportOptions {
            verify: VerifyOptions {
                supported_engines: &[],
                app_version: "0.0.1",
                allowed_hashes: &[],
                require_signature: false,
                verifier: None,
            },
            activate: false,
            keep_versions: 2,
            now: "t",
        }
    }

    fn multipart_entry(a: &[u8], b: &[u8]) -> IndexEntry {
        let whole: Vec<u8> = [a, b].concat();
        IndexEntry {
            id: "m".into(),
            version: "2026.12.0".into(),
            role: "transcription".into(),
            url: String::new(),
            sha256: hex::encode(Sha256::digest(&whole)),
            size: whole.len() as u64,
            default: true,
            parts: vec![
                Part {
                    url: "https://h/m.part001.zip".into(),
                    sha256: hex::encode(Sha256::digest(a)),
                    size: a.len() as u64,
                },
                Part {
                    url: "https://h/m.part002.zip".into(),
                    sha256: hex::encode(Sha256::digest(b)),
                    size: b.len() as u64,
                },
            ],
        }
    }

    #[test]
    fn parts_are_checked_joined_and_imported() {
        let d = tempfile::tempdir().unwrap();
        let s = ModelStore::new(d.path().to_path_buf(), StoreKind::User);
        let (a, b) = (b"first half ".to_vec(), b"second half".to_vec());
        let e = multipart_entry(&a, &b);
        let f = Multi(vec![
            ("https://h/m.part001.zip".into(), a),
            ("https://h/m.part002.zip".into(), b),
        ]);
        let mut seen = 0;
        let r = download_and_install(&f, &e, &s, &opts(), &Ok1, &mut |d, t| {
            seen = d;
            assert_eq!(t, Some(e.size));
        })
        .unwrap();
        // The joined bytes are not a real pack, so the import (not the checksums) rejects them.
        assert!(matches!(r, ImportOutcome::Rejected { .. }));
        assert_eq!(seen, e.size);
        assert!(std::fs::read_dir(d.path().join("incoming"))
            .unwrap()
            .flatten()
            .all(|f| !f.file_name().to_string_lossy().contains(".part")));
    }

    #[test]
    fn a_corrupt_part_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let s = ModelStore::new(d.path().to_path_buf(), StoreKind::User);
        let (a, b) = (b"first half ".to_vec(), b"second half".to_vec());
        let e = multipart_entry(&a, &b);
        let f = Multi(vec![
            ("https://h/m.part001.zip".into(), a),
            ("https://h/m.part002.zip".into(), b"tampered!!".to_vec()),
        ]);
        let r = download_and_install(&f, &e, &s, &opts(), &Ok1, &mut |_, _| {});
        assert_eq!(r.unwrap_err(), RemoteError::Checksum);
        assert!(
            std::fs::read_dir(d.path().join("incoming"))
                .unwrap()
                .count()
                == 0
        );
    }

    #[test]
    fn index_rejects_parts_whose_sizes_do_not_add_up() {
        let e = multipart_entry(b"aa", b"bb");
        let mut idx = Index {
            schema: 1,
            models: vec![e],
        };
        assert!(Index::parse(serde_json::to_string(&idx).unwrap().as_bytes()).is_ok());
        idx.models[0].size += 1;
        assert_eq!(
            Index::parse(serde_json::to_string(&idx).unwrap().as_bytes()),
            Err(RemoteError::BadIndex)
        );
    }

    #[test]
    fn split_then_join_gives_the_same_bytes() {
        let d = tempfile::tempdir().unwrap();
        let pack = d.path().join("m@1.sprkmodel");
        let data: Vec<u8> = (0..2500u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&pack, &data).unwrap();
        let parts = split_pack(&pack, &d.path().join("out"), 1000, "https://h/dl", ".zip").unwrap();
        assert_eq!(
            parts.iter().map(|p| p.size).collect::<Vec<_>>(),
            vec![1000, 1000, 500]
        );
        assert!(parts[0].url.ends_with("/m@1.sprkmodel.part001.zip"));
        let mut joined = Vec::new();
        for n in 1..=3 {
            joined.extend(
                std::fs::read(d.path().join(format!("out/m@1.sprkmodel.part{n:03}.zip"))).unwrap(),
            );
        }
        assert_eq!(joined, data);
    }

    #[test]
    fn http_urls_never_reach_the_fetcher() {
        let f = Fake {
            index: vec![],
            pack: vec![],
            calls: RefCell::default(),
        };
        assert_eq!(
            fetch_index(&f, "http://x/index.json").unwrap_err(),
            RemoteError::NotHttps
        );
        assert!(f.calls.borrow().is_empty());
    }
}

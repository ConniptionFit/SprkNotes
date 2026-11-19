//! `.sprkmodel` packs: a zip with `model.json`, optional `model.sig` and the declared files
//! (SPEC 16.3, 16.6). Packs never contain executable code: only the manifest's files are
//! accepted, as flat names, with size limits.
use crate::manifest::{Manifest, ManifestError};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;

pub const MAX_ENTRIES: usize = 64;
pub const MAX_ENTRY_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("pack could not be read (partial or corrupt file)")]
    Corrupt,
    #[error("pack contains an unsafe or unexpected entry")]
    UnsafeEntry,
    #[error("pack entry is too large")]
    TooLarge,
    #[error("pack has no model.json")]
    NoManifest,
    #[error("{0}")]
    Manifest(#[from] ManifestError),
    #[error("signature check failed: {0}")]
    Signature(String),
    #[error("model is not on the admin allow-list (unverified)")]
    NotAllowed,
    #[error("file operation failed: {0}")]
    Io(#[from] std::io::Error),
}

/// M15 plugs the Ed25519 SSHSIG verifier in here (SPEC 16.10). The verifier sees the exact
/// `model.json` bytes and the `model.sig` contents.
pub trait SignatureVerifier {
    fn verify(&self, model_json: &[u8], signature: &[u8]) -> Result<(), String>;
}

pub struct VerifyOptions<'a> {
    pub supported_engines: &'a [&'a str],
    pub app_version: &'a str,
    /// Non-empty: only packs whose `model.json` SHA-256 is listed may install.
    pub allowed_hashes: &'a [String],
    pub require_signature: bool,
    pub verifier: Option<&'a dyn SignatureVerifier>,
}

/// A pack that passed every check that does not need extraction.
#[derive(Debug, Clone)]
pub struct VerifiedPack {
    pub manifest: Manifest,
    /// SHA-256 of `model.json` (what `AllowedModelHashes` holds).
    pub manifest_hash: String,
}

fn read_limited(entry: &mut impl Read, limit: u64) -> Result<Vec<u8>, PackError> {
    let mut buf = Vec::new();
    entry
        .take(limit + 1)
        .read_to_end(&mut buf)
        .map_err(|_| PackError::Corrupt)?;
    if buf.len() as u64 > limit {
        return Err(PackError::TooLarge);
    }
    Ok(buf)
}

/// Build a pack from a model folder (`sprkctl model pack`). The folder must verify.
pub fn pack_folder(folder: &Path, out: &Path) -> Result<Manifest, PackError> {
    let json = std::fs::read_to_string(folder.join("model.json"))?;
    let manifest = Manifest::from_json(&json)?;
    manifest.verify_files(folder)?;
    let file = std::fs::File::create(out)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut names: Vec<String> = manifest.files.values().cloned().collect();
    names.sort();
    names.dedup();
    zip.start_file("model.json", opts)
        .map_err(|_| PackError::Corrupt)?;
    zip.write_all(json.as_bytes())?;
    if folder.join("model.sig").is_file() {
        zip.start_file("model.sig", opts)
            .map_err(|_| PackError::Corrupt)?;
        zip.write_all(&std::fs::read(folder.join("model.sig"))?)?;
    }
    for n in names {
        zip.start_file(n.as_str(), opts)
            .map_err(|_| PackError::Corrupt)?;
        let mut f = std::fs::File::open(folder.join(&n))?;
        std::io::copy(&mut f, &mut zip)?;
    }
    zip.finish().map_err(|_| PackError::Corrupt)?;
    Ok(manifest)
}

fn flat_name_ok(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['/', '\\', ':'])
        && name != "."
        && name != ".."
        && !name.starts_with('.')
}

/// Open the zip, block unsafe entries, parse and check `model.json`, apply the allow-list and
/// signature rules. Hashes of the data files are checked after extraction.
pub fn verify_pack(pack: &Path, o: &VerifyOptions) -> Result<VerifiedPack, PackError> {
    let file = std::fs::File::open(pack)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| PackError::Corrupt)?; // partial files fail here
    if zip.len() > MAX_ENTRIES {
        return Err(PackError::UnsafeEntry);
    }
    let mut manifest_bytes = None;
    let mut sig_bytes = None;
    let mut names = Vec::new();
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).map_err(|_| PackError::Corrupt)?;
        let name = e.name().to_string();
        if e.is_dir() || e.enclosed_name().is_none() || !flat_name_ok(&name) {
            return Err(PackError::UnsafeEntry);
        }
        if e.size() > MAX_ENTRY_BYTES {
            return Err(PackError::TooLarge);
        }
        match name.as_str() {
            "model.json" => manifest_bytes = Some(read_limited(&mut e, MAX_MANIFEST_BYTES)?),
            "model.sig" => sig_bytes = Some(read_limited(&mut e, MAX_MANIFEST_BYTES)?),
            _ => {}
        }
        names.push(name);
    }
    let mbytes = manifest_bytes.ok_or(PackError::NoManifest)?;
    let text = std::str::from_utf8(&mbytes).map_err(|_| PackError::Corrupt)?;
    let manifest = Manifest::from_json(text)?;
    manifest.check_compat(o.supported_engines, o.app_version)?;
    // Only declared files (plus the manifest and signature) may be in the pack.
    for n in &names {
        let declared =
            n == "model.json" || n == "model.sig" || manifest.files.values().any(|f| f == n);
        if !declared {
            return Err(PackError::UnsafeEntry);
        }
    }
    let manifest_hash = hex::encode(Sha256::digest(&mbytes));
    if !o.allowed_hashes.is_empty()
        && !o
            .allowed_hashes
            .iter()
            .any(|h| h.eq_ignore_ascii_case(&manifest_hash))
    {
        return Err(PackError::NotAllowed);
    }
    if o.require_signature {
        let v = o
            .verifier
            .ok_or_else(|| PackError::Signature("no verifier available".into()))?;
        let sig = sig_bytes.ok_or_else(|| PackError::Signature("pack is not signed".into()))?;
        v.verify(&mbytes, &sig).map_err(PackError::Signature)?;
    }
    Ok(VerifiedPack {
        manifest,
        manifest_hash,
    })
}

/// Extract into `dest` (an empty or new folder), then check every file hash.
pub fn extract_pack(pack: &Path, dest: &Path) -> Result<(), PackError> {
    let file = std::fs::File::open(pack)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| PackError::Corrupt)?;
    std::fs::create_dir_all(dest)?;
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).map_err(|_| PackError::Corrupt)?;
        let name = e.name().to_string();
        if e.is_dir() || e.enclosed_name().is_none() || !flat_name_ok(&name) {
            return Err(PackError::UnsafeEntry);
        }
        let declared_size = e.size();
        if declared_size > MAX_ENTRY_BYTES {
            return Err(PackError::TooLarge);
        }
        let mut out = std::fs::File::create(dest.join(&name))?;
        // Never trust the header size: copy at most the limit and compare.
        let copied = std::io::copy(&mut (&mut e).take(MAX_ENTRY_BYTES + 1), &mut out)
            .map_err(|_| PackError::Corrupt)?;
        if copied > MAX_ENTRY_BYTES {
            return Err(PackError::TooLarge);
        }
        out.sync_all()?;
    }
    let text = std::fs::read_to_string(dest.join("model.json"))?;
    Manifest::from_json(&text)?.verify_files(dest)?;
    Ok(())
}

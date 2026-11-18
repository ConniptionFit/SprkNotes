//! `model.json` schema 1 (SPEC 16.5): parsing, validation and file hash verification.
//! Models are data: nothing here knows about a specific model, engine or path.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path};

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Features {
    pub punctuation: bool,
    pub casing: bool,
    pub streaming: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmokeTest {
    pub fixture: String,
    pub expect_contains: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    /// `YYYY.MM.patch`, sorts correctly.
    pub version: String,
    pub display_name: String,
    /// Must match an adapter compiled into the app (SPEC 17.2).
    pub engine: String,
    pub languages: Vec<String>,
    pub sample_rate: u32,
    pub features: Features,
    pub accelerators: Vec<String>,
    pub min_app_version: String,
    /// Logical role (encoder, tokens, ...) to file name inside the model folder.
    pub files: BTreeMap<String, String>,
    /// File name to lowercase hex SHA-256.
    pub sha256: BTreeMap<String, String>,
    #[serde(default)]
    pub smoke_test: Option<SmokeTest>,
    pub license: String,
    pub source: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("model.json is not valid: {0}")]
    Parse(String),
    #[error("unsupported model.json schema {0} (this app reads schema {SCHEMA})")]
    Schema(u32),
    #[error("invalid manifest: {0}")]
    Invalid(String),
    #[error("unknown engine '{0}'")]
    UnknownEngine(String),
    #[error("model needs app {needs} or newer (this is {have})")]
    AppTooOld { needs: String, have: String },
    #[error("file '{0}' is missing from the model folder")]
    MissingFile(String),
    #[error("hash mismatch for '{0}'")]
    HashMismatch(String),
    #[error("io error reading '{0}'")]
    Io(String),
}

fn parse_semver(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.split('.').map(|p| p.parse::<u64>().ok());
    let r = (it.next()??, it.next()??, it.next()??);
    it.next().is_none().then_some(r)
}

/// `YYYY.MM.patch` with a plausible month.
fn valid_model_version(v: &str) -> bool {
    matches!(parse_semver(v), Some((y, m, _)) if (2000..=9999).contains(&y) && (1..=12).contains(&m))
}

fn safe_file_name(n: &str) -> bool {
    let mut c = Path::new(n).components();
    matches!((c.next(), c.next()), (Some(Component::Normal(_)), None))
}

impl Manifest {
    pub fn from_json(s: &str) -> Result<Self, ManifestError> {
        // Check the schema number first so a future schema gives a clear reason
        // instead of a confusing missing-field error.
        let v: serde_json::Value =
            serde_json::from_str(s).map_err(|e| ManifestError::Parse(e.to_string()))?;
        let schema = v.get("schema").and_then(|x| x.as_u64()).unwrap_or(0);
        if schema != u64::from(SCHEMA) {
            return Err(ManifestError::Schema(u32::try_from(schema).unwrap_or(0)));
        }
        let m: Manifest =
            serde_json::from_value(v).map_err(|e| ManifestError::Parse(e.to_string()))?;
        m.validate()?;
        Ok(m)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        let bad = |m: &str| Err(ManifestError::Invalid(m.into()));
        if self.id.is_empty()
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-._".contains(c))
        {
            return bad("id must be lowercase letters, digits, '-', '.', '_'");
        }
        if !valid_model_version(&self.version) {
            return bad("version must be YYYY.MM.patch");
        }
        if parse_semver(&self.min_app_version).is_none() {
            return bad("min_app_version must be MAJOR.MINOR.PATCH");
        }
        if self.engine.is_empty() || self.languages.is_empty() || self.sample_rate == 0 {
            return bad("engine, languages and sample_rate are required");
        }
        if self.files.is_empty() {
            return bad("files must not be empty");
        }
        for name in self.files.values() {
            if !safe_file_name(name) {
                return bad("file names must be plain names inside the model folder");
            }
            match self.sha256.get(name) {
                Some(h) if h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()) => {}
                _ => return bad("every file needs a 64-char hex sha256"),
            }
        }
        Ok(())
    }

    /// Rejects an engine this build has no adapter for, or a model that needs a newer app.
    pub fn check_compat(
        &self,
        supported_engines: &[&str],
        app_version: &str,
    ) -> Result<(), ManifestError> {
        if !supported_engines.contains(&self.engine.as_str()) {
            return Err(ManifestError::UnknownEngine(self.engine.clone()));
        }
        match (
            parse_semver(app_version),
            parse_semver(&self.min_app_version),
        ) {
            (Some(have), Some(need)) if have >= need => Ok(()),
            _ => Err(ManifestError::AppTooOld {
                needs: self.min_app_version.clone(),
                have: app_version.into(),
            }),
        }
    }

    /// Streams every declared file through SHA-256 and compares to the manifest.
    pub fn verify_files(&self, dir: &Path) -> Result<(), ManifestError> {
        for name in self.files.values() {
            let path = dir.join(name);
            let mut f =
                std::fs::File::open(&path).map_err(|_| ManifestError::MissingFile(name.clone()))?;
            let mut h = Sha256::new();
            let mut buf = [0u8; 64 * 1024];
            loop {
                let n = f
                    .read(&mut buf)
                    .map_err(|_| ManifestError::Io(name.clone()))?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            let got = hex::encode(h.finalize());
            let want = self.sha256.get(name).map(|s| s.to_ascii_lowercase());
            if want.as_deref() != Some(got.as_str()) {
                return Err(ManifestError::HashMismatch(name.clone()));
            }
        }
        Ok(())
    }

    /// Absolute path of a logical file (e.g. "encoder") inside `dir`.
    pub fn file_path(&self, dir: &Path, role: &str) -> Option<std::path::PathBuf> {
        self.files.get(role).map(|n| dir.join(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(hash: &str) -> String {
        format!(
            r#"{{"schema":1,"id":"test-model","version":"2027.01.0","display_name":"Test",
"engine":"test/fake","languages":["en"],"sample_rate":16000,
"features":{{"punctuation":true,"casing":true,"streaming":false}},
"accelerators":["cpu"],"min_app_version":"0.0.1",
"files":{{"tokens":"tokens.txt"}},"sha256":{{"tokens.txt":"{hash}"}},
"license":"MIT","source":"synthetic"}}"#
        )
    }
    const H: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"; // "hello"

    #[test]
    fn parses_valid() {
        let m = Manifest::from_json(&sample(H)).unwrap();
        assert_eq!(m.id, "test-model");
        assert!(m.smoke_test.is_none());
    }

    #[test]
    fn rejects_future_schema() {
        let s = sample(H).replace("\"schema\":1", "\"schema\":2");
        assert!(matches!(
            Manifest::from_json(&s),
            Err(ManifestError::Schema(2))
        ));
    }

    #[test]
    fn rejects_bad_version_and_traversal() {
        let s = sample(H).replace("2027.01.0", "2027.13.0");
        assert!(matches!(
            Manifest::from_json(&s),
            Err(ManifestError::Invalid(_))
        ));
        let s = sample(H).replace("\"tokens.txt\"", "\"../x\"");
        assert!(Manifest::from_json(&s).is_err());
    }

    #[test]
    fn compat_checks_engine_and_app_version() {
        let m = Manifest::from_json(&sample(H)).unwrap();
        assert!(m.check_compat(&["test/fake"], "0.0.1").is_ok());
        assert!(matches!(
            m.check_compat(&["other"], "0.0.1"),
            Err(ManifestError::UnknownEngine(_))
        ));
        let mut newer = m.clone();
        newer.min_app_version = "0.4.0".into();
        assert!(matches!(
            newer.check_compat(&["test/fake"], "0.0.1"),
            Err(ManifestError::AppTooOld { .. })
        ));
    }

    #[test]
    fn verifies_hashes_missing_and_tampered() {
        let dir = tempfile::tempdir().unwrap();
        let m = Manifest::from_json(&sample(H)).unwrap();
        assert!(matches!(
            m.verify_files(dir.path()),
            Err(ManifestError::MissingFile(_))
        ));
        std::fs::write(dir.path().join("tokens.txt"), "hello").unwrap();
        assert!(m.verify_files(dir.path()).is_ok());
        std::fs::write(dir.path().join("tokens.txt"), "hellO").unwrap();
        assert!(matches!(
            m.verify_files(dir.path()),
            Err(ManifestError::HashMismatch(_))
        ));
    }
}

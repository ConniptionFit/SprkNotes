//! Settings that shape a session, read from admin policy (SPEC 10.2). Same rules on every platform.

use sprk_audio::Source;
use sprk_config::AppPolicy;
use sprk_pipeline::commit::Config;
use std::path::PathBuf;

/// Commit buffer settings: the live text delay (1 to 30 seconds) and filler removal.
pub fn commit_config(p: &AppPolicy) -> Config {
    let mut c = Config::default();
    if let Some(s) = p.commit_delay_seconds {
        c.delay_ms = s.clamp(1, 30) * 1000;
    }
    if let Some(f) = p.filler_removal {
        c.drop_fillers = f;
    }
    c
}

/// Default transcripts folder: the Documents folder, or `~/Documents` when none is configured
/// (a fresh home on Linux has no user-dirs entry).
pub fn default_transcripts_dir(
    documents: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    let docs = documents.or_else(|| home.map(|h| h.join("Documents")))?;
    Some(docs.join("SprkNotes").join("Transcripts"))
}

/// The window names sources `mic` and `system`.
pub fn source_of(source: &str) -> Result<Source, String> {
    match source {
        "mic" => Ok(Source::Mic),
        "system" => Ok(Source::System),
        _ => Err("unknown source".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_delay_is_clamped() {
        let p = AppPolicy {
            commit_delay_seconds: Some(90),
            filler_removal: Some(false),
            ..Default::default()
        };
        let c = commit_config(&p);
        assert_eq!(c.delay_ms, 30_000);
        assert!(!c.drop_fillers);
        let p = AppPolicy {
            commit_delay_seconds: Some(0),
            ..Default::default()
        };
        assert_eq!(commit_config(&p).delay_ms, 1_000);
    }

    #[test]
    fn sources_by_name() {
        assert_eq!(source_of("mic"), Ok(Source::Mic));
        assert_eq!(source_of("system"), Ok(Source::System));
        assert!(source_of("camera").is_err());
    }
}

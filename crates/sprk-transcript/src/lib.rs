//! Transcript format `sprknotes-transcript/1`, crash-safe writer, naming and recovery
//! (SPEC 5 and 6). Pure std: no clock, no OS calls beyond files, fully testable on Linux.

pub mod export;
pub mod extras;
pub mod format;
pub mod library;
pub mod naming;
pub mod parse;
pub mod recovery;
pub mod retention;
pub mod writer;

pub const FORMAT: &str = "sprknotes-transcript/1";

/// Why transcript operations fail. Messages never contain transcript text or names.
#[derive(Debug, thiserror::Error)]
pub enum TranscriptError {
    #[error("file operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a sprknotes transcript")]
    NotATranscript,
}

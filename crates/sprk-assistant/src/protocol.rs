//! JSON lines between the app and the helper process (ADR-009). One object per line.
//! Prompts and replies are PHI-grade: they travel over the local pipe only and are never logged.
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// First message. The helper answers with `Hello`.
    Hello {
        version: u32,
    },
    /// Load the model from an installed pack folder.
    Load {
        model_dir: String,
        threads: u32,
        context_tokens: u32,
    },
    Generate {
        id: u64,
        prompt: String,
        max_tokens: u32,
        temperature: f32,
        #[serde(default)]
        stop: Vec<String>,
    },
    /// Pause or resume generation for request `id` between tokens (transcription is behind).
    Hold {
        id: u64,
    },
    Resume {
        id: u64,
    },
    Cancel {
        id: u64,
    },
    Unload,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    NotLoaded,
    LoadFailed,
    Cancelled,
    Failed,
    BadRequest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Response {
    Hello {
        version: u32,
    },
    Loaded {
        load_ms: u32,
    },
    Unloaded,
    /// A streamed piece of the reply.
    Text {
        id: u64,
        text: String,
    },
    /// Counts only.
    Done {
        id: u64,
        prompt_tokens: u32,
        output_tokens: u32,
        generate_ms: u32,
    },
    /// No text, no details: only the kind.
    Error {
        id: Option<u64>,
        kind: ErrorKind,
    },
}

impl Request {
    pub fn to_line(&self) -> String {
        // Serializing these plain types cannot fail; an empty line is rejected by the reader.
        serde_json::to_string(self).unwrap_or_default()
    }
    pub fn from_line(s: &str) -> Option<Self> {
        serde_json::from_str(s.trim()).ok()
    }
}

impl Response {
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
    pub fn from_line(s: &str) -> Option<Self> {
        serde_json::from_str(s.trim()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip_on_one_line() {
        let r = Request::Generate {
            id: 7,
            prompt: "line one\nline two".into(),
            max_tokens: 24,
            temperature: 0.2,
            stop: vec!["\n".into()],
        };
        let line = r.to_line();
        assert!(!line.contains('\n'), "newlines inside text are escaped");
        assert_eq!(Request::from_line(&line), Some(r));
    }

    #[test]
    fn responses_round_trip() {
        for r in [
            Response::Hello {
                version: PROTOCOL_VERSION,
            },
            Response::Loaded { load_ms: 4200 },
            Response::Text {
                id: 1,
                text: "Budget".into(),
            },
            Response::Done {
                id: 1,
                prompt_tokens: 900,
                output_tokens: 6,
                generate_ms: 800,
            },
            Response::Error {
                id: Some(1),
                kind: ErrorKind::Cancelled,
            },
        ] {
            assert_eq!(Response::from_line(&r.to_line()), Some(r));
        }
    }

    #[test]
    fn garbage_and_unknown_ops_are_rejected_not_panicking() {
        assert_eq!(Request::from_line("not json"), None);
        assert_eq!(Request::from_line(r#"{"op":"format_disk"}"#), None);
        assert_eq!(Response::from_line(""), None);
    }

    #[test]
    fn errors_carry_a_kind_and_nothing_else() {
        let line = Response::Error {
            id: None,
            kind: ErrorKind::LoadFailed,
        }
        .to_line();
        assert_eq!(line, r#"{"op":"error","id":null,"kind":"load_failed"}"#);
    }
}

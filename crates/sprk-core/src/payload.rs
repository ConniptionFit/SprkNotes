//! What the window receives (app/ui/src/lib/types.ts). One set of shapes for every shell, so the
//! shared UI components read the same events on a PC and a phone.

use serde::Serialize;
use sprk_pipeline::host::{UiCaption, UiLine, UiStatus};
use sprk_transcript::format::{event_kind, EventKind};

/// Event names, so both shells emit the same ones.
pub mod event {
    pub const LINE: &str = "transcript-line";
    pub const STATUS: &str = "session-status";
    pub const PROBLEM: &str = "session-problem";
    pub const DROPPED: &str = "transcript-dropped";
    pub const LEVELS: &str = "source-levels";
    pub const CAPTION: &str = "caption";
    pub const PHASE: &str = "session-phase";
    /// Model download progress ([`crate::download::Progress`]).
    pub const MODEL_DOWNLOAD: &str = "model-download";
}

/// A committed transcript line.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LinePayload {
    Statement {
        id: u64,
        t: String,
        speaker: String,
        text: String,
    },
    Event {
        id: u64,
        t: String,
        text: String,
    },
    /// A mark the user dropped (SPEC 5.1). In the file it is an event line.
    Mark {
        id: u64,
        t: String,
    },
    /// A note the user typed, without its "Note: " prefix.
    Note {
        id: u64,
        t: String,
        text: String,
    },
}

impl From<UiLine> for LinePayload {
    fn from(l: UiLine) -> Self {
        match l {
            UiLine::Statement {
                id,
                t,
                speaker,
                text,
            } => LinePayload::Statement {
                id,
                t,
                speaker,
                text,
            },
            UiLine::Event { id, t, text } => match event_kind(&text) {
                EventKind::Mark => LinePayload::Mark { id, t },
                EventKind::Note(n) => LinePayload::Note {
                    id,
                    t,
                    text: n.to_string(),
                },
                EventKind::Other => LinePayload::Event { id, t, text },
            },
        }
    }
}

/// Live caption payload (ADR-006). Transient: the UI shows it and forgets it.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct CaptionPayload {
    pub speaker: String,
    pub text: String,
    /// Leading words that are settled; the rest are tentative and may change.
    pub stable: usize,
    #[serde(rename = "final")]
    pub is_final: bool,
    /// Rows that locked since the last caption (ADR-008).
    pub sealed: Vec<String>,
    pub utt: u64,
    /// Where a finished utterance began, so its row shows a time before it commits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t: Option<String>,
}

impl From<&UiCaption> for CaptionPayload {
    fn from(c: &UiCaption) -> Self {
        CaptionPayload {
            speaker: c.speaker.to_string(),
            text: c.text.clone(),
            stable: c.stable,
            is_final: c.is_final,
            sealed: c.sealed.clone(),
            utt: c.utt,
            t: c.t.clone(),
        }
    }
}

/// A Me line shown live was dropped as microphone bleed (transient, never stored).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct DroppedPayload {
    pub speaker: String,
    pub text: String,
}

/// What the window is busy doing: `loading` (with the model's name) at Start, `ending` while the
/// transcript is closed off, `idle` otherwise.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Phase {
    pub phase: &'static str,
    pub model: Option<String>,
}

/// The session status as the window names it.
pub fn status_name(s: UiStatus) -> &'static str {
    match s {
        UiStatus::Ready => "ready",
        UiStatus::Transcribing => "transcribing",
        UiStatus::Paused => "paused",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_shape_matches_the_window() {
        let p: LinePayload = UiLine::Statement {
            id: 3,
            t: "00:00:04".into(),
            speaker: "Others".into(),
            text: "Hello".into(),
        }
        .into();
        assert_eq!(
            serde_json::to_value(p).ok(),
            Some(
                serde_json::json!({"kind":"statement","id":3,"t":"00:00:04","speaker":"Others","text":"Hello"})
            )
        );
    }

    #[test]
    fn caption_uses_final_and_skips_missing_time() {
        let c = UiCaption {
            speaker: "Others",
            text: "Hi there".into(),
            stable: 1,
            is_final: false,
            sealed: vec![],
            utt: 7,
            t: None,
        };
        let v = serde_json::to_value(CaptionPayload::from(&c)).ok();
        assert_eq!(
            v,
            Some(
                serde_json::json!({"speaker":"Others","text":"Hi there","stable":1,"final":false,"sealed":[],"utt":7})
            )
        );
    }

    #[test]
    fn status_names() {
        assert_eq!(status_name(UiStatus::Transcribing), "transcribing");
    }
}

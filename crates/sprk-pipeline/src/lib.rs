//! Session pipeline logic: statement rules, bleed dedupe and the commit buffer
//! (SPEC 4.2 to 4.4). Pure and deterministic: time is passed in, nothing reads a clock.

pub mod caption;
pub mod commit;
pub mod dedupe;
pub mod host;
pub mod rules;
pub mod session;

/// Speaker label. Remote speakers are `Others` unless a confirmed 1:1 call names them
/// (SPEC 19.4); when unsure, never guess a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Speaker {
    Me,
    Others,
    Named(String),
}

impl Speaker {
    pub fn label(&self) -> &str {
        match self {
            Speaker::Me => "Me",
            Speaker::Others => "Others",
            Speaker::Named(n) => n,
        }
    }

    /// Remote speakers come from the system (loopback) stream.
    pub fn is_remote(&self) -> bool {
        !matches!(self, Speaker::Me)
    }
}

/// A finalized ASR utterance for one stream.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub speaker: Speaker,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_remote_flag() {
        assert_eq!(Speaker::Me.label(), "Me");
        assert!(!Speaker::Me.is_remote());
        assert!(Speaker::Others.is_remote());
        assert!(Speaker::Named("Sam".into()).is_remote());
    }
}

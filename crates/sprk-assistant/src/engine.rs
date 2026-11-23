//! The engine seam. A real adapter (llama.cpp, ADR-009) lives in the helper process; the app and
//! all feature logic only see this trait, so they test on Linux with [`FakeEngine`].
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    #[error("the assistant model is not loaded")]
    NotLoaded,
    #[error("the assistant model could not be loaded")]
    LoadFailed,
    #[error("generation was cancelled")]
    Cancelled,
    #[error("the assistant engine failed")]
    Failed,
}

/// What the engine should do between tokens. The app flips it from the lag monitor (ADR-009).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Run,
    /// Pause between tokens (transcription is behind). The engine waits and polls again.
    Hold,
    /// Stop now and return [`EngineError::Cancelled`].
    Cancel,
}

pub trait RunControl: Send + Sync {
    fn state(&self) -> Control;
}

/// A shared control cell: clone it into the engine and keep one handle to drive it.
#[derive(Debug, Clone, Default)]
pub struct SharedControl(Arc<AtomicU8>);

impl SharedControl {
    pub fn set(&self, c: Control) {
        let v = match c {
            Control::Run => 0,
            Control::Hold => 1,
            Control::Cancel => 2,
        };
        self.0.store(v, Ordering::Relaxed);
    }
}

impl RunControl for SharedControl {
    fn state(&self) -> Control {
        match self.0.load(Ordering::Relaxed) {
            1 => Control::Hold,
            2 => Control::Cancel,
            _ => Control::Run,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenRequest {
    /// The full prompt, already in the pack's prompt format.
    pub prompt: String,
    pub max_tokens: u32,
    pub temperature: f32,
    /// Generation stops (and the stop text is dropped) when one of these appears.
    pub stop: Vec<String>,
}

/// Counts only. Never text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GenStats {
    pub prompt_tokens: u32,
    pub output_tokens: u32,
    pub load_ms: u32,
    pub generate_ms: u32,
}

pub trait AssistantEngine {
    fn is_loaded(&self) -> bool;
    fn load(&mut self) -> Result<(), EngineError>;
    fn unload(&mut self);
    /// Runs one request. `on_text` receives pieces as they are produced. The engine checks
    /// `control` between tokens.
    fn generate(
        &mut self,
        req: &GenRequest,
        control: &dyn RunControl,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<GenStats, EngineError>;
}

/// Scripted engine for tests: answers each request with the next canned reply, in pieces of
/// `piece_chars` characters, and records every prompt it was given.
#[derive(Debug, Default)]
pub struct FakeEngine {
    pub replies: Vec<String>,
    pub piece_chars: usize,
    pub loaded: bool,
    pub fail_load: bool,
    /// Sleep between pieces, to let tests hold or cancel a request in flight.
    pub piece_delay_ms: u64,
    pub seen_prompts: Vec<String>,
    next: usize,
}

impl FakeEngine {
    /// An engine whose model never loads.
    pub fn failing_load() -> Self {
        Self {
            fail_load: true,
            ..Self::default()
        }
    }

    pub fn with_replies(replies: &[&str]) -> Self {
        Self {
            replies: replies.iter().map(|s| (*s).to_string()).collect(),
            piece_chars: 4,
            ..Self::default()
        }
    }
}

impl AssistantEngine for FakeEngine {
    fn is_loaded(&self) -> bool {
        self.loaded
    }

    fn load(&mut self) -> Result<(), EngineError> {
        if self.fail_load {
            return Err(EngineError::LoadFailed);
        }
        self.loaded = true;
        Ok(())
    }

    fn unload(&mut self) {
        self.loaded = false;
    }

    fn generate(
        &mut self,
        req: &GenRequest,
        control: &dyn RunControl,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<GenStats, EngineError> {
        if !self.loaded {
            return Err(EngineError::NotLoaded);
        }
        self.seen_prompts.push(req.prompt.clone());
        let reply = self.replies.get(self.next).cloned().unwrap_or_default();
        self.next += 1;
        let chars: Vec<char> = reply.chars().collect();
        let step = self.piece_chars.max(1);
        let mut out = String::new();
        let mut produced = 0u32;
        for piece in chars.chunks(step) {
            // Like a real engine: wait while held, stop when cancelled.
            loop {
                match control.state() {
                    Control::Cancel => return Err(EngineError::Cancelled),
                    Control::Hold => std::thread::sleep(std::time::Duration::from_millis(5)),
                    Control::Run => break,
                }
            }
            if self.piece_delay_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(self.piece_delay_ms));
            }
            let text: String = piece.iter().collect();
            out.push_str(&text);
            if let Some(cut) = req.stop.iter().filter_map(|s| out.find(s.as_str())).min() {
                let already = out.len() - text.len();
                if cut > already {
                    on_text(&out[already..cut]);
                }
                produced += 1;
                break;
            }
            on_text(&text);
            produced += 1;
            if produced >= req.max_tokens {
                break;
            }
        }
        Ok(GenStats {
            prompt_tokens: u32::try_from(req.prompt.len() / 4).unwrap_or(u32::MAX),
            output_tokens: produced,
            ..GenStats::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(stop: &[&str], max: u32) -> GenRequest {
        GenRequest {
            prompt: "p".into(),
            max_tokens: max,
            temperature: 0.1,
            stop: stop.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    fn run(
        e: &mut FakeEngine,
        r: &GenRequest,
        c: &dyn RunControl,
    ) -> (String, Result<GenStats, EngineError>) {
        let mut text = String::new();
        let res = e.generate(r, c, &mut |t| text.push_str(t));
        (text, res)
    }

    #[test]
    fn needs_load_first_and_streams_pieces() {
        let mut e = FakeEngine::with_replies(&["Hello there world"]);
        let c = SharedControl::default();
        assert_eq!(
            run(&mut e, &req(&[], 100), &c).1,
            Err(EngineError::NotLoaded)
        );
        e.load().unwrap();
        let (text, res) = run(&mut e, &req(&[], 100), &c);
        assert_eq!(text, "Hello there world");
        assert!(res.unwrap().output_tokens >= 4);
    }

    #[test]
    fn stop_text_and_max_tokens_end_generation() {
        let mut e = FakeEngine::with_replies(&["Weekly sync\nextra", "abcdefghijkl"]);
        e.load().unwrap();
        let c = SharedControl::default();
        assert_eq!(run(&mut e, &req(&["\n"], 100), &c).0, "Weekly sync");
        assert_eq!(run(&mut e, &req(&[], 2), &c).0, "abcdefgh");
    }

    #[test]
    fn cancel_stops_generation() {
        let mut e = FakeEngine::with_replies(&["a long answer here"]);
        e.load().unwrap();
        let c = SharedControl::default();
        c.set(Control::Cancel);
        assert_eq!(
            run(&mut e, &req(&[], 100), &c).1,
            Err(EngineError::Cancelled)
        );
    }

    #[test]
    fn failed_load_is_reported_without_details() {
        let mut e = FakeEngine::failing_load();
        assert_eq!(e.load(), Err(EngineError::LoadFailed));
        assert!(!e.is_loaded());
    }
}

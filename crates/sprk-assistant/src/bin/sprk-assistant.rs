//! The assistant helper process (ADR-009). The app starts it at low priority and talks to it
//! over stdin and stdout (JSON lines). It has no network code and writes nothing to disk.
//!
//! Real engines are added as adapters here (the llama.cpp adapter arrives with the benchmark
//! spike). Until one is compiled in, every `Load` answers `load_failed`, which the app treats
//! as "assistant not available" and carries on.
use sprk_assistant::engine::{AssistantEngine, EngineError};
use sprk_assistant::host::{serve, Factory, LoadParams};
use std::io::{stdin, stdout, BufReader};

#[cfg(feature = "fake-engine")]
fn factory() -> Box<Factory> {
    // Test builds only: SPRK_ASSISTANT_FAKE_REPLIES is a JSON array of canned replies.
    Box::new(|_p: &LoadParams| {
        let raw = std::env::var("SPRK_ASSISTANT_FAKE_REPLIES").unwrap_or_else(|_| "[]".into());
        let replies: Vec<String> =
            serde_json::from_str(&raw).map_err(|_| EngineError::LoadFailed)?;
        let refs: Vec<&str> = replies.iter().map(String::as_str).collect();
        Ok(
            Box::new(sprk_assistant::engine::FakeEngine::with_replies(&refs))
                as Box<dyn AssistantEngine>,
        )
    })
}

#[cfg(not(feature = "fake-engine"))]
fn factory() -> Box<Factory> {
    Box::new(
        |_p: &LoadParams| -> Result<Box<dyn AssistantEngine>, EngineError> {
            Err(EngineError::LoadFailed)
        },
    )
}

fn main() {
    serve(BufReader::new(stdin()), stdout(), factory());
}

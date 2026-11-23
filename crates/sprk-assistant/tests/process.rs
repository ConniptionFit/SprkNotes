//! Starts the real helper binary (built with the scripted engine) and talks to it.
#![cfg(feature = "fake-engine")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use sprk_assistant::client::RemoteEngine;
use sprk_assistant::engine::{AssistantEngine, EngineError, GenRequest, SharedControl};
use sprk_assistant::host::LoadParams;
use std::path::Path;

fn params() -> LoadParams {
    LoadParams {
        model_dir: "unused".into(),
        threads: 1,
        context_tokens: 2048,
    }
}

#[test]
fn the_helper_process_answers_and_exits_when_dropped() {
    std::env::set_var("SPRK_ASSISTANT_FAKE_REPLIES", r#"["Budget review"]"#);
    let mut e =
        RemoteEngine::spawn(Path::new(env!("CARGO_BIN_EXE_sprk-assistant")), params()).unwrap();
    e.load().unwrap();
    let c = SharedControl::default();
    let mut got = String::new();
    let r = GenRequest {
        prompt: "p".into(),
        max_tokens: 50,
        temperature: 0.1,
        stop: vec![],
    };
    let stats = e.generate(&r, &c, &mut |t| got.push_str(t)).unwrap();
    assert_eq!(got, "Budget review");
    assert!(stats.output_tokens > 0);
    e.unload();
    drop(e); // must return promptly: Shutdown, then the process ends
}

#[test]
fn a_missing_helper_is_a_plain_failure() {
    let r = RemoteEngine::spawn(Path::new("/definitely/not/here"), params());
    assert!(matches!(r, Err(EngineError::Failed)));
}

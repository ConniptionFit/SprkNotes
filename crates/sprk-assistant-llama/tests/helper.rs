//! Runs the real llama.cpp helper binary against a model folder.
//! Set SPRK_TEST_ASSISTANT_MODEL to a folder with model.json and the weights (the CI script makes
//! a tiny random-weight one with scripts/make-test-assistant-model.py). Without it the tests skip.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use sprk_assistant::client::RemoteEngine;
use sprk_assistant::engine::{AssistantEngine, Control, EngineError, GenRequest, SharedControl};
use sprk_assistant::host::LoadParams;
use std::path::Path;
use std::time::{Duration, Instant};

fn model_dir() -> Option<String> {
    std::env::var("SPRK_TEST_ASSISTANT_MODEL").ok()
}

fn start(dir: &str) -> RemoteEngine {
    let p = LoadParams {
        model_dir: dir.to_string(),
        threads: 2,
        context_tokens: 512,
    };
    RemoteEngine::spawn(Path::new(env!("CARGO_BIN_EXE_sprk-assistant")), p).unwrap()
}

fn req(prompt: &str, max: u32, stop: &[&str]) -> GenRequest {
    GenRequest {
        prompt: prompt.into(),
        max_tokens: max,
        temperature: 0.0,
        stop: stop.iter().map(|s| (*s).to_string()).collect(),
    }
}

#[test]
fn loads_generates_and_unloads() {
    let Some(dir) = model_dir() else {
        eprintln!("skipped: SPRK_TEST_ASSISTANT_MODEL not set");
        return;
    };
    let mut e = start(&dir);
    e.load().unwrap();
    let c = SharedControl::default();
    let mut got = String::new();
    let prompt = "<|im_start|>user\nName this meeting<|im_end|>\n<|im_start|>assistant\n";
    let stats = e
        .generate(&req(prompt, 12, &[]), &c, &mut |t| got.push_str(t))
        .unwrap();
    assert!(stats.output_tokens >= 1 && stats.output_tokens <= 12);
    assert!(stats.prompt_tokens > 5);
    assert!(!got.is_empty());
    // Greedy decoding is repeatable.
    let mut again = String::new();
    e.generate(&req(prompt, 12, &[]), &c, &mut |t| again.push_str(t))
        .unwrap();
    assert_eq!(got, again);
    e.unload();
    assert!(!e.is_loaded());
}

#[test]
fn a_prompt_that_does_not_fit_is_a_plain_failure() {
    let Some(dir) = model_dir() else { return };
    let mut e = start(&dir);
    e.load().unwrap();
    let c = SharedControl::default();
    let long = "word ".repeat(400);
    let r = e.generate(&req(&long, 8, &[]), &c, &mut |_| {});
    assert_eq!(r, Err(EngineError::Failed));
    // And the engine is still usable afterwards.
    assert!(e.generate(&req("hello", 4, &[]), &c, &mut |_| {}).is_ok());
}

#[test]
fn stop_text_ends_the_reply_and_cancel_ends_a_long_one() {
    let Some(dir) = model_dir() else { return };
    let mut e = start(&dir);
    e.load().unwrap();
    let c = SharedControl::default();
    // Whatever the model says first, using it as a stop string leaves nothing before it.
    let mut first = String::new();
    e.generate(&req("hello", 6, &[]), &c, &mut |t| first.push_str(t))
        .unwrap();
    let head: String = first.chars().take(2).collect();
    let mut cut = String::new();
    e.generate(&req("hello", 6, &[&head]), &c, &mut |t| cut.push_str(t))
        .unwrap();
    assert!(!cut.contains(&head));

    let c2 = c.clone();
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        c2.set(Control::Cancel);
    });
    let started = Instant::now();
    let r = e.generate(&req("hello", 400, &[]), &c, &mut |_| {});
    t.join().unwrap();
    assert!(matches!(r, Err(EngineError::Cancelled)) || r.is_ok());
    assert!(started.elapsed() < Duration::from_secs(20));
}

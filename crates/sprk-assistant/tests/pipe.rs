//! The app and the helper talk over a pipe pair (in-process here, two processes in the app).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use sprk_assistant::client::RemoteEngine;
use sprk_assistant::engine::{
    AssistantEngine, Control, EngineError, FakeEngine, GenRequest, SharedControl,
};
use sprk_assistant::host::{serve, Factory, LoadParams};
use sprk_assistant::prompt::PromptFormat;
use sprk_assistant::title::suggest_title;
use sprk_assistant::view::Line;
use std::io::{pipe, BufReader};
use std::thread::{self, JoinHandle};
use std::time::Duration;

fn params() -> LoadParams {
    LoadParams {
        model_dir: "models/x".into(),
        threads: 2,
        context_tokens: 4096,
    }
}

fn start(factory: Box<Factory>) -> (RemoteEngine, JoinHandle<()>) {
    let (app_to_host_r, app_to_host_w) = pipe().unwrap();
    let (host_to_app_r, host_to_app_w) = pipe().unwrap();
    let host = thread::spawn(move || serve(BufReader::new(app_to_host_r), host_to_app_w, factory));
    let engine =
        RemoteEngine::from_streams(BufReader::new(host_to_app_r), app_to_host_w, params()).unwrap();
    (engine, host)
}

fn fake(replies: &[&str], delay_ms: u64) -> Box<Factory> {
    let replies: Vec<String> = replies.iter().map(|s| (*s).to_string()).collect();
    Box::new(move |_p: &LoadParams| {
        let mut e =
            FakeEngine::with_replies(&replies.iter().map(String::as_str).collect::<Vec<_>>());
        e.piece_delay_ms = delay_ms;
        Ok(Box::new(e) as Box<dyn AssistantEngine>)
    })
}

fn req(max: u32) -> GenRequest {
    GenRequest {
        prompt: "p".into(),
        max_tokens: max,
        temperature: 0.1,
        stop: vec![],
    }
}

fn meeting() -> Vec<Line> {
    [
        "We need to review the quarterly budget before Friday.",
        "Finance wants the vendor invoices reconciled first.",
        "I will send the reconciled invoices tomorrow morning.",
        "Does the budget include the new laptop purchase order?",
        "Yes, the laptop purchase order is in the second tab.",
        "Great, then the budget review is ready for approval.",
    ]
    .iter()
    .enumerate()
    .map(|(i, t)| Line {
        id: i as u64 + 1,
        t_ms: (i as u64 + 1) * 20_000,
        speaker: "Me".into(),
        text: (*t).into(),
    })
    .collect()
}

#[test]
fn a_title_comes_back_through_the_pipe() {
    let (mut engine, host) = start(fake(&["Quarterly budget review"], 0));
    let c = SharedControl::default();
    let t = suggest_title(&mut engine, &c, PromptFormat::ChatMl, &meeting(), &[]).unwrap();
    assert_eq!(t.as_deref(), Some("Quarterly budget review"));
    engine.unload();
    assert!(!engine.is_loaded());
    drop(engine);
    host.join().unwrap();
}

#[test]
fn generate_before_load_and_a_failed_load_report_plain_errors() {
    let (mut engine, host) = start(Box::new(|_p: &LoadParams| Err(EngineError::LoadFailed)));
    let c = SharedControl::default();
    let mut sink = |_t: &str| {};
    assert_eq!(
        engine.generate(&req(8), &c, &mut sink),
        Err(EngineError::NotLoaded)
    );
    assert_eq!(engine.load(), Err(EngineError::LoadFailed));
    drop(engine);
    host.join().unwrap();
}

#[test]
fn cancel_stops_a_running_request_and_the_engine_stays_usable() {
    let (mut engine, host) = start(fake(&["a long reply that takes a while", "ok"], 20));
    engine.load().unwrap();
    let c = SharedControl::default();
    let c2 = c.clone();
    let canceller = thread::spawn(move || {
        thread::sleep(Duration::from_millis(80));
        c2.set(Control::Cancel);
    });
    let mut got = String::new();
    let r = engine.generate(&req(100), &c, &mut |t| got.push_str(t));
    canceller.join().unwrap();
    assert_eq!(r, Err(EngineError::Cancelled));
    assert!(got.len() < "a long reply that takes a while".len());

    // The next request runs normally.
    c.set(Control::Run);
    let mut got = String::new();
    engine
        .generate(&req(100), &c, &mut |t| got.push_str(t))
        .unwrap();
    assert_eq!(got, "ok");
    drop(engine);
    host.join().unwrap();
}

#[test]
fn hold_pauses_output_and_resume_finishes_the_reply() {
    let (mut engine, host) = start(fake(&["abcdefghijklmnop"], 15));
    engine.load().unwrap();
    let c = SharedControl::default();
    let c2 = c.clone();
    let driver = thread::spawn(move || {
        thread::sleep(Duration::from_millis(40));
        c2.set(Control::Hold);
        thread::sleep(Duration::from_millis(400));
        c2.set(Control::Run);
    });
    let started = std::time::Instant::now();
    let mut got = String::new();
    engine
        .generate(&req(100), &c, &mut |t| got.push_str(t))
        .unwrap();
    driver.join().unwrap();
    assert_eq!(got, "abcdefghijklmnop");
    assert!(
        started.elapsed() >= Duration::from_millis(400),
        "the hold delayed the reply"
    );
    drop(engine);
    host.join().unwrap();
}

#[test]
fn after_unload_the_next_request_reports_not_loaded() {
    let (mut engine, host) = start(fake(&["x"], 0));
    engine.load().unwrap();
    engine.unload();
    let c = SharedControl::default();
    let mut sink = |_t: &str| {};
    assert_eq!(
        engine.generate(&req(8), &c, &mut sink),
        Err(EngineError::NotLoaded)
    );
    drop(engine);
    host.join().unwrap();
}

#[test]
fn a_dead_helper_is_a_failed_request_not_a_hang() {
    let (mut engine, host) = start(Box::new(|_p: &LoadParams| -> Result<_, EngineError> {
        panic!("simulated helper crash")
    }));
    assert_eq!(engine.load(), Err(EngineError::Failed));
    assert!(!engine.is_loaded());
    let c = SharedControl::default();
    let mut sink = |_t: &str| {};
    assert!(engine.generate(&req(8), &c, &mut sink).is_err());
    drop(engine);
    assert!(host.join().is_err(), "the helper thread panicked");
}

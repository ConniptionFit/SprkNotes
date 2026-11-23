//! The helper side of the pipe (ADR-009): reads requests, drives an engine, writes responses.
//! It is generic over the streams and the engine factory, so it runs in-process in tests and as
//! the `sprk-assistant` helper binary in the app. It never logs and never keeps request text.
use crate::engine::{AssistantEngine, Control, EngineError, GenRequest, RunControl, SharedControl};
use crate::protocol::{ErrorKind, Request, Response, PROTOCOL_VERSION};
use std::io::{BufRead, Write};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

/// What the app asked for in `Load`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadParams {
    pub model_dir: String,
    pub threads: u32,
    pub context_tokens: u32,
}

pub type Factory = dyn FnMut(&LoadParams) -> Result<Box<dyn AssistantEngine>, EngineError> + Send;

fn kind_of(e: &EngineError) -> ErrorKind {
    match e {
        EngineError::NotLoaded => ErrorKind::NotLoaded,
        EngineError::LoadFailed => ErrorKind::LoadFailed,
        EngineError::Cancelled => ErrorKind::Cancelled,
        EngineError::Failed => ErrorKind::Failed,
    }
}

/// Shared so the reader thread can change run state while the worker is mid-generation.
struct Gate {
    control: SharedControl,
    /// The id the current Hold/Resume/Cancel messages apply to.
    current: Mutex<Option<u64>>,
}

struct GateControl(Arc<Gate>);
impl RunControl for GateControl {
    fn state(&self) -> Control {
        self.0.control.state()
    }
}

/// Serves requests until `Shutdown` or end of input. Returns after the engine is unloaded.
pub fn serve<R, W>(input: R, output: W, mut factory: Box<Factory>)
where
    R: BufRead + Send + 'static,
    W: Write + Send,
{
    let gate = Arc::new(Gate {
        control: SharedControl::default(),
        current: Mutex::new(None),
    });
    let (tx, rx) = mpsc::channel::<Request>();
    let reader_gate = Arc::clone(&gate);
    // The reader thread turns Hold/Resume/Cancel into state changes right away, even while the
    // worker is busy generating; everything else is queued for the worker.
    let reader = thread::spawn(move || {
        for line in input.lines() {
            let Ok(line) = line else { break };
            let Some(req) = Request::from_line(&line) else {
                continue;
            };
            match &req {
                Request::Hold { id } | Request::Resume { id } | Request::Cancel { id } => {
                    let current = reader_gate.current.lock().map(|g| *g).unwrap_or(None);
                    if current == Some(*id) {
                        reader_gate.control.set(match req {
                            Request::Hold { .. } => Control::Hold,
                            Request::Resume { .. } => Control::Run,
                            _ => Control::Cancel,
                        });
                    }
                }
                Request::Shutdown => {
                    let _ = tx.send(req);
                    return;
                }
                _ => {
                    if tx.send(req).is_err() {
                        return;
                    }
                }
            }
        }
        let _ = tx.send(Request::Shutdown);
    });

    let out = Mutex::new(output);
    let send = |r: Response| {
        if let Ok(mut o) = out.lock() {
            let _ = writeln!(o, "{}", r.to_line());
            let _ = o.flush();
        }
    };
    let mut engine: Option<Box<dyn AssistantEngine>> = None;
    for req in rx {
        match req {
            Request::Hello { .. } => send(Response::Hello {
                version: PROTOCOL_VERSION,
            }),
            Request::Load {
                model_dir,
                threads,
                context_tokens,
            } => {
                let started = std::time::Instant::now();
                let params = LoadParams {
                    model_dir,
                    threads,
                    context_tokens,
                };
                let loaded = factory(&params).and_then(|mut e| e.load().map(|()| e));
                match loaded {
                    Ok(e) => {
                        engine = Some(e);
                        let ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
                        send(Response::Loaded { load_ms: ms });
                    }
                    Err(e) => send(Response::Error {
                        id: None,
                        kind: kind_of(&e),
                    }),
                }
            }
            Request::Generate {
                id,
                prompt,
                max_tokens,
                temperature,
                stop,
            } => {
                let Some(e) = engine.as_mut() else {
                    send(Response::Error {
                        id: Some(id),
                        kind: ErrorKind::NotLoaded,
                    });
                    continue;
                };
                gate.control.set(Control::Run);
                if let Ok(mut c) = gate.current.lock() {
                    *c = Some(id);
                }
                let req = GenRequest {
                    prompt,
                    max_tokens,
                    temperature,
                    stop,
                };
                let started = std::time::Instant::now();
                let result = e.generate(&req, &GateControl(Arc::clone(&gate)), &mut |t| {
                    send(Response::Text {
                        id,
                        text: t.to_string(),
                    });
                });
                if let Ok(mut c) = gate.current.lock() {
                    *c = None;
                }
                match result {
                    Ok(s) => send(Response::Done {
                        id,
                        prompt_tokens: s.prompt_tokens,
                        output_tokens: s.output_tokens,
                        generate_ms: u32::try_from(started.elapsed().as_millis())
                            .unwrap_or(u32::MAX),
                    }),
                    Err(err) => send(Response::Error {
                        id: Some(id),
                        kind: kind_of(&err),
                    }),
                }
            }
            Request::Unload => {
                if let Some(mut e) = engine.take() {
                    e.unload();
                }
                send(Response::Unloaded);
            }
            Request::Shutdown => break,
            // Handled by the reader thread; stale ones are ignored.
            Request::Hold { .. } | Request::Resume { .. } | Request::Cancel { .. } => {}
        }
    }
    if let Some(mut e) = engine.take() {
        e.unload();
    }
    let _ = reader.join();
}

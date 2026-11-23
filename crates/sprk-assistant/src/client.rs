//! The app side of the pipe (ADR-009): an [`AssistantEngine`] that talks to the helper process.
//! Feature code only sees the trait, so titles, recaps and Q&A never know a process is involved.
//! Hold, resume and cancel follow the caller's [`RunControl`] while a request is running.
use crate::engine::{AssistantEngine, Control, EngineError, GenRequest, GenStats, RunControl};
use crate::host::LoadParams;
use crate::protocol::{ErrorKind, Request, Response, PROTOCOL_VERSION};
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
const LOAD_TIMEOUT: Duration = Duration::from_secs(180);
const POLL: Duration = Duration::from_millis(50);

pub struct RemoteEngine {
    tx: Box<dyn Write + Send>,
    rx: Receiver<Response>,
    params: LoadParams,
    loaded: bool,
    next_id: u64,
    /// Longest a single request may run before it is cancelled.
    pub request_timeout: Duration,
    child: Option<Child>,
}

fn from_kind(k: ErrorKind) -> EngineError {
    match k {
        ErrorKind::NotLoaded => EngineError::NotLoaded,
        ErrorKind::LoadFailed => EngineError::LoadFailed,
        ErrorKind::Cancelled => EngineError::Cancelled,
        ErrorKind::Failed | ErrorKind::BadRequest => EngineError::Failed,
    }
}

impl RemoteEngine {
    /// Connects over any pair of streams (a child's pipes, or in-memory pipes in tests) and
    /// checks the protocol version.
    pub fn from_streams<R, W>(reader: R, writer: W, params: LoadParams) -> Result<Self, EngineError>
    where
        R: BufRead + Send + 'static,
        W: Write + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if let Some(r) = Response::from_line(&line) {
                    if tx.send(r).is_err() {
                        break;
                    }
                }
            }
        });
        let mut me = Self {
            tx: Box::new(writer),
            rx,
            params,
            loaded: false,
            next_id: 1,
            request_timeout: Duration::from_secs(120),
            child: None,
        };
        me.send(&Request::Hello {
            version: PROTOCOL_VERSION,
        })?;
        match me.rx.recv_timeout(HELLO_TIMEOUT) {
            Ok(Response::Hello { version }) if version == PROTOCOL_VERSION => Ok(me),
            _ => Err(EngineError::Failed),
        }
    }

    /// Starts the helper program at low priority and connects to it.
    pub fn spawn(helper: &Path, params: LoadParams) -> Result<Self, EngineError> {
        let mut cmd = low_priority_command(helper);
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| EngineError::Failed)?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            return Err(EngineError::Failed);
        };
        match Self::from_streams(std::io::BufReader::new(stdout), stdin, params) {
            Ok(mut me) => {
                me.child = Some(child);
                Ok(me)
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(e)
            }
        }
    }

    fn send(&mut self, r: &Request) -> Result<(), EngineError> {
        writeln!(self.tx, "{}", r.to_line()).map_err(|_| EngineError::Failed)?;
        self.tx.flush().map_err(|_| EngineError::Failed)
    }

    fn wait_for<T>(
        &mut self,
        timeout: Duration,
        mut pick: impl FnMut(&Response) -> Option<Result<T, EngineError>>,
    ) -> Result<T, EngineError> {
        let end = Instant::now() + timeout;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(r) => {
                    if let Some(out) = pick(&r) {
                        return out;
                    }
                }
                Err(RecvTimeoutError::Timeout) => return Err(EngineError::Failed),
                Err(RecvTimeoutError::Disconnected) => {
                    self.loaded = false;
                    return Err(EngineError::Failed);
                }
            }
        }
    }
}

#[cfg(windows)]
fn low_priority_command(helper: &Path) -> Command {
    use std::os::windows::process::CommandExt;
    const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut c = Command::new(helper);
    c.creation_flags(BELOW_NORMAL_PRIORITY_CLASS | CREATE_NO_WINDOW);
    c
}

#[cfg(not(windows))]
fn low_priority_command(helper: &Path) -> Command {
    // `nice` is part of coreutils; without it the helper simply runs at normal priority.
    let has_nice = ["/usr/bin/nice", "/bin/nice"]
        .iter()
        .any(|p| Path::new(p).exists());
    if has_nice {
        let mut c = Command::new("nice");
        c.arg("-n").arg("10").arg(helper);
        c
    } else {
        Command::new(helper)
    }
}

impl AssistantEngine for RemoteEngine {
    fn is_loaded(&self) -> bool {
        self.loaded
    }

    fn load(&mut self) -> Result<(), EngineError> {
        let p = self.params.clone();
        self.send(&Request::Load {
            model_dir: p.model_dir,
            threads: p.threads,
            context_tokens: p.context_tokens,
        })?;
        self.wait_for(LOAD_TIMEOUT, |r| match r {
            Response::Loaded { .. } => Some(Ok(())),
            Response::Error { id: None, kind } => Some(Err(from_kind(*kind))),
            _ => None,
        })?;
        self.loaded = true;
        Ok(())
    }

    fn unload(&mut self) {
        if self.send(&Request::Unload).is_ok() {
            let _ = self.wait_for(Duration::from_secs(10), |r| {
                matches!(r, Response::Unloaded).then_some(Ok(()))
            });
        }
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
        let id = self.next_id;
        self.next_id += 1;
        self.send(&Request::Generate {
            id,
            prompt: req.prompt.clone(),
            max_tokens: req.max_tokens,
            temperature: req.temperature,
            stop: req.stop.clone(),
        })?;
        let started = Instant::now();
        let mut sent = Control::Run;
        let mut cancel_sent = false;
        loop {
            // Mirror the caller's run control to the helper whenever it changes.
            let want = control.state();
            if want != sent && !cancel_sent {
                let msg = match want {
                    Control::Run => Request::Resume { id },
                    Control::Hold => Request::Hold { id },
                    Control::Cancel => Request::Cancel { id },
                };
                self.send(&msg)?;
                cancel_sent = want == Control::Cancel;
                sent = want;
            }
            // A held request does not count against the time limit.
            if want != Control::Hold && !cancel_sent && started.elapsed() > self.request_timeout {
                self.send(&Request::Cancel { id })?;
                cancel_sent = true;
            }
            match self.rx.recv_timeout(POLL) {
                Ok(Response::Text { id: rid, text }) if rid == id => on_text(&text),
                Ok(Response::Done {
                    id: rid,
                    prompt_tokens,
                    output_tokens,
                    generate_ms,
                }) if rid == id => {
                    return Ok(GenStats {
                        prompt_tokens,
                        output_tokens,
                        load_ms: 0,
                        generate_ms,
                    });
                }
                Ok(Response::Error { id: rid, kind }) if rid == Some(id) => {
                    return Err(from_kind(kind));
                }
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    self.loaded = false;
                    return Err(EngineError::Failed);
                }
            }
        }
    }
}

impl Drop for RemoteEngine {
    fn drop(&mut self) {
        let _ = self.send(&Request::Shutdown);
        if let Some(mut c) = self.child.take() {
            let end = Instant::now() + Duration::from_secs(2);
            while Instant::now() < end {
                if matches!(c.try_wait(), Ok(Some(_))) {
                    return;
                }
                thread::sleep(Duration::from_millis(20));
            }
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

//! Glue between the app and the on-device assistant (ADR-009). All logic lives in the portable
//! `sprk-assistant` crate; this file only feeds it committed lines, maps results to the UI, and
//! starts the helper process. Prompts, questions, answers and titles are PHI-grade: nothing here
//! logs them, and errors sent to the UI are fixed codes with no text.
use serde::{Deserialize, Serialize};
use sprk_assistant::actions::{ActionItem, Owner};
use sprk_assistant::client::RemoteEngine;
use sprk_assistant::engine::{AssistantEngine, EngineError};
use sprk_assistant::extras::{Coverage, Explained, Shortened};
use sprk_assistant::features::{FeatureSet, ALL};
use sprk_assistant::governor::Tier;
use sprk_assistant::host::LoadParams;
use sprk_assistant::qa::Answer;
use sprk_assistant::runtime::{discover_packs, pack_roots, PackSource, Runtime, Spawner};
use sprk_assistant::service::{Status, UserSettings};
use sprk_assistant::view::{hms, Line, Note};
use sprk_pipeline::host::UiLine;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

pub struct Assistant {
    rt: Arc<Runtime>,
}

fn helper_path() -> PathBuf {
    let name = if cfg!(windows) {
        "sprk-assistant.exe"
    } else {
        "sprk-assistant"
    };
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join(name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// Free physical memory in MB, or 0 when unknown (which makes the memory gate refuse).
fn available_mb() -> u64 {
    #[cfg(windows)]
    {
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut m = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        // SAFETY: `m` is a valid, correctly sized MEMORYSTATUSEX that lives for the call.
        if unsafe { GlobalMemoryStatusEx(&mut m) }.is_ok() {
            return m.ullAvailPhys / (1024 * 1024);
        }
        0
    }
    #[cfg(not(windows))]
    {
        std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|t| {
                t.lines()
                    .find_map(|l| l.strip_prefix("MemAvailable:"))
                    .and_then(|r| r.split_whitespace().next()?.parse::<u64>().ok())
            })
            .map(|kb| kb / 1024)
            .unwrap_or(0)
    }
}

impl Assistant {
    pub fn new() -> Self {
        let helper = helper_path();
        let spawn: Spawner = Arc::new(move |p: &LoadParams| {
            RemoteEngine::spawn(&helper, p.clone())
                .map(|e| Box::new(e) as Box<dyn AssistantEngine + Send>)
        });
        let user = crate::models::user_data_dir();
        let machine = std::env::var_os("ProgramData").map(PathBuf::from);
        let roots = pack_roots(user.as_deref(), machine.as_deref());
        let packs: PackSource = Arc::new(move || discover_packs(&roots));
        let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
        Self {
            rt: Arc::new(Runtime::new(spawn, packs, cores)),
        }
    }

    /// Called for every committed line and event while a session runs.
    pub fn feed(&self, l: &UiLine) {
        match l {
            UiLine::Statement {
                id,
                t,
                speaker,
                text,
            } => self.rt.with_workspace(|w| {
                w.push_line(Line {
                    id: *id,
                    t_ms: parse_t(t),
                    speaker: speaker.to_string(),
                    text: text.clone(),
                })
            }),
            UiLine::Event { t, text, .. } => self.rt.with_workspace(|w| {
                w.push_note(Note {
                    t_ms: parse_t(t),
                    text: text.clone(),
                })
            }),
        }
    }

    /// New session: forget the old one's lines and pins.
    pub fn clear(&self) {
        self.rt.clear_session();
    }

    /// Unloads the helper after the idle delay. Started once from `setup`.
    pub fn start_idle_timer(&self) {
        let rt = Arc::clone(&self.rt);
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            rt.idle_tick(&crate::policy());
        });
        // The Rolling feature: one short catch-up per ten minutes of meeting, only if chosen.
        let rt = Arc::clone(&self.rt);
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
            rt.background_tick(&crate::policy(), available_mb());
        });
    }
}

/// `HH:MM:SS` (or `MM:SS`) to milliseconds. Anything else is 0.
fn parse_t(t: &str) -> u64 {
    let parts: Vec<u64> = t
        .trim()
        .rsplit(' ')
        .next()
        .unwrap_or("")
        .split(':')
        .filter_map(|p| p.parse().ok())
        .collect();
    let secs = match parts.as_slice() {
        [h, m, s] => h * 3600 + m * 60 + s,
        [m, s] => m * 60 + s,
        _ => 0,
    };
    secs * 1000
}

// ------------------------------------------------------------------ UI payloads

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusPayload {
    /// `off`, `noModel`, `notEnoughMemory`, `ready`.
    state: &'static str,
    locked: bool,
    can_download: bool,
    model: Option<String>,
    tier: String,
    max_tier: String,
    enabled: bool,
    /// Features that are on now (user choice narrowed by policy).
    features: Vec<&'static str>,
    /// Features policy allows the user to turn on.
    allowed_features: Vec<&'static str>,
}

fn tier_name(t: Tier) -> String {
    format!("{t:?}").to_lowercase()
}

#[tauri::command]
pub fn assistant_status(state: State<Assistant>) -> StatusPayload {
    let policy = crate::policy();
    let can_download = policy.model_download != Some(false);
    let (st, eff) = state.rt.status(&policy, available_mb(), can_download);
    let (s, locked, dl, model) = match &st {
        Status::Off { locked } => ("off", *locked, false, None),
        Status::NoModel { can_download } => ("noModel", false, *can_download, None),
        Status::NotEnoughMemory => ("notEnoughMemory", false, false, None),
        Status::Ready { id_version, .. } => ("ready", false, false, Some(id_version.clone())),
    };
    let allowed = match policy.assistant_features.as_deref() {
        Some(l) => FeatureSet::parse_list(l),
        None => FeatureSet::all(),
    };
    StatusPayload {
        state: s,
        locked,
        can_download: dl,
        model,
        tier: tier_name(eff.tier),
        max_tier: tier_name(eff.max_tier),
        enabled: eff.enabled,
        features: eff.features.keys(),
        allowed_features: ALL
            .into_iter()
            .filter(|f| allowed.has(*f))
            .map(|f| f.key())
            .collect(),
    }
}

/// The Settings section. Held in memory by the UI's saved preferences and sent at launch and on change.
#[tauri::command]
pub fn assistant_set(state: State<Assistant>, enabled: bool, tier: String, features: Vec<String>) {
    let set = features
        .iter()
        .filter_map(|k| sprk_assistant::features::Feature::parse(k))
        .fold(FeatureSet::none(), |s, f| s.with(f, true));
    state.rt.set_user(UserSettings {
        enabled,
        tier: Tier::parse(&tier).unwrap_or(Tier::Light),
        features: set,
    });
    if !enabled {
        state.rt.unload();
    }
}

#[tauri::command]
pub fn assistant_cancel(state: State<Assistant>) {
    state.rt.cancel();
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Request {
    Title,
    #[serde(rename_all = "camelCase")]
    Recap {
        minutes: u32,
    },
    Actions,
    Ask {
        question: String,
    },
    Agenda {
        items: Vec<String>,
    },
    Commitments,
    #[serde(rename_all = "camelCase")]
    Explain {
        line_id: u64,
    },
    #[serde(rename_all = "camelCase")]
    Shorten {
        from_id: u64,
        to_id: u64,
    },
    Prep,
    /// Whole-meeting summary.
    Summary,
    /// Questions from Others since I last spoke.
    Asked,
    /// Background catch-ups written so far.
    Catchups,
    PinNow {
        label: String,
    },
    #[serde(rename_all = "camelCase")]
    Unpin {
        line_id: u64,
    },
    Pins,
    Parking,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cited {
    text: String,
    cites: Vec<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionOut {
    task: String,
    /// `me`, `named` or `unknown`.
    owner_kind: &'static str,
    owner_name: Option<String>,
    due: Option<String>,
    cites: Vec<u64>,
}

fn action_out(i: &ActionItem) -> ActionOut {
    let (k, n) = match &i.owner {
        Owner::Me => ("me", None),
        Owner::Named(n) => ("named", Some(n.clone())),
        Owner::Unknown => ("unknown", None),
    };
    ActionOut {
        task: i.task.clone(),
        owner_kind: k,
        owner_name: n,
        due: i.due.clone(),
        cites: i.cites.clone(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinOut {
    line_id: u64,
    t: String,
    label: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParkedOut {
    line_id: u64,
    t: String,
    text: String,
}

/// Result of one request. `state` is `ok`, `notStated` or `unconfirmed` where that applies.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Response {
    Title {
        text: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Recap {
        bullets: Vec<Cited>,
        dropped: usize,
        nothing_said: bool,
    },
    Actions {
        items: Vec<ActionOut>,
        dropped: usize,
    },
    Answer {
        state: &'static str,
        text: Option<String>,
        cites: Vec<u64>,
    },
    Agenda {
        items: Vec<AgendaOut>,
    },
    Commitments {
        items: Vec<ActionOut>,
    },
    Pins {
        pins: Vec<PinOut>,
    },
    Parking {
        items: Vec<ParkedOut>,
    },
    Prep {
        markdown: String,
    },
    Asked {
        items: Vec<ParkedOut>,
    },
    Catchups {
        items: Vec<CatchupOut>,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatchupOut {
    from: String,
    to: String,
    bullets: Vec<Cited>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgendaOut {
    item: String,
    covered: bool,
    cites: Vec<u64>,
}

fn recap_out(r: sprk_assistant::recap::Recap) -> Response {
    Response::Recap {
        bullets: r
            .bullets
            .into_iter()
            .map(|b| Cited {
                text: b.text,
                cites: b.cites,
            })
            .collect(),
        dropped: r.dropped,
        nothing_said: r.nothing_said,
    }
}

fn err_code(e: sprk_assistant::runtime::RunError) -> String {
    use sprk_assistant::runtime::RunError as R;
    match e {
        R::Off => "off",
        R::Disabled => "disabled",
        R::NoModel => "noModel",
        R::NotEnoughMemory => "notEnoughMemory",
        R::Busy => "busy",
        R::Cancelled => "cancelled",
        R::Failed => "failed",
    }
    .to_string()
}

fn ws_err(e: sprk_assistant::workspace::WorkspaceError) -> String {
    use sprk_assistant::workspace::WorkspaceError as W;
    match e {
        W::Disabled => "disabled".to_string(),
        W::Engine(EngineError::Cancelled) => "cancelled".to_string(),
        W::Engine(_) => "failed".to_string(),
    }
}

fn answer_like(state: &'static str, text: Option<String>, cites: Vec<u64>) -> Response {
    Response::Answer { state, text, cites }
}

/// One entry point for every feature, so the UI bridge stays small. Heavy work runs off the UI thread.
#[tauri::command]
pub async fn assistant_run(app: AppHandle, request: Request) -> Result<Response, String> {
    tauri::async_runtime::spawn_blocking(move || run_blocking(&app.state::<Assistant>(), request))
        .await
        .map_err(|_| "failed".to_string())?
}

fn run_blocking(state: &Assistant, request: Request) -> Result<Response, String> {
    let policy = crate::policy();
    let rt = &state.rt;
    let mb = available_mb();
    let feats = rt.effective(&policy).features;
    let go = |r: Result<Response, sprk_assistant::runtime::RunError>| r.map_err(err_code);
    match request {
        Request::Title => go(rt
            .run(&policy, mb, |w, f, e, c, fm| w.title(f, e, c, fm))
            .map(|text| Response::Title { text })),
        Request::Recap { minutes } => go(rt
            .run(&policy, mb, |w, f, e, c, fm| w.recap(f, e, c, fm, minutes))
            .map(recap_out)),
        Request::Actions => go(rt
            .run(&policy, mb, |w, f, e, c, fm| w.actions(f, e, c, fm))
            .map(|l| Response::Actions {
                items: l.items.iter().map(action_out).collect(),
                dropped: l.dropped,
            })),
        Request::Ask { question } => go(rt
            .run(&policy, mb, |w, f, e, c, fm| w.ask(f, e, c, fm, &question))
            .map(|a| match a {
                Answer::Found { text, cites } => answer_like("ok", Some(text), cites),
                Answer::NotStated => answer_like("notStated", None, vec![]),
                Answer::Unconfirmed => answer_like("unconfirmed", None, vec![]),
            })),
        Request::Agenda { items } => go(rt
            .run(&policy, mb, |w, f, e, c, fm| w.agenda(f, e, c, fm, &items))
            .map(|v| Response::Agenda {
                items: v
                    .into_iter()
                    .map(|s| match s.coverage {
                        Coverage::Covered { cites } => AgendaOut {
                            item: s.item,
                            covered: true,
                            cites,
                        },
                        Coverage::NotYet => AgendaOut {
                            item: s.item,
                            covered: false,
                            cites: vec![],
                        },
                    })
                    .collect(),
            })),
        Request::Commitments => go(rt
            .run(&policy, mb, |w, f, e, c, fm| w.commitments(f, e, c, fm))
            .map(|v| Response::Commitments {
                items: v.iter().map(action_out).collect(),
            })),
        Request::Explain { line_id } => go(rt
            .run(&policy, mb, |w, f, e, c, fm| {
                w.explain(f, e, c, fm, line_id)
            })
            .map(|x| match x {
                Explained::Text { text, cites } => answer_like("ok", Some(text), cites),
                Explained::NotStated => answer_like("notStated", None, vec![]),
                Explained::Unconfirmed => answer_like("unconfirmed", None, vec![]),
            })),
        Request::Shorten { from_id, to_id } => go(rt
            .run(&policy, mb, |w, f, e, c, fm| {
                w.shorten(f, e, c, fm, from_id, to_id)
            })
            .map(|x| match x {
                Shortened::Text { text, cites } => answer_like("ok", Some(text), cites),
                Shortened::Unconfirmed => answer_like("unconfirmed", None, vec![]),
                Shortened::Empty => answer_like("notStated", None, vec![]),
            })),
        Request::Prep => go(rt
            .run(&policy, mb, |w, f, e, c, fm| w.prep(f, e, c, fm))
            .map(|p| Response::Prep {
                markdown: sprk_assistant::extras::prep_markdown(&p),
            })),
        Request::Summary => go(rt
            .run(&policy, mb, |w, f, e, c, fm| w.summary(f, e, c, fm))
            .map(recap_out)),
        // No model needed below: these never start the helper.
        Request::Asked => {
            let items = rt
                .with_workspace(|w| w.asked(feats, 5 * 60_000))
                .map_err(ws_err)?
                .into_iter()
                .map(|a| ParkedOut {
                    line_id: a.line_id,
                    t: hms(a.t_ms),
                    text: a.text,
                })
                .collect();
            Ok(Response::Asked { items })
        }
        Request::Catchups => {
            let items = rt
                .with_workspace(|w| w.catchups(feats))
                .map_err(ws_err)?
                .into_iter()
                .map(|c| CatchupOut {
                    from: hms(c.from_ms),
                    to: hms(c.to_ms),
                    bullets: c
                        .bullets
                        .into_iter()
                        .map(|b| Cited {
                            text: b.text,
                            cites: b.cites,
                        })
                        .collect(),
                })
                .collect();
            Ok(Response::Catchups { items })
        }
        Request::PinNow { label } => {
            rt.with_workspace(|w| w.pin_now(feats, &label))
                .map_err(ws_err)?;
            pins_response(rt, feats)
        }
        Request::Unpin { line_id } => {
            rt.with_workspace(|w| w.unpin(feats, line_id))
                .map_err(ws_err)?;
            pins_response(rt, feats)
        }
        Request::Pins => pins_response(rt, feats),
        Request::Parking => {
            let items = rt
                .with_workspace(|w| w.parking(feats))
                .map_err(ws_err)?
                .into_iter()
                .map(|p| ParkedOut {
                    line_id: p.line_id,
                    t: hms(p.t_ms),
                    text: p.text,
                })
                .collect();
            Ok(Response::Parking { items })
        }
    }
}

fn pins_response(rt: &Runtime, feats: FeatureSet) -> Result<Response, String> {
    let pins = rt
        .with_workspace(|w| w.pins(feats))
        .map_err(ws_err)?
        .into_iter()
        .map(|p| PinOut {
            line_id: p.line_id,
            t: hms(p.t_ms),
            label: p.label,
        })
        .collect();
    Ok(Response::Pins { pins })
}

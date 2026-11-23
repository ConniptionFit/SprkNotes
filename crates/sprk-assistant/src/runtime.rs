//! The assistant's life inside the app (ADR-009), free of any UI toolkit so it tests on Linux.
//! It owns the session workspace, starts the helper only when a request needs it, keeps one
//! request at a time, and unloads after idle. Prompts, answers and titles pass through here and are
//! never logged; errors are plain kinds with no text.
use crate::engine::{AssistantEngine, Control, EngineError, RunControl, SharedControl};
use crate::features::FeatureSet;
use crate::governor::{choose_pack, default_threads, IdleUnload, PackChoice, Refusal, Tier};
use crate::host::LoadParams;
use crate::prompt::PromptFormat;
use crate::service::{effective, Effective, UserSettings};
use crate::workspace::{Workspace, WorkspaceError};
use sprk_config::AppPolicy;
use sprk_models::{Manifest, ModelStore, StoreKind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::Instant;

/// Largest context the app asks for. Views are at most about 2.5k tokens plus the prompt.
const MAX_CONTEXT_TOKENS: u32 = 6_144;

/// An installed assistant pack as the runtime needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pack {
    pub choice: PackChoice,
    pub dir: PathBuf,
    pub format: PromptFormat,
    pub context_tokens: u32,
}

/// Reads every installed assistant pack under `roots` (newest first within an id). A pack with an
/// unknown tier or prompt format is skipped, not guessed at.
pub fn discover_packs(roots: &[PathBuf]) -> Vec<Pack> {
    let mut out = Vec::new();
    for root in roots {
        let store = ModelStore::for_kind(
            root.clone(),
            StoreKind::User,
            sprk_models::manifest::KIND_ASSISTANT,
        );
        for i in store.list() {
            let Ok(text) = std::fs::read_to_string(i.dir.join("model.json")) else {
                continue;
            };
            let Ok(man) = Manifest::from_json(&text) else {
                continue;
            };
            let Some(spec) = man.assistant else { continue };
            let (Some(tier), Some(format)) = (
                Tier::parse(&spec.tier),
                PromptFormat::from_id(&spec.prompt_format),
            ) else {
                continue;
            };
            out.push(Pack {
                choice: PackChoice {
                    id_version: i.id_version.clone(),
                    tier,
                    ram_estimate_mb: spec.ram_estimate_mb,
                },
                dir: i.dir.clone(),
                format,
                context_tokens: spec.context_tokens.min(MAX_CONTEXT_TOKENS),
            });
        }
    }
    out
}

/// Why a request did not produce a result. No transcript text, ever.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RunError {
    #[error("the assistant is off")]
    Off,
    #[error("that feature is off")]
    Disabled,
    #[error("no assistant model is installed")]
    NoModel,
    #[error("not enough free memory right now")]
    NotEnoughMemory,
    #[error("the assistant is busy with another request")]
    Busy,
    #[error("cancelled")]
    Cancelled,
    #[error("the assistant could not finish")]
    Failed,
}

pub type Spawner =
    Arc<dyn Fn(&LoadParams) -> Result<Box<dyn AssistantEngine + Send>, EngineError> + Send + Sync>;
pub type PackSource = Arc<dyn Fn() -> Vec<Pack> + Send + Sync>;

struct Loaded {
    engine: Box<dyn AssistantEngine + Send>,
    id_version: String,
    format: PromptFormat,
}

pub struct Runtime {
    ws: Mutex<Workspace>,
    user: Mutex<UserSettings>,
    engine: Mutex<Option<Loaded>>,
    idle: Mutex<IdleUnload>,
    control: SharedControl,
    spawn: Spawner,
    packs: PackSource,
    started: Instant,
    cores: usize,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Runtime {
    pub fn new(spawn: Spawner, packs: PackSource, cores: usize) -> Self {
        Self {
            ws: Mutex::default(),
            user: Mutex::new(UserSettings::default()),
            engine: Mutex::new(None),
            idle: Mutex::new(IdleUnload::new(Some(5), 0)),
            control: SharedControl::default(),
            spawn,
            packs,
            started: Instant::now(),
            cores,
        }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    pub fn set_user(&self, u: UserSettings) {
        *lock(&self.user) = u;
    }

    pub fn user(&self) -> UserSettings {
        *lock(&self.user)
    }

    pub fn effective(&self, policy: &AppPolicy) -> Effective {
        effective(policy, &self.user())
    }

    /// Session lines. Short locks only, so transcription never waits on the model.
    pub fn with_workspace<T>(&self, f: impl FnOnce(&mut Workspace) -> T) -> T {
        f(&mut lock(&self.ws))
    }

    pub fn clear_session(&self) {
        lock(&self.ws).clear();
    }

    /// Stops the request in flight, if any.
    pub fn cancel(&self) {
        self.control.set(Control::Cancel);
    }

    /// Tell a running request to wait (transcription is lagging) or carry on.
    pub fn set_control(&self, c: Control) {
        self.control.set(c);
    }

    pub fn is_loaded(&self) -> bool {
        match self.engine.try_lock() {
            Ok(g) => g.is_some(),
            Err(TryLockError::WouldBlock) => true,
            Err(TryLockError::Poisoned(p)) => p.into_inner().is_some(),
        }
    }

    /// Unloads the helper once it has sat unused for the idle delay. Call every half minute or so.
    pub fn idle_tick(&self, policy: &AppPolicy) {
        let eff = self.effective(policy);
        let Ok(mut g) = self.engine.try_lock() else {
            return; // busy
        };
        if g.is_some() {
            let mut idle = lock(&self.idle);
            idle.minutes = eff.idle_unload_minutes;
            if idle.should_unload(self.now_ms(), false) {
                *g = None; // dropping the engine stops the helper
            }
        }
    }

    /// Unload now (assistant turned off, or the session ended and the user asked).
    pub fn unload(&self) {
        if let Ok(mut g) = self.engine.try_lock() {
            *g = None;
        }
    }

    /// What Settings and the Ask button show.
    pub fn status(
        &self,
        policy: &AppPolicy,
        available_mb: u64,
        can_download: bool,
    ) -> (crate::service::Status, Effective) {
        let eff = self.effective(policy);
        let choices: Vec<PackChoice> = (self.packs)().into_iter().map(|p| p.choice).collect();
        (
            crate::service::status(&eff, &choices, available_mb, can_download),
            eff,
        )
    }

    /// Runs one feature against a snapshot of the session. Starts the helper if needed, with the
    /// memory check. One request at a time: a second one gets `Busy`.
    pub fn run<T>(
        &self,
        policy: &AppPolicy,
        available_mb: u64,
        f: impl FnOnce(
            &Workspace,
            FeatureSet,
            &mut dyn AssistantEngine,
            &dyn RunControl,
            PromptFormat,
        ) -> Result<T, WorkspaceError>,
    ) -> Result<T, RunError> {
        let eff = self.effective(policy);
        if !eff.enabled {
            return Err(RunError::Off);
        }
        let mut guard = match self.engine.try_lock() {
            Ok(g) => g,
            Err(TryLockError::WouldBlock) => return Err(RunError::Busy),
            Err(TryLockError::Poisoned(p)) => p.into_inner(),
        };
        if guard.is_none() {
            let packs = (self.packs)();
            let choices: Vec<PackChoice> = packs.iter().map(|p| p.choice.clone()).collect();
            let pick = choose_pack(available_mb, eff.max_tier, eff.tier, &choices).map_err(
                |r| match r {
                    Refusal::Off => RunError::Off,
                    Refusal::NoPack => RunError::NoModel,
                    Refusal::NotEnoughMemory => RunError::NotEnoughMemory,
                },
            )?;
            let Some(pack) = packs
                .iter()
                .find(|p| p.choice.id_version == pick.id_version)
            else {
                return Err(RunError::NoModel);
            };
            let params = LoadParams {
                model_dir: pack.dir.to_string_lossy().into_owned(),
                threads: u32::try_from(default_threads(self.cores, eff.max_threads)).unwrap_or(1),
                context_tokens: pack.context_tokens,
            };
            let engine = (self.spawn)(&params).map_err(|_| RunError::Failed)?;
            *guard = Some(Loaded {
                engine,
                id_version: pack.choice.id_version.clone(),
                format: pack.format,
            });
        }
        let Some(loaded) = guard.as_mut() else {
            return Err(RunError::Failed);
        };
        self.control.set(Control::Run);
        let snapshot = lock(&self.ws).clone();
        let result = f(
            &snapshot,
            eff.features,
            loaded.engine.as_mut(),
            &self.control,
            loaded.format,
        );
        lock(&self.idle).touch(self.now_ms());
        match result {
            Ok(v) => Ok(v),
            Err(WorkspaceError::Disabled) => Err(RunError::Disabled),
            Err(WorkspaceError::Engine(EngineError::Cancelled)) => Err(RunError::Cancelled),
            Err(WorkspaceError::Engine(_)) => {
                // A failed helper is dropped so the next request starts a fresh one.
                *guard = None;
                Err(RunError::Failed)
            }
        }
    }

    /// Called about once a minute while a session runs. When the Rolling feature is on and a
    /// catch-up is due, writes one for the last 10 minutes. Never queues behind a user request:
    /// if the helper is busy this simply tries again at the next tick.
    pub fn background_tick(&self, policy: &AppPolicy, available_mb: u64) {
        let eff = self.effective(policy);
        if !eff.enabled || !eff.background || !lock(&self.ws).catchup_due(eff.features) {
            return;
        }
        let out = self.run(policy, available_mb, |w, f, e, c, fm| {
            let to = w.now_ms();
            let r = w.recap(f, e, c, fm, 10)?;
            Ok((to, r))
        });
        if let Ok((to_ms, r)) = out {
            lock(&self.ws).add_catchup(crate::workspace::Catchup {
                from_ms: to_ms.saturating_sub(crate::workspace::ROLLING_MS),
                to_ms,
                bullets: r.bullets,
            });
        }
    }

    /// The loaded model's id, for the status line. `None` when nothing is loaded.
    pub fn loaded_model(&self) -> Option<String> {
        self.engine
            .try_lock()
            .ok()
            .and_then(|g| g.as_ref().map(|l| l.id_version.clone()))
    }
}

/// Where assistant packs live: next to transcription packs, in their own folder.
pub fn pack_roots(user_data: Option<&Path>, program_data: Option<&Path>) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = program_data {
        v.push(p.join("SprkNotes").join("assistant-models"));
    }
    if let Some(p) = user_data {
        v.push(p.join("SprkNotes").join("assistant-models"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::FakeEngine;
    use crate::features::Feature;
    use crate::view::Line;
    use sprk_config::FakePolicy;

    fn policy(pairs: &[(&str, &str)]) -> AppPolicy {
        AppPolicy::load(&FakePolicy(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ))
    }

    fn pack(tier: Tier, ram: u32) -> Pack {
        Pack {
            choice: PackChoice {
                id_version: format!("m-{tier:?}@2027.01.0"),
                tier,
                ram_estimate_mb: ram,
            },
            dir: PathBuf::from("/nowhere"),
            format: PromptFormat::ChatMl,
            context_tokens: 4096,
        }
    }

    fn runtime(packs: Vec<Pack>, replies: &'static [&'static str]) -> Runtime {
        let spawn: Spawner = Arc::new(move |_p| {
            Ok(Box::new(FakeEngine::with_replies(replies)) as Box<dyn AssistantEngine + Send>)
        });
        let src: PackSource = Arc::new(move || packs.clone());
        Runtime::new(spawn, src, 8)
    }

    fn on() -> UserSettings {
        UserSettings {
            enabled: true,
            ..UserSettings::default()
        }
    }

    fn feed(rt: &Runtime) {
        rt.with_workspace(|w| {
            for (i, t) in [
                "Welcome, today is the budget review.",
                "The invoices are due on the fifteenth.",
                "I will send them by Thursday.",
            ]
            .iter()
            .enumerate()
            {
                w.push_line(Line {
                    id: i as u64 + 1,
                    t_ms: (i as u64 + 1) * 10_000,
                    speaker: "Others".into(),
                    text: (*t).into(),
                });
            }
        });
    }

    #[test]
    fn off_until_turned_on_and_no_helper_starts() {
        let rt = runtime(vec![pack(Tier::Light, 1500)], &[]);
        let r = rt.run(&policy(&[]), 16_000, |w, f, e, c, fm| {
            w.recap(f, e, c, fm, 5)
        });
        assert_eq!(r.unwrap_err(), RunError::Off);
        assert!(!rt.is_loaded());
    }

    #[test]
    fn loads_on_first_use_then_unloads_when_idle() {
        let rt = runtime(
            vec![pack(Tier::Light, 1500)],
            &["The invoices are due on the fifteenth. [L2]"],
        );
        rt.set_user(on());
        feed(&rt);
        let a = rt
            .run(&policy(&[]), 16_000, |w, f, e, c, fm| {
                w.ask(f, e, c, fm, "When are the invoices due?")
            })
            .unwrap();
        assert!(matches!(a, crate::qa::Answer::Found { .. }));
        assert!(rt.is_loaded());
        assert_eq!(rt.loaded_model().as_deref(), Some("m-Light@2027.01.0"));
        // Idle time is measured from the last request; zero minutes means never, five is not up yet.
        rt.idle_tick(&policy(&[]));
        assert!(rt.is_loaded());
        rt.unload();
        assert!(!rt.is_loaded());
    }

    #[test]
    fn memory_and_missing_pack_are_plain_refusals() {
        let rt = runtime(vec![pack(Tier::Light, 1500)], &[]);
        rt.set_user(on());
        let r = rt.run(&policy(&[]), 2_000, |w, f, e, c, fm| {
            w.recap(f, e, c, fm, 5)
        });
        assert_eq!(r.unwrap_err(), RunError::NotEnoughMemory);
        let rt = runtime(vec![], &[]);
        rt.set_user(on());
        let r = rt.run(&policy(&[]), 16_000, |w, f, e, c, fm| {
            w.recap(f, e, c, fm, 5)
        });
        assert_eq!(r.unwrap_err(), RunError::NoModel);
    }

    #[test]
    fn extras_stay_off_until_chosen() {
        let rt = runtime(vec![pack(Tier::Light, 1500)], &[]);
        rt.set_user(on());
        feed(&rt);
        let r = rt.run(&policy(&[]), 16_000, |w, f, e, c, fm| w.prep(f, e, c, fm));
        assert_eq!(r.unwrap_err(), RunError::Disabled);
        let mut u = on();
        u.features = u.features.with(Feature::Pins, true);
        rt.set_user(u);
        let pins = rt.with_workspace(|w| w.pin_now(rt.effective(&policy(&[])).features, "x"));
        assert!(pins.unwrap().is_some());
    }

    #[test]
    fn rolling_catchup_runs_when_due_and_only_when_chosen() {
        let rt = runtime(
            vec![pack(Tier::Light, 1500)],
            &["- The invoices are due on the fifteenth. [L2]"],
        );
        let mut u = on();
        rt.set_user(u);
        // Plenty of lines spread over more than ten minutes.
        rt.with_workspace(|w| {
            for i in 1..=12u64 {
                w.push_line(Line {
                    id: i,
                    t_ms: i * 60_000,
                    speaker: "Others".into(),
                    text: if i == 2 {
                        "The invoices are due on the fifteenth.".into()
                    } else {
                        format!("Filler line number {i} about the project plan.")
                    },
                });
            }
        });
        let eff = |rt: &Runtime| rt.effective(&policy(&[])).features;
        rt.background_tick(&policy(&[]), 16_000);
        assert!(rt.with_workspace(|w| w.catchups(eff(&rt)).is_err()));
        u.features = u.features.with(Feature::Rolling, true);
        rt.set_user(u);
        assert!(rt.with_workspace(|w| w.catchup_due(eff(&rt))));
        rt.background_tick(&policy(&[]), 16_000);
        assert_eq!(
            rt.with_workspace(|w| w.catchups(eff(&rt)).map(|c| c.len())),
            Ok(1)
        );
        // Just ran: not due again until another ten minutes pass.
        assert!(!rt.with_workspace(|w| w.catchup_due(eff(&rt))));
        // Policy can turn the background off.
        let off = policy(&[("AssistantBackground", "0")]);
        rt.background_tick(&off, 16_000);
    }

    #[test]
    fn discovers_assistant_packs_and_skips_unknown_formats() {
        let tmp = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let root = tmp.path().join("SprkNotes").join("assistant-models");
        assert!(discover_packs(&pack_roots(Some(tmp.path()), None)).is_empty());
        std::fs::create_dir_all(&root).unwrap_or(());
        assert!(discover_packs(&[root]).is_empty());
    }
}

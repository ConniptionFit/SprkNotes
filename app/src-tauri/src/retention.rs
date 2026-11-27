//! Retention (SPEC 6.6): "Keep transcripts for" in Settings, or an administrator's
//! `TranscriptRetentionDays`. Transcripts older than that go to the Recycle Bin (deleted when it
//! can't take them) when the app starts and once a day while it runs. The running session's
//! file and the one open in the Library are never touched. Only a count reaches the window;
//! nothing here logs names, titles or text.
use serde::Serialize;
use sprk_transcript::retention::{self, Swept};
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter, Manager};

/// What Settings offers, in days (0 is forever). An administrator may set any other number.
pub const CHOICES: [u64; 4] = [0, 30, 90, 365];

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// Which source decided the setting; the window says it in plain words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Policy,
    User,
    Install,
    Default,
}

/// Days to keep (0 is forever) and where that came from: policy, then the person's choice,
/// then the installer's starting value, then forever. Pure, so it is tested.
pub fn resolve(policy: Option<u64>, user: Option<u64>, install: Option<u64>) -> (u64, Source) {
    if let Some(d) = policy {
        return (d, Source::Policy);
    }
    if let Some(d) = user {
        return (d, Source::User);
    }
    if let Some(d) = install {
        return (d, Source::Install);
    }
    (0, Source::Default)
}

fn current(policy: &sprk_config::AppPolicy) -> (u64, Source) {
    resolve(
        policy.transcript_retention_days,
        crate::save_location::load().keep_days,
        sprk_config::admin_defaults().transcript_retention_days,
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Days to keep; 0 keeps transcripts forever.
    pub days: u64,
    pub source: Source,
    /// Set by an administrator: the Settings choice is read-only.
    pub locked: bool,
}

#[tauri::command]
pub fn retention_status() -> Status {
    let (days, source) = current(&crate::policy());
    Status {
        days,
        source,
        locked: source == Source::Policy,
    }
}

/// Save the person's choice (one of [`CHOICES`]). Takes effect at the next start or daily
/// check, so a slip of the mouse can be put back before anything is removed.
#[tauri::command]
pub fn retention_set(days: u64) -> Result<Status, String> {
    if crate::policy().transcript_retention_days.is_some() {
        return Err("Your organization sets how long transcripts are kept.".into());
    }
    if !CHOICES.contains(&days) {
        return Err("Pick one of the choices shown.".into());
    }
    crate::save_location::store(&crate::save_location::Saved {
        keep_days: Some(days),
        ..crate::save_location::load()
    })?;
    Ok(retention_status())
}

/// Moves to the Recycle Bin, and deletes what it can't take, without asking.
struct RecycleOrDelete;

impl sprk_transcript::library::Trash for RecycleOrDelete {
    fn trash(&self, paths: &[std::path::PathBuf]) -> std::io::Result<()> {
        sprk_handoff::trash::recycle_or_delete(paths)
    }
}

/// Today's local date as days since 1970-01-01.
fn today() -> Option<i64> {
    retention::day_number(&chrono::Local::now().format("%Y-%m-%d").to_string())
}

/// One sweep now. The session lock is held throughout, so no session can start on, or continue,
/// a transcript while it is being removed.
fn sweep_now(app: &AppHandle) -> Swept {
    let policy = crate::policy();
    let (days, _) = current(&policy);
    if days == 0 {
        return Swept::default();
    }
    let (Ok(root), Some(today)) = (crate::transcripts_root(app, &policy), today()) else {
        return Swept::default();
    };
    let state = app.state::<crate::AppState>();
    state.hold(|live| {
        let protected: Vec<_> = live.into_iter().chain(state.viewing()).collect();
        retention::sweep(&root, days, today, &protected, &RecycleOrDelete)
    })
}

/// What the window hears after a sweep that removed something: a count, never names.
#[derive(Serialize, Clone)]
struct Removed {
    removed: usize,
    days: u64,
}

/// At start (once the window has had a moment to load) and then once a day while the app runs.
/// The wall clock is checked every ten minutes, so a PC that sleeps through the night still
/// sweeps when it wakes.
pub fn start(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("sprk-retention".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(20));
            loop {
                let swept = sweep_now(&app);
                if swept.removed > 0 {
                    let _ = app.emit(
                        "retention-removed",
                        Removed {
                            removed: swept.removed,
                            days: current(&crate::policy()).0,
                        },
                    );
                }
                let mut last = SystemTime::now();
                loop {
                    std::thread::sleep(Duration::from_secs(600));
                    match last.elapsed() {
                        Ok(d) if d >= DAY => break,
                        Ok(_) => {}
                        // The clock was set back: count the day from now.
                        Err(_) => last = SystemTime::now(),
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_then_user_then_install_then_forever() {
        assert_eq!(resolve(Some(90), Some(30), Some(365)), (90, Source::Policy));
        assert_eq!(resolve(Some(0), Some(30), None), (0, Source::Policy));
        assert_eq!(resolve(None, Some(0), Some(365)), (0, Source::User));
        assert_eq!(resolve(None, None, Some(365)), (365, Source::Install));
        assert_eq!(resolve(None, None, None), (0, Source::Default));
    }
}

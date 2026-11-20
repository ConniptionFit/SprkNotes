//! Library and Summarize commands (SPEC 8.4, 9). The window sends a transcript path; every path
//! is checked to be a transcript inside the transcripts root before anything touches it.
//! Nothing here logs titles, names or transcript text.
use serde::Serialize;
use sprk_handoff::detect::{self, Decision, SystemProbe, TargetInfo};
use sprk_handoff::{Launcher, Request};
use sprk_transcript::{export, format::timestamp, library};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

#[cfg(windows)]
use sprk_handoff::shell::ShellLauncher;
#[cfg(not(windows))]
use sprk_handoff::unix::ShellLauncher;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    path: String,
    name: String,
    title: String,
    date: String,
    start: String,
    duration: String,
    status: String,
    folder: String,
}

#[derive(Serialize)]
pub struct ViewLine {
    kind: &'static str,
    t: String,
    speaker: String,
    text: String,
}

#[derive(Serialize)]
pub struct View {
    title: String,
    date: String,
    start: String,
    duration: String,
    context: String,
    lines: Vec<ViewLine>,
}

#[derive(Serialize, Clone)]
pub struct Target {
    id: &'static str,
    name: &'static str,
}

impl From<&TargetInfo> for Target {
    fn from(t: &TargetInfo) -> Self {
        Target {
            id: t.id,
            name: t.name,
        }
    }
}

/// Result of pressing Summarize.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summarized {
    /// Both clients are installed: show the choice, then call again with the chosen id.
    choose: Vec<Target>,
    /// Plain-language notes for the user.
    toasts: Vec<String>,
    /// Prompt text the OS clipboard could not take; the window copies it.
    clipboard: Option<String>,
}

fn root(app: &AppHandle) -> Result<PathBuf, String> {
    crate::transcripts_root(app, &crate::policy())
}

fn checked(app: &AppHandle, path: &str) -> Result<PathBuf, String> {
    library::resolve(&root(app)?, path)
        .ok_or_else(|| "That transcript is not in your library.".to_string())
}

fn entry(e: library::Entry) -> Entry {
    Entry {
        path: e.path.to_string_lossy().into_owned(),
        name: e.name,
        title: e.title,
        date: e.date,
        start: e.start,
        duration: e.duration,
        status: e.status,
        folder: e.folder,
    }
}

#[tauri::command]
pub fn library_list(app: AppHandle, query: String) -> Result<Vec<Entry>, String> {
    Ok(library::search(&root(&app)?, &query)
        .into_iter()
        .map(entry)
        .collect())
}

#[tauri::command]
pub fn library_open(app: AppHandle, path: String) -> Result<View, String> {
    let root = root(&app)?;
    let p = checked(&app, &path)?;
    let t = library::load(&root, &p).ok_or("Could not read that transcript.")?;
    let mut rows: Vec<(u64, u8, ViewLine)> = Vec::new();
    for g in &t.segments {
        rows.push((
            g.start_ms,
            1,
            ViewLine {
                kind: "statement",
                t: timestamp(g.start_ms),
                speaker: g.speaker.clone(),
                text: g.text.clone(),
            },
        ));
    }
    for e in &t.events {
        rows.push((
            e.t_ms,
            0,
            ViewLine {
                kind: "event",
                t: timestamp(e.t_ms),
                speaker: String::new(),
                text: e.text.clone(),
            },
        ));
    }
    rows.sort_by_key(|r| (r.0, r.1));
    Ok(View {
        title: t.title,
        date: t.date,
        start: t.start,
        duration: t.duration,
        context: t.meeting_context,
        lines: rows.into_iter().map(|r| r.2).collect(),
    })
}

fn reveal(path: &Path) {
    let mut l = ShellLauncher::default();
    l.reveal(&path.to_string_lossy());
}

/// "View in folder".
#[tauri::command]
pub fn library_reveal(app: AppHandle, path: String) -> Result<(), String> {
    reveal(&checked(&app, &path)?);
    Ok(())
}

/// Export next to the transcript (txt, json or docx) and show it. Returns the file name.
#[tauri::command]
pub fn library_export(app: AppHandle, path: String, format: String) -> Result<String, String> {
    let root = root(&app)?;
    let p = checked(&app, &path)?;
    let exporter = export::by_extension(&format).ok_or("Unknown file type.")?;
    let t = library::load(&root, &p).ok_or("Could not read that transcript.")?;
    let bytes = exporter.export(&t).map_err(|e| e.to_string())?;
    let dir = p.parent().ok_or("No folder.")?;
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let out =
        sprk_transcript::naming::unique_path(dir, &format!("{stem}.{}", exporter.extension()));
    std::fs::write(&out, bytes).map_err(|_| "Could not save the file.".to_string())?;
    reveal(&out);
    Ok(out
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default())
}

fn copilot_scheme(target: &Option<String>) -> String {
    match target {
        Some(t) if !t.to_ascii_lowercase().starts_with("https://") => {
            t.split(':').next().unwrap_or("").to_ascii_lowercase()
        }
        _ => detect::COPILOT_SCHEME.to_string(),
    }
}

/// Installed, not-hidden clients. A custom https Copilot target has nothing to detect, so an
/// admin who set one gets Copilot offered (unless they hid it).
fn found(policy: &sprk_config::AppPolicy) -> Vec<TargetInfo> {
    let hide_claude = policy.show_claude_handoff == Some(false);
    let hide_copilot = policy.show_copilot_handoff == Some(false);
    let mut v = detect::available(
        &SystemProbe,
        &copilot_scheme(&policy.copilot_target),
        hide_claude,
        hide_copilot,
    );
    let https = policy
        .copilot_target
        .as_deref()
        .is_some_and(|t| t.to_ascii_lowercase().starts_with("https://"));
    if https && !hide_copilot && v.iter().all(|t| t.id != "copilot") {
        v.push(detect::COPILOT);
    }
    v
}

/// The newest transcript, so Summarize knows whether it has anything to work on.
#[tauri::command]
pub fn latest_transcript(app: AppHandle) -> Result<Option<Entry>, String> {
    Ok(library::scan(&root(&app)?).into_iter().next().map(entry))
}

fn read_template(app: &AppHandle, policy: &sprk_config::AppPolicy) -> String {
    use tauri::Manager;
    let user = app
        .path()
        .config_dir()
        .ok()
        .map(|d| d.join("SprkNotes").join("prompts").join("summary.md"))
        .and_then(|p| std::fs::read_to_string(p).ok());
    let admin = policy
        .prompt_template_path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok());
    sprk_handoff::pick_template(user, admin)
}

/// Summarize a transcript (`path`, or the newest one) with `target`, or let the app pick:
/// one client installed means use it, none means copy the prompt and show the file, both means
/// return them so the user can choose. SprkNotes sends nothing; the user sends in that app.
#[tauri::command]
pub fn summarize(
    app: AppHandle,
    path: Option<String>,
    target: Option<String>,
) -> Result<Summarized, String> {
    let policy = crate::policy();
    let root = root(&app)?;
    let file = match path {
        Some(p) => checked(&app, &p)?,
        None => library::scan(&root)
            .into_iter()
            .next()
            .map(|e| e.path)
            .ok_or("There is no transcript to summarize yet.")?,
    };
    let t = library::load(&root, &file).ok_or("Could not read that transcript.")?;
    let req = Request {
        file: file.to_string_lossy().into_owned(),
        folder: file
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        title: t.title,
        date: t.date,
    };
    let prompt = sprk_handoff::fill_template(&read_template(&app, &policy), &req);
    let options = found(&policy);
    let plan = match target.as_deref() {
        // The user picked: only a client that is installed and allowed counts.
        Some(id) => {
            if !options.iter().any(|o| o.id == id) {
                return Err("That app is not available on this computer.".into());
            }
            detect::plan_for(id, policy.copilot_target.as_deref(), &req, &prompt)
                .ok_or("Unknown app.")?
        }
        None => match detect::decide(&options) {
            Decision::Run(one) => {
                detect::plan_for(one.id, policy.copilot_target.as_deref(), &req, &prompt)
                    .ok_or("Unknown app.")?
            }
            Decision::Ask(many) => {
                return Ok(Summarized {
                    choose: many.iter().map(Target::from).collect(),
                    toasts: vec![],
                    clipboard: None,
                })
            }
            Decision::Fallback => detect::fallback_plan(&req, &prompt),
        },
    };
    let mut l = ShellLauncher::default();
    sprk_handoff::run(&plan, &mut l);
    // Only on Linux can the clipboard tools be missing.
    #[cfg(not(windows))]
    let clipboard = l.uncopied.take();
    #[cfg(windows)]
    let clipboard = None;
    Ok(Summarized {
        choose: vec![],
        toasts: l.toasts,
        clipboard,
    })
}

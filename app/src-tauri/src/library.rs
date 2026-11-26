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

pub(crate) fn checked(app: &AppHandle, path: &str) -> Result<PathBuf, String> {
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

/// Open a transcript in the Library. It counts as open (retention leaves it alone, SPEC 6.6)
/// until `library_close` or another one is opened.
#[tauri::command]
pub fn library_open(
    app: AppHandle,
    state: tauri::State<crate::AppState>,
    path: String,
) -> Result<View, String> {
    let root = root(&app)?;
    let p = checked(&app, &path)?;
    if let Ok(mut v) = state.viewing.lock() {
        *v = Some(p.clone());
    }
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

/// The Library closed its transcript view.
#[tauri::command]
pub fn library_close(state: tauri::State<crate::AppState>) {
    if let Ok(mut v) = state.viewing.lock() {
        *v = None;
    }
}

fn reveal(path: &Path) {
    let mut l = ShellLauncher::default();
    l.reveal(&path.to_string_lossy());
}

/// Transcripts a crash left `in-progress` (SPEC 6.4). The window asks once at launch. Nothing
/// is reported while a session runs, and a file touched in the last seconds may belong to
/// another running copy, so it is left alone.
#[tauri::command]
pub fn recovery_scan(
    app: AppHandle,
    state: tauri::State<crate::AppState>,
) -> Result<Vec<Entry>, String> {
    if state.host.lock().map(|h| h.is_some()).unwrap_or(true) {
        return Ok(Vec::new());
    }
    let idle = |p: &Path| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|d| d >= std::time::Duration::from_secs(15))
    };
    Ok(library::scan(&root(&app)?)
        .into_iter()
        .filter(|e| e.status == "in-progress" && idle(&e.path))
        .map(entry)
        .collect())
}

/// "Mark finished": the transcript keeps every saved line and is closed as interrupted.
#[tauri::command]
pub fn recovery_mark(
    app: AppHandle,
    state: tauri::State<crate::AppState>,
    path: String,
) -> Result<(), String> {
    if state.host.lock().map(|h| h.is_some()).unwrap_or(true) {
        return Err("End the current session first.".into());
    }
    let p = checked(&app, &path)?;
    sprk_transcript::recovery::mark_interrupted(&p)
        .map_err(|_| "Could not mark that transcript finished.".to_string())
}

/// "Open folder": a library folder (`""` is the top) in File Explorer.
#[tauri::command]
pub fn library_open_folder(app: AppHandle, folder: String) -> Result<(), String> {
    let root = root(&app)?;
    std::fs::create_dir_all(&root).map_err(|_| "The transcripts folder could not be opened.")?;
    let dir = if folder.trim().is_empty() {
        root.clone()
    } else {
        // Only folders inside the library, never `..` or an absolute path.
        let rel = std::path::Path::new(folder.trim());
        if rel.is_absolute()
            || rel
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err("That folder is not in the library.".into());
        }
        root.join(rel)
    };
    if !dir.is_dir() {
        return Err("That folder no longer exists.".into());
    }
    let mut l = ShellLauncher::default();
    if l.open_url(&dir.to_string_lossy()) {
        Ok(())
    } else {
        Err("File Explorer could not be opened.".into())
    }
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

/// The Copilot link to open: the admin's, or the first default link that has a handler (the
/// Microsoft 365 Copilot app registers more than one across versions).
fn copilot_target(policy: &sprk_config::AppPolicy) -> Option<String> {
    policy
        .copilot_target
        .clone()
        .or_else(|| Some(detect::default_copilot_link(&SystemProbe)))
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
        .and_then(|p| std::fs::read_to_string(crate::save_location::expand(p)).ok());
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
    if policy.summarize_enabled == Some(false) {
        return Err("Your organization has turned off Summarize.".into());
    }
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
        body: std::fs::read_to_string(&file).unwrap_or_default(),
    };
    let prompt = sprk_handoff::fill_template(&read_template(&app, &policy), &req);
    let options = found(&policy);
    let plan = match target.as_deref() {
        // The user picked: only a client that is installed and allowed counts.
        Some(id) => {
            if !options.iter().any(|o| o.id == id) {
                return Err("That app is not available on this computer.".into());
            }
            detect::plan_for(id, copilot_target(&policy).as_deref(), &req, &prompt)
                .ok_or("Unknown app.")?
        }
        None => match detect::decide(&options) {
            Decision::Run(one) => {
                detect::plan_for(one.id, copilot_target(&policy).as_deref(), &req, &prompt)
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
    #[cfg(windows)]
    {
        use tauri::Manager;
        l.owner = app
            .get_webview_window("main")
            .and_then(|w| w.hwnd().ok())
            .map_or(0, |h| h.0 as isize);
    }
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

// ---- Organize (SPEC 8.4): rename, folders, move, delete to the Recycle Bin. ----

/// The file the running session writes to, if any. The library leaves that one alone: moving
/// or deleting it mid-session would lose lines.
fn live(state: &tauri::State<crate::AppState>) -> Option<PathBuf> {
    let host = state.host.lock().ok()?;
    let p = host.as_ref()?.path()?;
    Some(std::fs::canonicalize(&p).unwrap_or(p))
}

fn same(a: &Path, b: &Path) -> bool {
    std::fs::canonicalize(a).map(|a| a == b).unwrap_or(false)
}

fn not_live(state: &tauri::State<crate::AppState>, p: &Path) -> Result<(), String> {
    match live(state) {
        Some(l) if same(p, &l) => Err("End the current session first.".into()),
        _ => Ok(()),
    }
}

fn plain(e: library::LibraryError) -> String {
    use library::LibraryError as E;
    match e {
        E::BadName => "That name can't be used. Try a shorter one without symbols.".into(),
        E::FolderExists => "There is already a folder with that name.".into(),
        E::NoSuchFolder => "That folder is not in your library.".into(),
        E::Trash => "Could not move it to the Recycle Bin.".into(),
        E::Transcript(_) => "Could not change that transcript.".into(),
    }
}

struct RecycleBin;

impl library::Trash for RecycleBin {
    fn trash(&self, paths: &[PathBuf]) -> std::io::Result<()> {
        sprk_handoff::trash::to_trash(paths)
    }
}

/// Every user folder under the library, as paths relative to it ("Clients/Acme").
#[tauri::command]
pub fn library_folders(app: AppHandle) -> Result<Vec<String>, String> {
    Ok(library::folders(&root(&app)?))
}

/// Rename a transcript: the title in the file and the file name follow. Returns the new path.
#[tauri::command]
pub fn library_rename(
    app: AppHandle,
    state: tauri::State<crate::AppState>,
    path: String,
    title: String,
) -> Result<String, String> {
    let p = checked(&app, &path)?;
    not_live(&state, &p)?;
    library::rename(&root(&app)?, &p, &title)
        .map(|n| n.to_string_lossy().into_owned())
        .map_err(plain)
}

/// Move a transcript into `folder` ("" is the library itself). Returns the new path.
#[tauri::command]
pub fn library_move(
    app: AppHandle,
    state: tauri::State<crate::AppState>,
    path: String,
    folder: String,
) -> Result<String, String> {
    let p = checked(&app, &path)?;
    not_live(&state, &p)?;
    library::move_to(&root(&app)?, &p, &folder)
        .map(|n| n.to_string_lossy().into_owned())
        .map_err(plain)
}

/// Delete a transcript to the Recycle Bin. The window confirms first.
#[tauri::command]
pub fn library_delete(
    app: AppHandle,
    state: tauri::State<crate::AppState>,
    path: String,
) -> Result<(), String> {
    let p = checked(&app, &path)?;
    not_live(&state, &p)?;
    library::delete(&root(&app)?, &p, &RecycleBin).map_err(plain)
}

/// New folder inside `parent` ("" for the top). Returns its relative path.
#[tauri::command]
pub fn folder_create(app: AppHandle, parent: String, name: String) -> Result<String, String> {
    library::create_folder(&root(&app)?, &parent, &name).map_err(plain)
}

/// Rename a folder. Refused while the running session writes inside it.
#[tauri::command]
pub fn folder_rename(
    app: AppHandle,
    state: tauri::State<crate::AppState>,
    folder: String,
    name: String,
) -> Result<String, String> {
    let root = root(&app)?;
    if let Some(l) = live(&state) {
        let dir = std::fs::canonicalize(root.join(&folder)).ok();
        if dir.is_some_and(|d| l.starts_with(d)) {
            return Err("End the current session first.".into());
        }
    }
    library::rename_folder(&root, &folder, &name).map_err(plain)
}

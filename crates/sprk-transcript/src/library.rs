//! Library (SPEC 8.4): list and search finished transcripts under the transcripts root, and
//! organize them: rename, move between folders, create and rename folders, delete (through a
//! [`Trash`], so a deleted transcript can be restored). Pure std plus the parser, so it is
//! testable on Linux. Nothing here logs titles or text, and errors never carry them.
use crate::format::NameSource;
use crate::naming::{sanitize_title, unique_path};
use crate::parse::Parsed;
use crate::writer::rewrite_head;
use crate::FORMAT;
use std::path::{Component, Path, PathBuf};

/// Why a library change was refused. Messages are plain and never name the file or folder.
#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("that name is not allowed")]
    BadName,
    #[error("a folder with that name already exists")]
    FolderExists,
    #[error("that folder is not in the library")]
    NoSuchFolder,
    #[error("could not move it to the trash")]
    Trash,
    #[error(transparent)]
    Transcript(#[from] crate::TranscriptError),
}

impl From<std::io::Error> for LibraryError {
    fn from(e: std::io::Error) -> Self {
        LibraryError::Transcript(e.into())
    }
}

/// Moves files to the system's Recycle Bin or trash, so a delete can be undone there.
/// The app passes the platform one; tests pass a fake.
pub trait Trash {
    /// All paths are absolute. Must not delete permanently when it cannot trash.
    fn trash(&self, paths: &[PathBuf]) -> std::io::Result<()>;
}

/// One row of the library list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    /// File name without `.md`.
    pub name: String,
    pub title: String,
    pub date: String,
    pub start: String,
    /// When the session last ended (front matter `end`), empty while one runs.
    pub end: String,
    pub duration: String,
    pub status: String,
    /// Folder relative to the root, `""` for the root itself. Always `/` separated.
    pub folder: String,
}

fn is_transcript_name(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

/// Read only the front matter block (cheap: no need to parse a long transcript for the list).
fn front_matter(md: &str) -> Option<&str> {
    let rest = md
        .strip_prefix("---\n")
        .or_else(|| md.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 6 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name();
        // The sidecar folder `.sprknotes` and other dot folders hold no transcripts.
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            walk(&p, out, depth + 1);
        } else if ft.is_file() && is_transcript_name(&p) {
            out.push(p);
        }
    }
}

fn entry_for(root: &Path, path: &Path) -> Option<Entry> {
    let md = std::fs::read_to_string(path).ok()?;
    let fm = front_matter(&md)?;
    if !fm.lines().any(|l| l.trim() == format!("format: {FORMAT}")) {
        // Summaries and notes the user saved next to a transcript are not transcripts.
        return None;
    }
    let p = Parsed::parse(&md, None);
    let rel = path.parent()?.strip_prefix(root).ok()?;
    let folder = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    Some(Entry {
        path: path.to_path_buf(),
        name: path.file_stem()?.to_string_lossy().into_owned(),
        title: p.title,
        date: p.date,
        start: p.start,
        end: p.end,
        duration: p.duration,
        status: p.status,
        folder,
    })
}

/// All transcripts under `root`, newest first. A missing root is an empty library.
pub fn scan(root: &Path) -> Vec<Entry> {
    let mut files = Vec::new();
    walk(root, &mut files, 0);
    let mut v: Vec<Entry> = files.iter().filter_map(|f| entry_for(root, f)).collect();
    v.sort_by(|a, b| (&b.date, &b.start, &b.name).cmp(&(&a.date, &a.start, &a.name)));
    v
}

/// Search by name, title and full text (case-insensitive). An empty query lists everything.
pub fn search(root: &Path, query: &str) -> Vec<Entry> {
    let q = query.trim().to_lowercase();
    let all = scan(root);
    if q.is_empty() {
        return all;
    }
    all.into_iter()
        .filter(|e| {
            e.name.to_lowercase().contains(&q)
                || e.title.to_lowercase().contains(&q)
                || std::fs::read_to_string(&e.path).is_ok_and(|t| t.to_lowercase().contains(&q))
        })
        .collect()
}

/// A path the UI sent, accepted only if it is a transcript file inside `root`. Guards the
/// commands that read, reveal, export or hand off a file against paths outside the library.
pub fn resolve(root: &Path, path: &str) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let p = Path::new(path).canonicalize().ok()?;
    (p.starts_with(&root) && p.is_file() && is_transcript_name(&p)).then(|| plain(p))
}

/// Windows `canonicalize` returns `\\?\C:\...`; Explorer and app links want the plain form.
fn plain(p: PathBuf) -> PathBuf {
    match p.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => p,
    }
}

/// Parse a transcript with its sidecar (when present) for viewing or export.
pub fn load(root: &Path, path: &Path) -> Option<Parsed> {
    let md = std::fs::read_to_string(path).ok()?;
    let name = path.file_stem()?.to_string_lossy().into_owned();
    let side = root.join(".sprknotes").join(format!("{name}.jsonl"));
    let side = std::fs::read_to_string(side).ok();
    Some(Parsed::parse(&md, side.as_deref()))
}

/// The sidecar of a transcript (`<root>/.sprknotes/<stem>.jsonl`), when there is one.
fn sidecar_of(root: &Path, path: &Path) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let p = root.join(".sprknotes").join(format!("{stem}.jsonl"));
    p.is_file().then_some(p)
}

/// Keep the sidecar next to its transcript's name after a rename or a move that had to pick a
/// new name. A sidecar is optional (exact times only), so failing here never fails the change.
fn follow_sidecar(root: &Path, old: &Path, new: &Path) {
    let Some(sc) = sidecar_of(root, old) else {
        return;
    };
    let Some(stem) = new.file_stem() else {
        return;
    };
    let target = root
        .join(".sprknotes")
        .join(format!("{}.jsonl", stem.to_string_lossy()));
    if target != sc && !target.exists() {
        let _ = std::fs::rename(sc, target);
    }
}

/// `YYYY-MM-DD HHmm ` at the start of a file name, kept when the user renames the title.
fn stamp_prefix(stem: &str) -> Option<&str> {
    let b = stem.as_bytes();
    let digits = |r: std::ops::Range<usize>| {
        r.into_iter()
            .all(|i| b.get(i).is_some_and(u8::is_ascii_digit))
    };
    let ok = b.len() > 16
        && digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && b[10] == b' '
        && digits(11..15)
        && b[15] == b' ';
    ok.then(|| &stem[..16])
}

/// Rename a transcript (SPEC 8.4): the title in the file and the file name both change, the
/// date and time at the start of the name stay, and the name counts as typed by the user
/// (`name_source: user`, SPEC 6.2). Returns the new path.
pub fn rename(root: &Path, path: &Path, title: &str) -> Result<PathBuf, LibraryError> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        return Err(LibraryError::BadName);
    }
    let text = std::fs::read_to_string(path)?;
    let mut meta = crate::recovery::parse_meta(&text)?;
    meta.title = title.clone();
    meta.name_source = NameSource::User;
    rewrite_head(path, &meta)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = format!(
        "{}{}.md",
        stamp_prefix(&stem).unwrap_or(""),
        sanitize_title(&title)
    );
    let dir = path.parent().ok_or(LibraryError::NoSuchFolder)?;
    if path
        .file_name()
        .is_some_and(|n| n.to_string_lossy() == name)
    {
        return Ok(path.to_path_buf());
    }
    let new = unique_path(dir, &name);
    std::fs::rename(path, &new)?;
    follow_sidecar(root, path, &new);
    Ok(new)
}

/// A folder path the UI sent (`/` separated, relative to the root, `""` for the root), checked:
/// plain names only, no `..`, no hidden folders.
fn folder_path(root: &Path, rel: &str) -> Result<PathBuf, LibraryError> {
    const BAD: &[char] = &['\\', ':', '<', '>', '"', '|', '?', '*'];
    let mut p = root.to_path_buf();
    for part in rel.split('/').filter(|s| !s.is_empty()) {
        if part.starts_with('.') || part.chars().any(|c| c.is_control() || BAD.contains(&c)) {
            return Err(LibraryError::NoSuchFolder);
        }
        p.push(part);
    }
    // A linked folder must not lead out of the library.
    if p.exists() {
        let (Ok(cp), Ok(cr)) = (p.canonicalize(), root.canonicalize()) else {
            return Err(LibraryError::NoSuchFolder);
        };
        if !cp.starts_with(cr) {
            return Err(LibraryError::NoSuchFolder);
        }
    }
    Ok(p)
}

/// A folder name made safe the same way as a title. Empty or dot-leading names are refused.
fn sanitize_folder(name: &str) -> Result<String, LibraryError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.starts_with('.') {
        return Err(LibraryError::BadName);
    }
    let clean = sanitize_title(trimmed);
    // sanitize_title turns an unusable name into "Transcript"; a folder should say no instead.
    if clean == "Transcript" && !trimmed.eq_ignore_ascii_case("transcript") {
        return Err(LibraryError::BadName);
    }
    Ok(clean)
}

fn rel_of(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .map(|r| {
            r.components()
                .filter_map(|c| match c {
                    Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default()
}

/// Every user folder under the root (`/` separated, sorted), including empty ones, so the
/// folder tree can show a folder before anything is moved into it. Hidden folders are skipped.
pub fn folders(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>, depth: usize) {
        if depth > 6 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                let p = e.path();
                out.push(rel_of(root, &p));
                walk(root, &p, out, depth + 1);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out, 0);
    out.sort_by_key(|f| f.to_lowercase());
    out
}

/// Create `name` inside `parent` (`""` for the root). Returns the new folder's relative path.
pub fn create_folder(root: &Path, parent: &str, name: &str) -> Result<String, LibraryError> {
    let base = folder_path(root, parent)?;
    if !base.is_dir() {
        return Err(LibraryError::NoSuchFolder);
    }
    let p = base.join(sanitize_folder(name)?);
    if p.exists() {
        return Err(LibraryError::FolderExists);
    }
    std::fs::create_dir(&p)?;
    Ok(rel_of(root, &p))
}

/// Rename the folder `rel` (never the root). Returns its new relative path.
pub fn rename_folder(root: &Path, rel: &str, name: &str) -> Result<String, LibraryError> {
    let from = folder_path(root, rel)?;
    if from == root || !from.is_dir() {
        return Err(LibraryError::NoSuchFolder);
    }
    let to = from
        .parent()
        .ok_or(LibraryError::NoSuchFolder)?
        .join(sanitize_folder(name)?);
    if to == from {
        return Ok(rel_of(root, &to));
    }
    // A case-only rename is the same folder on Windows; anything else must be free.
    let same = to.to_string_lossy().to_lowercase() == from.to_string_lossy().to_lowercase();
    if to.exists() && !same {
        return Err(LibraryError::FolderExists);
    }
    std::fs::rename(&from, &to)?;
    Ok(rel_of(root, &to))
}

/// Move a transcript into the folder `rel` (`""` for the root). A name already taken there gets
/// a counter, as at the start of a session. Returns the new path.
pub fn move_to(root: &Path, path: &Path, rel: &str) -> Result<PathBuf, LibraryError> {
    let dir = folder_path(root, rel)?;
    if !dir.is_dir() {
        return Err(LibraryError::NoSuchFolder);
    }
    if path.parent() == Some(dir.as_path()) {
        return Ok(path.to_path_buf());
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or(LibraryError::BadName)?;
    let new = unique_path(&dir, &name);
    std::fs::rename(path, &new)?;
    follow_sidecar(root, path, &new);
    Ok(new)
}

/// Delete a transcript by moving it, and its sidecar, to the trash (SPEC 8.4: Recycle Bin).
/// Summaries and exports saved next to it are the user's own files and stay.
pub fn delete(root: &Path, path: &Path, trash: &dyn Trash) -> Result<(), LibraryError> {
    let sidecar = sidecar_of(root, path);
    trash
        .trash(&[path.to_path_buf()])
        .map_err(|_| LibraryError::Trash)?;
    // The sidecar only adds exact times; a failure here leaves a hidden orphan, not a transcript.
    if let Some(sc) = sidecar {
        let _ = trash.trash(&[sc]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(title: &str, date: &str, start: &str, body: &str) -> String {
        format!(
            "---\nformat: sprknotes-transcript/1\ntitle: \"{title}\"\ndate: {date}\nstart: \"{start}\"\nduration: \"0:10:00\"\nstatus: complete\n---\n# {title}\n\n## Transcript\n\n- `00:00:04` **Me:** {body}\n"
        )
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sprk-lib-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn lists_newest_first_skips_non_transcripts_and_sidecars() {
        let d = tmp("list");
        std::fs::create_dir_all(d.join("Clients")).unwrap();
        std::fs::create_dir_all(d.join(".sprknotes")).unwrap();
        std::fs::write(d.join("a.md"), md("Old", "2026-11-01", "09:00", "hello")).unwrap();
        std::fs::write(
            d.join("Clients/b.md"),
            md("New", "2026-11-19", "09:00", "bye"),
        )
        .unwrap();
        std::fs::write(d.join("a - Summary.md"), "# Summary\nnot a transcript").unwrap();
        std::fs::write(
            d.join(".sprknotes/c.md"),
            md("Hidden", "2026-11-20", "09:00", "x"),
        )
        .unwrap();
        std::fs::write(d.join("notes.txt"), "x").unwrap();
        let v = scan(&d);
        assert_eq!(
            v.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(),
            ["New", "Old"]
        );
        assert_eq!(v[0].folder, "Clients");
        assert_eq!(v[1].folder, "");
        assert!(scan(&d.join("missing")).is_empty());
    }

    #[test]
    fn search_matches_name_and_full_text() {
        let d = tmp("search");
        std::fs::write(
            d.join("a.md"),
            md("Budget sync", "2026-11-01", "09:00", "the roadmap"),
        )
        .unwrap();
        std::fs::write(
            d.join("b.md"),
            md("Other", "2026-11-02", "09:00", "lunch plans"),
        )
        .unwrap();
        assert_eq!(search(&d, "budget").len(), 1);
        assert_eq!(search(&d, "ROADMAP").len(), 1);
        assert_eq!(search(&d, "").len(), 2);
        assert!(search(&d, "zzz").is_empty());
    }

    #[test]
    fn resolve_rejects_paths_outside_the_root() {
        let d = tmp("resolve");
        let outside = tmp("outside");
        std::fs::write(d.join("a.md"), md("A", "2026-11-01", "09:00", "x")).unwrap();
        std::fs::write(outside.join("o.md"), md("O", "2026-11-01", "09:00", "x")).unwrap();
        assert!(resolve(&d, d.join("a.md").to_str().unwrap()).is_some());
        assert!(resolve(&d, outside.join("o.md").to_str().unwrap()).is_none());
        let sneaky = format!(
            "{}/../{}/o.md",
            d.display(),
            outside.file_name().unwrap().to_string_lossy()
        );
        assert!(resolve(&d, &sneaky).is_none());
        assert!(resolve(&d, d.to_str().unwrap()).is_none());
    }

    struct FakeTrash(std::cell::RefCell<Vec<PathBuf>>, bool);
    impl Trash for FakeTrash {
        fn trash(&self, paths: &[PathBuf]) -> std::io::Result<()> {
            if !self.1 {
                return Err(std::io::ErrorKind::Unsupported.into());
            }
            for p in paths {
                std::fs::remove_file(p)?;
                self.0.borrow_mut().push(p.clone());
            }
            Ok(())
        }
    }

    #[test]
    fn rename_changes_title_and_name_keeps_stamp_and_sidecar() {
        let d = tmp("rename");
        std::fs::create_dir_all(d.join(".sprknotes")).unwrap();
        let p = d.join("2026-11-19 0900 Transcript.md");
        std::fs::write(&p, md("Transcript", "2026-11-19", "09:00", "hello")).unwrap();
        std::fs::write(
            d.join(".sprknotes/2026-11-19 0900 Transcript.jsonl"),
            "{}\n",
        )
        .unwrap();
        let new = rename(&d, &p, "  Budget:  sync \"Q3\" ").unwrap();
        assert_eq!(new, d.join("2026-11-19 0900 Budget sync Q3.md"));
        assert!(!p.exists());
        let text = std::fs::read_to_string(&new).unwrap();
        assert!(
            text.contains("title: \"Budget: sync \\\"Q3\\\"\""),
            "{text}"
        );
        assert!(text.contains("name_source: user"));
        assert!(text.contains("status: complete"));
        assert!(text.contains("# Budget: sync \"Q3\"\n"));
        assert!(text.contains("**Me:** hello"));
        assert!(d
            .join(".sprknotes/2026-11-19 0900 Budget sync Q3.jsonl")
            .exists());
        let e = scan(&d);
        assert_eq!(e[0].title, "Budget: sync \"Q3\"");

        // Renaming again keeps the quotes as typed (no double escaping).
        let again = rename(&d, &new, "Budget: sync \"Q3\"").unwrap();
        assert_eq!(again, new);
        assert_eq!(scan(&d)[0].title, "Budget: sync \"Q3\"");

        // A name taken in the folder gets a counter; an empty name is refused.
        let q = d.join("2026-11-19 1000 Other.md");
        std::fs::write(&q, md("Other", "2026-11-19", "10:00", "x")).unwrap();
        let taken = rename(&d, &q, "x").unwrap();
        assert_eq!(taken, d.join("x.md").with_file_name("2026-11-19 1000 x.md"));
        assert!(matches!(
            rename(&d, &taken, "   "),
            Err(LibraryError::BadName)
        ));
    }

    #[test]
    fn rename_without_a_stamp_uses_the_title_alone() {
        let d = tmp("rename-plain");
        let p = d.join("notes.md");
        std::fs::write(&p, md("Old", "2026-11-19", "09:00", "x")).unwrap();
        assert_eq!(rename(&d, &p, "New").unwrap(), d.join("New.md"));
        let p = d.join("2026-11-19 0900 A.md");
        std::fs::write(&p, md("A", "2026-11-19", "09:00", "x")).unwrap();
        std::fs::write(
            d.join("2026-11-19 0900 B.md"),
            md("B", "2026-11-19", "09:00", "x"),
        )
        .unwrap();
        assert_eq!(
            rename(&d, &p, "B").unwrap(),
            d.join("2026-11-19 0900 B (2).md")
        );
    }

    #[test]
    fn folders_create_rename_and_list() {
        let d = tmp("folders");
        std::fs::create_dir_all(d.join(".sprknotes")).unwrap();
        assert_eq!(create_folder(&d, "", "Clients").unwrap(), "Clients");
        assert_eq!(
            create_folder(&d, "Clients", "Acme / East").unwrap(),
            "Clients/Acme East"
        );
        assert!(matches!(
            create_folder(&d, "", "Clients"),
            Err(LibraryError::FolderExists)
        ));
        assert!(matches!(
            create_folder(&d, "", ".hidden"),
            Err(LibraryError::BadName)
        ));
        assert!(matches!(
            create_folder(&d, "", " ?? "),
            Err(LibraryError::BadName)
        ));
        assert!(matches!(
            create_folder(&d, "..", "x"),
            Err(LibraryError::NoSuchFolder)
        ));
        assert!(matches!(
            create_folder(&d, "Missing", "x"),
            Err(LibraryError::NoSuchFolder)
        ));
        assert_eq!(folders(&d), ["Clients", "Clients/Acme East"]);
        assert_eq!(
            rename_folder(&d, "Clients/Acme East", "Acme").unwrap(),
            "Clients/Acme"
        );
        create_folder(&d, "", "Archive").unwrap();
        assert!(matches!(
            rename_folder(&d, "Archive", "Clients"),
            Err(LibraryError::FolderExists)
        ));
        assert!(matches!(
            rename_folder(&d, "", "x"),
            Err(LibraryError::NoSuchFolder)
        ));
        assert!(matches!(
            rename_folder(&d, ".sprknotes", "x"),
            Err(LibraryError::NoSuchFolder)
        ));
        assert_eq!(rename_folder(&d, "Archive", "archive").unwrap(), "archive");
        assert_eq!(folders(&d), ["archive", "Clients", "Clients/Acme"]);
    }

    #[test]
    fn move_between_folders_keeps_names_unique() {
        let d = tmp("move");
        std::fs::create_dir_all(d.join(".sprknotes")).unwrap();
        create_folder(&d, "", "Clients").unwrap();
        let a = d.join("2026-11-19 0900 Sync.md");
        std::fs::write(&a, md("Sync", "2026-11-19", "09:00", "x")).unwrap();
        std::fs::write(d.join(".sprknotes/2026-11-19 0900 Sync.jsonl"), "{}\n").unwrap();
        std::fs::write(
            d.join("Clients/2026-11-19 0900 Sync.md"),
            md("Sync", "2026-11-19", "09:00", "y"),
        )
        .unwrap();
        let moved = move_to(&d, &a, "Clients").unwrap();
        assert_eq!(moved, d.join("Clients/2026-11-19 0900 Sync (2).md"));
        assert!(d.join(".sprknotes/2026-11-19 0900 Sync (2).jsonl").exists());
        assert_eq!(move_to(&d, &moved, "Clients").unwrap(), moved);
        let back = move_to(&d, &moved, "").unwrap();
        assert_eq!(back, d.join("2026-11-19 0900 Sync (2).md"));
        assert!(matches!(
            move_to(&d, &back, "Nope"),
            Err(LibraryError::NoSuchFolder)
        ));
        assert!(matches!(
            move_to(&d, &back, "../x"),
            Err(LibraryError::NoSuchFolder)
        ));
        assert!(matches!(
            move_to(&d, &back, ".sprknotes"),
            Err(LibraryError::NoSuchFolder)
        ));
        assert_eq!(scan(&d).iter().filter(|e| e.folder == "Clients").count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_folder_cannot_lead_out_of_the_library() {
        let d = tmp("link");
        let outside = tmp("link-out");
        std::os::unix::fs::symlink(&outside, d.join("Out")).unwrap();
        let a = d.join("a.md");
        std::fs::write(&a, md("A", "2026-11-19", "09:00", "x")).unwrap();
        assert!(matches!(
            move_to(&d, &a, "Out"),
            Err(LibraryError::NoSuchFolder)
        ));
        assert!(a.exists());
    }

    #[test]
    fn delete_trashes_the_transcript_and_its_sidecar_only() {
        let d = tmp("delete");
        std::fs::create_dir_all(d.join(".sprknotes")).unwrap();
        let a = d.join("t.md");
        std::fs::write(&a, md("T", "2026-11-19", "09:00", "x")).unwrap();
        std::fs::write(d.join(".sprknotes/t.jsonl"), "{}\n").unwrap();
        std::fs::write(d.join("t - Summary.md"), "summary").unwrap();
        let refusing = FakeTrash(Default::default(), false);
        assert!(matches!(
            delete(&d, &a, &refusing),
            Err(LibraryError::Trash)
        ));
        assert!(a.exists(), "nothing is removed when the trash refuses");
        let t = FakeTrash(Default::default(), true);
        delete(&d, &a, &t).unwrap();
        assert_eq!(*t.0.borrow(), vec![a.clone(), d.join(".sprknotes/t.jsonl")]);
        assert!(d.join("t - Summary.md").exists());
        assert!(scan(&d).is_empty());
    }
}

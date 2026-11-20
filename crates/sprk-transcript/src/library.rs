//! Library (SPEC 8.4): list and search finished transcripts under the transcripts root.
//! Pure std plus the parser, so it is testable on Linux. Nothing here logs titles or text.
use crate::parse::Parsed;
use crate::FORMAT;
use std::path::{Component, Path, PathBuf};

/// One row of the library list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    /// File name without `.md`.
    pub name: String,
    pub title: String,
    pub date: String,
    pub start: String,
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
}

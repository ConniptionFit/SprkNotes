//! File naming (SPEC 6.2): `YYYY-MM-DD HHmm <Title>.md`, sanitized, collision-safe.
use std::path::{Path, PathBuf};

const MAX_TITLE: usize = 120;
const INVALID: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Make a title safe as a Windows file name component.
pub fn sanitize_title(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_control() || INVALID.contains(&c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let mut t = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    t = t.chars().take(MAX_TITLE).collect();
    let t = t.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    if t.is_empty() {
        return "Transcript".into();
    }
    if RESERVED.contains(&t.to_ascii_uppercase().as_str()) {
        return format!("{t}_");
    }
    t
}

/// `YYYY-MM-DD HHmm Title.md`; `date` is `YYYY-MM-DD`, `hhmm` is `HHmm`.
pub fn file_name(date: &str, hhmm: &str, title: &str) -> String {
    format!("{date} {hhmm} {}.md", sanitize_title(title))
}

/// First free path for `name` in `dir`, adding ` (2)`, ` (3)`... before the extension.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (name.to_string(), String::new()),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_invalid_characters() {
        assert_eq!(sanitize_title("Q3: Plan / Review?"), "Q3 Plan Review");
    }

    #[test]
    fn empty_and_reserved_titles() {
        assert_eq!(sanitize_title("  ...  "), "Transcript");
        assert_eq!(sanitize_title("con"), "con_");
    }

    #[test]
    fn trims_to_120_chars() {
        assert_eq!(sanitize_title(&"a".repeat(300)).chars().count(), 120);
    }

    #[test]
    fn name_pattern() {
        assert_eq!(
            file_name("2026-11-18", "1300", "Weekly IT Sync"),
            "2026-11-18 1300 Weekly IT Sync.md"
        );
    }

    #[test]
    fn collisions_get_a_counter() {
        let d = std::env::temp_dir().join(format!("sprk-naming-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.md"), "x").unwrap();
        assert_eq!(unique_path(&d, "a.md"), d.join("a (2).md"));
        std::fs::write(d.join("a (2).md"), "x").unwrap();
        assert_eq!(unique_path(&d, "a.md"), d.join("a (3).md"));
        std::fs::remove_dir_all(&d).unwrap();
    }
}

//! Retention (SPEC 6.6): transcripts older than the chosen number of days are removed from the
//! library through a [`Trash`] (the app's one moves them to the Recycle Bin, or deletes them
//! when the Recycle Bin can't take them). A transcript still being written, or open in the
//! Library, is never touched. Pure std plus the library scan, so every rule here is tested on
//! Linux. Nothing here logs titles or text; the result is counts only.
use crate::library::{self, Entry, Trash};
use std::path::{Path, PathBuf};

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 for the `YYYY-MM-DD` at the start of `s`: a front matter `date`, or a
/// time such as `2026-11-18T13:42:15-06:00`, whose date part is the local date it was written.
/// None when there is no real date there.
pub fn day_number(s: &str) -> Option<i64> {
    let s = s.trim().trim_matches('"');
    let b = s.as_bytes();
    let digits = |r: std::ops::Range<usize>| r.into_iter().all(|i| b[i].is_ascii_digit());
    if b.len() < 10
        || b[4] != b'-'
        || b[7] != b'-'
        || !digits(0..4)
        || !digits(5..7)
        || !digits(8..10)
        || b.get(10).is_some_and(|c| *c != b'T' && *c != b' ')
    {
        return None;
    }
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let m: i64 = s.get(5..7)?.parse().ok()?;
    let d: i64 = s.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

/// The day a transcript's age counts from: the day it last ended (a continued transcript counts
/// from its newest session), else the day it started, else its front matter date.
pub fn last_day(e: &Entry) -> Option<i64> {
    day_number(&e.end)
        .or_else(|| day_number(&e.start))
        .or_else(|| day_number(&e.date))
}

/// Past keeping on `today`? With 30 days, a transcript that ended on the 1st is kept through
/// the 31st and goes on the 32nd. `keep_days` of 0 means keep forever.
pub fn is_expired(day: i64, today: i64, keep_days: u64) -> bool {
    if keep_days == 0 {
        return false;
    }
    let keep = i64::try_from(keep_days).unwrap_or(i64::MAX);
    today.saturating_sub(day) > keep
}

fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Transcripts under `root` that are past keeping on `today` (days since 1970-01-01, local).
/// Left alone: a transcript still marked in progress (being written now, or waiting for crash
/// recovery), any path in `protected` (the running session's file, the one open in the
/// Library), and one without a readable date.
pub fn due(root: &Path, keep_days: u64, today: i64, protected: &[PathBuf]) -> Vec<Entry> {
    if keep_days == 0 {
        return Vec::new();
    }
    library::scan(root)
        .into_iter()
        .filter(|e| e.status != "in-progress")
        .filter(|e| last_day(e).is_some_and(|d| is_expired(d, today, keep_days)))
        .filter(|e| !protected.iter().any(|p| same_file(&e.path, p)))
        .collect()
}

/// What a sweep did, in counts only.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Swept {
    pub removed: usize,
    pub failed: usize,
}

/// Remove every transcript [`due`] now, with its sidecar, through `trash`. A transcript that
/// can't be removed stays and is counted; the next sweep tries again.
pub fn sweep(
    root: &Path,
    keep_days: u64,
    today: i64,
    protected: &[PathBuf],
    trash: &dyn Trash,
) -> Swept {
    let mut out = Swept::default();
    for e in due(root, keep_days, today, protected) {
        match library::delete(root, &e.path, trash) {
            Ok(()) => out.removed += 1,
            Err(_) => out.failed += 1,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn md(date: &str, start: &str, end: Option<&str>, status: &str) -> String {
        let end = end.map(|e| format!("end: \"{e}\"\n")).unwrap_or_default();
        format!(
            "---\nformat: sprknotes-transcript/1\ntitle: \"T\"\ndate: {date}\nstart: \"{start}\"\n{end}duration: \"0:10:00\"\nstatus: {status}\n---\n# T\n\n## Transcript\n\n- `00:00:04` **Me:** hello\n"
        )
    }

    struct Bin(RefCell<Vec<PathBuf>>, bool);
    impl Trash for Bin {
        fn trash(&self, paths: &[PathBuf]) -> std::io::Result<()> {
            if !self.1 {
                return Err(std::io::ErrorKind::PermissionDenied.into());
            }
            for p in paths {
                std::fs::remove_file(p)?;
                self.0.borrow_mut().push(p.clone());
            }
            Ok(())
        }
    }

    fn today() -> i64 {
        day_number("2026-11-26").unwrap_or_default()
    }

    #[test]
    fn dates_parse_from_the_front_matter_and_times() {
        assert_eq!(day_number("1970-01-01"), Some(0));
        assert_eq!(day_number("2000-03-01"), Some(11_017));
        assert_eq!(
            day_number("2026-11-18T13:42:15-06:00"),
            day_number("2026-11-18")
        );
        assert_eq!(day_number("\"2026-11-18\""), day_number("2026-11-18"));
        assert_eq!(
            day_number("2024-02-29").map(|d| d + 1),
            day_number("2024-03-01")
        );
        for bad in [
            "",
            "09:00",
            "2026-02-29",
            "2026-13-01",
            "2026-11-31",
            "2026-1-01",
            "+026-11-18",
            "2026-11-18x",
        ] {
            assert_eq!(day_number(bad), None, "{bad}");
        }
    }

    #[test]
    fn age_counts_whole_days_and_zero_keeps_forever() {
        let ended = day_number("2026-10-27").unwrap_or_default();
        assert!(!is_expired(ended, today(), 30), "30 days after: kept");
        assert!(is_expired(ended - 1, today(), 30), "31 days after: gone");
        assert!(!is_expired(ended - 10_000, today(), 0));
        assert!(
            !is_expired(today() + 3, today(), 30),
            "a clock set back never deletes"
        );
        assert!(is_expired(
            day_number("2025-11-25").unwrap_or_default(),
            today(),
            365
        ));
        assert!(!is_expired(
            day_number("2025-11-26").unwrap_or_default(),
            today(),
            365
        ));
    }

    #[test]
    fn age_counts_from_the_last_end_then_the_start_then_the_date() {
        let e = |date: &str, start: &str, end: &str| Entry {
            path: PathBuf::new(),
            name: String::new(),
            title: String::new(),
            date: date.into(),
            start: start.into(),
            end: end.into(),
            duration: String::new(),
            status: "complete".into(),
            folder: String::new(),
        };
        let d = |s: &str| day_number(s);
        assert_eq!(
            last_day(&e(
                "2026-01-01",
                "2026-01-01T09:00:00-06:00",
                "2026-11-20T10:00:00-06:00"
            )),
            d("2026-11-20")
        );
        assert_eq!(
            last_day(&e("2026-01-01", "2026-01-02T09:00:00-06:00", "")),
            d("2026-01-02")
        );
        assert_eq!(last_day(&e("2026-01-01", "09:00", "")), d("2026-01-01"));
        assert_eq!(last_day(&e("", "09:00", "")), None);
    }

    #[test]
    fn sweep_removes_only_old_finished_transcripts_and_their_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".sprknotes")).unwrap();
        std::fs::create_dir_all(root.join("Clients")).unwrap();
        let w = |name: &str, body: String| {
            let p = root.join(name);
            std::fs::write(&p, body).unwrap();
            p
        };
        let old = w(
            "old.md",
            md(
                "2026-01-05",
                "2026-01-05T09:00:00-06:00",
                Some("2026-01-05T09:30:00-06:00"),
                "complete",
            ),
        );
        std::fs::write(root.join(".sprknotes/old.jsonl"), "{}\n").unwrap();
        let old_sub = w(
            "Clients/old2.md",
            md("2026-02-01", "09:00", None, "interrupted"),
        );
        let recent = w(
            "recent.md",
            md(
                "2026-11-20",
                "2026-11-20T09:00:00-06:00",
                Some("2026-11-20T09:30:00-06:00"),
                "complete",
            ),
        );
        let continued = w(
            "continued.md",
            md(
                "2026-01-05",
                "2026-01-05T09:00:00-06:00",
                Some("2026-11-25T09:30:00-06:00"),
                "complete",
            ),
        );
        let crashed = w(
            "crashed.md",
            md(
                "2026-01-05",
                "2026-01-05T09:00:00-06:00",
                None,
                "in-progress",
            ),
        );
        let undated = w("undated.md", md("someday", "09:00", None, "complete"));
        let summary = w("old - Summary.md", "# Summary\nold notes".into());

        // Forever never removes anything.
        let bin = Bin(RefCell::default(), true);
        assert_eq!(sweep(root, 0, today(), &[], &bin), Swept::default());
        assert!(bin.0.borrow().is_empty());

        let s = sweep(root, 30, today(), &[], &bin);
        assert_eq!(
            s,
            Swept {
                removed: 2,
                failed: 0
            }
        );
        assert!(!old.exists() && !old_sub.exists());
        assert!(
            !root.join(".sprknotes/old.jsonl").exists(),
            "the sidecar goes too"
        );
        for kept in [&recent, &continued, &crashed, &undated, &summary] {
            assert!(kept.exists(), "{}", kept.display());
        }
        // A year keeps everything that is left.
        assert_eq!(sweep(root, 365, today(), &[], &bin).removed, 0);
    }

    #[test]
    fn sweep_never_touches_the_active_or_open_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let body = md(
            "2025-06-01",
            "2025-06-01T09:00:00-06:00",
            Some("2025-06-01T09:30:00-06:00"),
            "complete",
        );
        let active = root.join("active.md");
        let open = root.join("open.md");
        let other = root.join("other.md");
        for p in [&active, &open, &other] {
            std::fs::write(p, &body).unwrap();
        }
        let bin = Bin(RefCell::default(), true);
        // The open file is passed by another spelling of the same path.
        let open_alias = root.join(".").join("open.md");
        let s = sweep(root, 30, today(), &[active.clone(), open_alias], &bin);
        assert_eq!(s.removed, 1);
        assert!(active.exists());
        assert!(open.exists());
        assert!(!other.exists());
        assert_eq!(*bin.0.borrow(), vec![other]);
    }

    #[test]
    fn a_refused_removal_leaves_the_file_and_is_counted() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("old.md");
        std::fs::write(&p, md("2025-06-01", "09:00", None, "complete")).unwrap();
        let refusing = Bin(RefCell::default(), false);
        assert_eq!(
            sweep(dir.path(), 90, today(), &[], &refusing),
            Swept {
                removed: 0,
                failed: 1
            }
        );
        assert!(p.exists());
    }
}

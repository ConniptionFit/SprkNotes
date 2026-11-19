//! Recovery of transcripts left `in-progress` by a crash (SPEC 6.4).
use crate::format::{event_line, Status};
use crate::writer::{rewrite_head, strip_front_matter};
use crate::TranscriptError;
use std::path::{Path, PathBuf};

fn front_matter_field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    rest[..end]
        .lines()
        .find_map(|l| l.strip_prefix(key).and_then(|v| v.strip_prefix(':')))
        .map(str::trim)
}

/// All `status: in-progress` sprknotes transcripts under `root` (recursive, skips hidden
/// folders such as `.sprknotes`). `is_live` lets the caller exclude a session it owns.
pub fn find_in_progress(root: &Path, is_live: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            if path.is_dir() && !hidden {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md") && !is_live(&path) {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if front_matter_field(&text, "format") == Some(crate::FORMAT)
                        && front_matter_field(&text, "status") == Some("in-progress")
                    {
                        found.push(path);
                    }
                }
            }
        }
    }
    found.sort();
    found
}

/// Last `HH:MM:SS` stamp in the body, in milliseconds (0 if there is none).
fn last_stamp_ms(body: &str) -> u64 {
    body.lines()
        .rev()
        .find_map(|l| {
            let t = l.trim_start_matches(['-', '>', ' ']).strip_prefix('`')?;
            let t = &t[..t.find('`')?];
            let mut p = t.split(':').map(|x| x.parse::<u64>().ok());
            let (h, m, s) = (p.next()??, p.next()??, p.next()??);
            Some((h * 3600 + m * 60 + s) * 1000)
        })
        .unwrap_or(0)
}

/// "Mark finished": append a `Transcription interrupted` event and set `status: interrupted`.
pub fn mark_interrupted(path: &Path) -> Result<(), TranscriptError> {
    use std::io::Write;
    let text = std::fs::read_to_string(path)?;
    let body = strip_front_matter(&text).ok_or(TranscriptError::NotATranscript)?;
    let at = last_stamp_ms(body);
    {
        let mut f = std::fs::OpenOptions::new().append(true).open(path)?;
        let sep = if text.ends_with('\n') { "\n" } else { "\n\n" };
        f.write_all(format!("{sep}{}\n", event_line(at, "Transcription interrupted")).as_bytes())?;
        f.sync_all()?;
    }
    let meta = parse_meta(&std::fs::read_to_string(path)?)?;
    rewrite_head(path, &meta)
}

fn parse_meta(text: &str) -> Result<crate::format::Meta, TranscriptError> {
    use crate::format::{Meta, NameSource};
    let get = |k: &str| front_matter_field(text, k).map(|v| v.trim_matches('"').to_string());
    let list = |k: &str| -> Vec<String> {
        get(k)
            .map(|v| {
                v.trim_start_matches('[')
                    .trim_end_matches(']')
                    .split(',')
                    .map(|x| x.trim().trim_matches('"').to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };
    let title = get("title").ok_or(TranscriptError::NotATranscript)?;
    Ok(Meta {
        title,
        date: get("date").unwrap_or_default(),
        start: get("start").unwrap_or_default(),
        end: get("end"),
        duration: get("duration"),
        status: Status::Interrupted,
        sources: list("sources"),
        speakers: list("speakers"),
        name_source: match get("name_source").as_deref() {
            Some("calendar") => NameSource::Calendar,
            Some("call") => NameSource::Call,
            Some("audio-source") => NameSource::AudioSource,
            Some("user") => NameSource::User,
            _ => NameSource::Default,
        },
        engine: get("engine").unwrap_or_default(),
        app: get("app").unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{Meta, NameSource};
    use crate::writer::TranscriptWriter;

    fn meta() -> Meta {
        Meta {
            title: "Crash Test".into(),
            date: "2026-11-18".into(),
            start: "2026-11-18T09:00:00-06:00".into(),
            end: None,
            duration: None,
            status: Status::InProgress,
            sources: vec!["mic".into()],
            speakers: vec!["Me".into()],
            name_source: NameSource::Calendar,
            engine: "e".into(),
            app: "SprkNotes 0.0.1".into(),
        }
    }

    #[test]
    fn crash_then_recover() {
        let d = std::env::temp_dir().join(format!("sprk-recovery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".sprknotes")).unwrap();
        let mut w = TranscriptWriter::create(d.join("t.md"), None, meta(), None).unwrap();
        w.append_statement(125_000, 127_000, "Me", "mic", "committed before the crash")
            .unwrap();
        std::mem::forget(w); // killed: never finalized

        // a finished transcript must not show up
        let done = TranscriptWriter::create(d.join("done.md"), None, meta(), None).unwrap();
        done.finalize("e", "d", Status::Complete).unwrap();

        let found = find_in_progress(&d, &|_| false);
        assert_eq!(found, vec![d.join("t.md")]);
        assert!(
            find_in_progress(&d, &|p| p == d.join("t.md")).is_empty(),
            "live sessions are skipped"
        );

        mark_interrupted(&found[0]).unwrap();
        let s = std::fs::read_to_string(&found[0]).unwrap();
        assert!(s.contains("status: interrupted"));
        assert!(s.contains("committed before the crash"));
        assert!(s.contains("> `00:02:05` Transcription interrupted"));
        assert!(s.contains("name_source: calendar"));
        assert!(find_in_progress(&d, &|_| false).is_empty());
        std::fs::remove_dir_all(d).unwrap();
    }
}

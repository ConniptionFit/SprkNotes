//! Crash-safe transcript writer (SPEC 6.3): every committed line is appended and flushed
//! to disk immediately. The file is rewritten only once, at finalize, via temp file + rename.
use crate::format::{event_line, statement_line, NameSource, Status};
use crate::{format::Meta, TranscriptError};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct TranscriptWriter {
    path: PathBuf,
    file: File,
    sidecar: Option<File>,
    meta: Meta,
    last_speaker: Option<String>,
    next_id: u64,
}

fn open_append(path: &Path, create_new: bool) -> std::io::Result<File> {
    let mut o = OpenOptions::new();
    o.append(true);
    if create_new {
        o.create_new(true);
    } else {
        o.create(true);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // READ | WRITE | DELETE sharing so the open file can be renamed (SPEC 6.2).
        o.share_mode(0x1 | 0x2 | 0x4);
    }
    o.open(path)
}

/// Minimal JSON string escaping for the sidecar (no serde needed).
fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

impl TranscriptWriter {
    /// Create the file with front matter (`status: in-progress`), heading and `## Transcript`.
    /// `context_section` is the optional "Meeting context" block (already formatted, SPEC 7.3).
    pub fn create(
        path: PathBuf,
        sidecar: Option<PathBuf>,
        mut meta: Meta,
        context_section: Option<&str>,
    ) -> Result<Self, TranscriptError> {
        meta.status = Status::InProgress;
        let mut file = open_append(&path, true)?;
        let mut head = meta.front_matter();
        head.push_str(&format!("\n# {}\n\n", meta.title));
        if let Some(c) = context_section {
            head.push_str(c);
            head.push_str("\n\n");
        }
        head.push_str("## Transcript\n\n");
        file.write_all(head.as_bytes())?;
        file.sync_data()?;
        let sidecar = match sidecar {
            Some(p) => {
                if let Some(dir) = p.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                Some(open_append(&p, false)?)
            }
            None => None,
        };
        Ok(Self {
            path,
            file,
            sidecar,
            meta,
            last_speaker: None,
            next_id: 1,
        })
    }

    /// Continue an existing transcript (SPEC 6.5): the file goes back to `in-progress`, keeps
    /// every line and gets an event line saying where transcription continued. Returns the writer
    /// and the time (ms) the new lines start from, just after the last saved one, so the
    /// timestamps keep counting up. `app` and `engine` are this session's.
    pub fn reopen(
        path: PathBuf,
        sidecar: Option<PathBuf>,
        app: &str,
        engine: &str,
    ) -> Result<(Self, u64), TranscriptError> {
        let text = std::fs::read_to_string(&path)?;
        strip_front_matter(&text).ok_or(TranscriptError::NotATranscript)?;
        let side = sidecar
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok());
        let parsed = crate::parse::Parsed::parse(&text, side.as_deref());
        let last = parsed
            .segments
            .iter()
            .map(|s| s.end_ms.max(s.start_ms))
            .chain(parsed.events.iter().map(|e| e.t_ms))
            .max()
            .unwrap_or(0)
            .max(duration_ms(&parsed.duration));
        // Start on the next whole second so the continued lines never share the last stamp.
        let offset = (last / 1000 + 1) * 1000;
        let meta = Meta {
            title: parsed.title.clone(),
            date: parsed.date.clone(),
            start: parsed.start.clone(),
            end: None,
            duration: None,
            status: Status::InProgress,
            sources: if parsed.sources.is_empty() {
                vec!["mic".into(), "system".into()]
            } else {
                parsed.sources.clone()
            },
            speakers: if parsed.speakers.is_empty() {
                vec!["Me".into(), "Others".into()]
            } else {
                parsed.speakers.clone()
            },
            name_source: NameSource::parse(&parsed.name_source),
            engine: engine.to_string(),
            app: app.to_string(),
        };
        rewrite_head(&path, &meta)?;
        let file = open_append(&path, false)?;
        let sidecar = match sidecar {
            Some(p) => {
                if let Some(dir) = p.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                Some(open_append(&p, false)?)
            }
            None => None,
        };
        let mut w = Self {
            path,
            file,
            sidecar,
            meta,
            last_speaker: None,
            next_id: parsed.segments.len() as u64 + 1,
        };
        w.append_event(offset, "Transcription continued")?;
        Ok((w, offset))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn write_flush(&mut self, text: &str) -> Result<(), TranscriptError> {
        self.file.write_all(text.as_bytes())?;
        self.file.sync_data()?;
        Ok(())
    }

    /// Append one committed statement. A blank line is inserted when the speaker changes.
    pub fn append_statement(
        &mut self,
        start_ms: u64,
        end_ms: u64,
        speaker: &str,
        source: &str,
        text: &str,
    ) -> Result<(), TranscriptError> {
        let mut out = String::new();
        if self.last_speaker.as_deref().is_some_and(|s| s != speaker) {
            out.push('\n');
        }
        out.push_str(&statement_line(start_ms, speaker, text));
        out.push('\n');
        self.write_flush(&out)?;
        self.last_speaker = Some(speaker.to_string());
        if let Some(sc) = self.sidecar.as_mut() {
            let line = format!(
                "{{\"id\":{},\"start_ms\":{},\"end_ms\":{},\"speaker\":{},\"source\":{},\"text\":{}}}\n",
                self.next_id,
                start_ms,
                end_ms,
                json_str(speaker),
                json_str(source),
                json_str(text)
            );
            sc.write_all(line.as_bytes())?;
            sc.sync_data()?;
        }
        self.next_id += 1;
        Ok(())
    }

    /// Append an event line (pause, resume, source change...). Resets speaker grouping.
    pub fn append_event(&mut self, at_ms: u64, text: &str) -> Result<(), TranscriptError> {
        let out = format!("\n{}\n", event_line(at_ms, text));
        self.write_flush(&out)?;
        self.last_speaker = None;
        if let Some(sc) = self.sidecar.as_mut() {
            let line = format!(
                "{{\"event\":true,\"t_ms\":{},\"type\":{}}}\n",
                at_ms,
                json_str(text)
            );
            sc.write_all(line.as_bytes())?;
            sc.sync_data()?;
        }
        Ok(())
    }

    /// Move the open file to a new path (same directory rules decided by the caller).
    pub fn rename(
        &mut self,
        new_path: PathBuf,
        new_title: &str,
        source: NameSource,
    ) -> Result<(), TranscriptError> {
        // A name the user typed always wins; lower-ranked names never replace it.
        if source < self.meta.name_source {
            return Ok(());
        }
        std::fs::rename(&self.path, &new_path)?;
        self.path = new_path;
        self.meta.title = new_title.to_string();
        self.meta.name_source = source;
        Ok(())
    }

    pub fn meta(&self) -> &Meta {
        &self.meta
    }

    /// Rewrite the front matter (end, duration, status) and heading atomically.
    /// Consumes the writer, which closes the open handle first.
    pub fn finalize(
        mut self,
        end: &str,
        duration: &str,
        status: Status,
    ) -> Result<PathBuf, TranscriptError> {
        self.meta.end = Some(end.to_string());
        self.meta.duration = Some(duration.to_string());
        self.meta.status = status;
        let path = self.path.clone();
        let meta = self.meta.clone();
        drop(self);
        rewrite_head(&path, &meta)?;
        Ok(path)
    }
}

/// `H:MM:SS` (front matter `duration`) to ms; 0 when absent or unreadable.
fn duration_ms(v: &str) -> u64 {
    let parts: Vec<u64> = v.trim().split(':').filter_map(|p| p.parse().ok()).collect();
    match parts.as_slice() {
        [h, m, s] => (h * 3600 + m * 60 + s) * 1000,
        [m, s] => (m * 60 + s) * 1000,
        _ => 0,
    }
}

/// Drop `## Participants` and `## Audio sources` sections so rewriting is idempotent.
fn strip_generated_sections(body: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in body.split_inclusive('\n') {
        if line.starts_with("## ") {
            skipping = line.starts_with("## Participants") || line.starts_with("## Audio sources");
        }
        if !skipping {
            out.push_str(line);
        }
    }
    out
}

/// Replace the front matter block and `# Title` heading, add the Participants and Audio sources
/// sections rebuilt from the event lines, and keep every statement and event line unchanged.
pub(crate) fn rewrite_head(path: &Path, meta: &Meta) -> Result<(), TranscriptError> {
    let text = std::fs::read_to_string(path)?;
    let rest = strip_front_matter(&text).ok_or(TranscriptError::NotATranscript)?;
    let rest = strip_generated_sections(rest);
    let extras = crate::extras::Extras::from_events(&rest);
    let mut body = String::new();
    let mut heading_done = false;
    for line in rest.split_inclusive('\n') {
        if !heading_done && line.starts_with("# ") {
            body.push_str(&format!("# {}\n", meta.title));
            heading_done = true;
        } else if line.starts_with("## Transcript") {
            body.push_str(&extras.sections());
            body.push_str(line);
        } else {
            body.push_str(line);
        }
    }
    let new = format!(
        "{}{}",
        meta.front_matter_with(&extras.front_matter_blocks()),
        body
    );
    let tmp = path.with_extension("md.tmp");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(new.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Text after the closing `---` of the front matter, or None if there is none.
pub(crate) fn strip_front_matter(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("---\n")?;
    let idx = rest.find("\n---\n")?;
    Some(&rest[idx + 5..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> Meta {
        Meta {
            title: "Weekly IT Sync".into(),
            date: "2026-11-18".into(),
            start: "2026-11-18T13:00:04-06:00".into(),
            end: None,
            duration: None,
            status: Status::InProgress,
            sources: vec!["mic".into(), "system".into()],
            speakers: vec!["Me".into(), "Others".into()],
            name_source: NameSource::Default,
            engine: "test-engine".into(),
            app: "SprkNotes 0.0.1".into(),
        }
    }

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sprk-writer-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn create_writes_header_in_progress() {
        let d = tmpdir("create");
        let w = TranscriptWriter::create(d.join("t.md"), None, meta(), None).unwrap();
        let s = std::fs::read_to_string(w.path()).unwrap();
        assert!(s.starts_with("---\nformat: sprknotes-transcript/1\n"));
        assert!(s.contains("status: in-progress"));
        assert!(s.contains("# Weekly IT Sync"));
        assert!(s.ends_with("## Transcript\n\n"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn lines_are_on_disk_before_any_shutdown() {
        // Crash safety: read the file while the writer is still open, no finalize.
        let d = tmpdir("crash");
        let mut w = TranscriptWriter::create(d.join("t.md"), None, meta(), None).unwrap();
        w.append_statement(4_000, 6_000, "Me", "mic", "first line")
            .unwrap();
        w.append_statement(9_000, 11_000, "Others", "system", "second\nline")
            .unwrap();
        let s = std::fs::read_to_string(w.path()).unwrap();
        assert!(
            s.contains("- `00:00:04` **Me:** first line\n\n- `00:00:09` **Others:** second line\n")
        );
        std::mem::forget(w); // simulate a kill: no drop, no finalize
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn same_speaker_has_no_blank_line_and_events_reset_grouping() {
        let d = tmpdir("group");
        let mut w = TranscriptWriter::create(d.join("t.md"), None, meta(), None).unwrap();
        w.append_statement(1_000, 2_000, "Me", "mic", "a").unwrap();
        w.append_statement(3_000, 4_000, "Me", "mic", "b").unwrap();
        w.append_event(842_000, "Transcription paused").unwrap();
        w.append_statement(900_000, 901_000, "Me", "mic", "c")
            .unwrap();
        let s = std::fs::read_to_string(w.path()).unwrap();
        assert!(s.contains("**Me:** a\n- `00:00:03` **Me:** b\n"));
        assert!(s.contains("\n> `00:14:02` Transcription paused\n"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn sidecar_gets_jsonl_with_escaping() {
        let d = tmpdir("sidecar");
        let sc = d.join(".sprknotes").join("s1.jsonl");
        let mut w =
            TranscriptWriter::create(d.join("t.md"), Some(sc.clone()), meta(), None).unwrap();
        w.append_statement(1, 2, "Me", "mic", "say \"hi\"").unwrap();
        let j = std::fs::read_to_string(sc).unwrap();
        assert_eq!(
            j,
            "{\"id\":1,\"start_ms\":1,\"end_ms\":2,\"speaker\":\"Me\",\"source\":\"mic\",\"text\":\"say \\\"hi\\\"\"}\n"
        );
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn finalize_rewrites_only_the_head() {
        let d = tmpdir("final");
        let mut w = TranscriptWriter::create(d.join("t.md"), None, meta(), None).unwrap();
        w.append_statement(4_000, 6_000, "Me", "mic", "body stays")
            .unwrap();
        let p = w
            .finalize("2026-11-18T13:42:15-06:00", "00:42:11", Status::Complete)
            .unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("status: complete"));
        assert!(s.contains("end: \"2026-11-18T13:42:15-06:00\""));
        assert!(s.contains("duration: \"00:42:11\""));
        assert!(!s.contains("in-progress"));
        assert!(s.ends_with("- `00:00:04` **Me:** body stays\n"));
        assert!(!d.join("t.md.tmp").exists());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn reopen_continues_after_the_last_line_and_keeps_everything() {
        let d = tmpdir("reopen");
        let side = d.join(".sprknotes").join("t.jsonl");
        let mut m = meta();
        m.name_source = NameSource::User;
        let mut w = TranscriptWriter::create(d.join("t.md"), Some(side.clone()), m, None).unwrap();
        w.append_statement(4_000, 6_500, "Me", "mic", "first part")
            .unwrap();
        let p = w
            .finalize("2026-11-18T13:42:15-06:00", "0:00:07", Status::Complete)
            .unwrap();

        let (mut w, offset) =
            TranscriptWriter::reopen(p.clone(), Some(side.clone()), "SprkNotes 0.0.2", "e2")
                .unwrap();
        assert_eq!(offset, 8_000);
        assert_eq!(w.meta().name_source, NameSource::User);
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("status: in-progress"));
        assert!(!s.contains("duration:"));
        w.append_statement(
            offset + 1_000,
            offset + 2_000,
            "Others",
            "system",
            "second part",
        )
        .unwrap();
        let p = w
            .finalize("2026-11-18T14:00:00-06:00", "0:00:10", Status::Complete)
            .unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("- `00:00:04` **Me:** first part"));
        assert!(s.contains("> `00:00:08` Transcription continued"));
        assert!(s.contains("- `00:00:09` **Others:** second part"));
        assert!(s.contains("app: \"SprkNotes 0.0.2\""));
        let parsed =
            crate::parse::Parsed::parse(&s, std::fs::read_to_string(&side).ok().as_deref());
        assert_eq!(parsed.segments.len(), 2);
        assert_eq!(parsed.segments[1].end_ms, 10_000);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn reopen_refuses_a_file_that_is_not_a_transcript() {
        let d = tmpdir("reopen-bad");
        std::fs::write(d.join("x.md"), "hello").unwrap();
        assert!(TranscriptWriter::reopen(d.join("x.md"), None, "a", "e").is_err());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn rename_respects_name_source_rank() {
        let d = tmpdir("rename");
        let mut w = TranscriptWriter::create(d.join("a.md"), None, meta(), None).unwrap();
        w.rename(d.join("b.md"), "Calendar Name", NameSource::Calendar)
            .unwrap();
        assert!(d.join("b.md").exists() && !d.join("a.md").exists());
        w.rename(d.join("c.md"), "User Name", NameSource::User)
            .unwrap();
        // a lower-ranked source can no longer rename once the user typed a name
        w.rename(d.join("d.md"), "Audio Name", NameSource::AudioSource)
            .unwrap();
        assert!(d.join("c.md").exists() && !d.join("d.md").exists());
        assert_eq!(w.meta().title, "User Name");
        let p = w.finalize("e", "d", Status::Complete).unwrap();
        assert!(std::fs::read_to_string(p)
            .unwrap()
            .contains("# User Name\n"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn create_refuses_to_overwrite() {
        let d = tmpdir("noclobber");
        std::fs::write(d.join("t.md"), "x").unwrap();
        assert!(TranscriptWriter::create(d.join("t.md"), None, meta(), None).is_err());
        std::fs::remove_dir_all(d).unwrap();
    }
}

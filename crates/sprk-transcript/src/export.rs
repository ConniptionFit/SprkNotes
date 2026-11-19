//! Exporters (SPEC 5.2) behind one trait so new formats are adapters (SPEC 17).
//! Output is generated on demand from a [`Parsed`] transcript; nothing here touches disk.
use crate::format::timestamp;
use crate::parse::Parsed;

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("could not build the file: {0}")]
    Build(String),
}

pub trait Exporter {
    fn name(&self) -> &'static str;
    fn extension(&self) -> &'static str;
    fn export(&self, t: &Parsed) -> Result<Vec<u8>, ExportError>;
}

/// All built-in exporters.
pub fn all() -> Vec<Box<dyn Exporter>> {
    vec![Box::new(Txt), Box::new(Json), Box::new(Docx)]
}

pub fn by_extension(ext: &str) -> Option<Box<dyn Exporter>> {
    all().into_iter().find(|e| {
        e.extension()
            .eq_ignore_ascii_case(ext.trim_start_matches('.'))
    })
}

fn header_pairs(t: &Parsed) -> Vec<(&'static str, String)> {
    let mut v = vec![
        ("Title", t.title.clone()),
        ("Date", t.date.clone()),
        ("Start", t.start.clone()),
    ];
    if !t.end.is_empty() {
        v.push(("End", t.end.clone()));
    }
    if !t.duration.is_empty() {
        v.push(("Duration", t.duration.clone()));
    }
    v.push(("Status", t.status.clone()));
    if !t.speakers.is_empty() {
        v.push(("Speakers", t.speakers.join(", ")));
    }
    v
}

pub struct Txt;
impl Exporter for Txt {
    fn name(&self) -> &'static str {
        "Plain text"
    }
    fn extension(&self) -> &'static str {
        "txt"
    }
    fn export(&self, t: &Parsed) -> Result<Vec<u8>, ExportError> {
        let mut s = String::new();
        for (k, v) in header_pairs(t) {
            s.push_str(&format!("{k}: {v}\n"));
        }
        s.push('\n');
        // Statements and events interleaved by time (events win ties so a pause reads first).
        let mut rows: Vec<(u64, u8, String)> = Vec::new();
        for g in &t.segments {
            rows.push((
                g.start_ms,
                1,
                format!("[{}] {}: {}", timestamp(g.start_ms), g.speaker, g.text),
            ));
        }
        for e in &t.events {
            rows.push((
                e.t_ms,
                0,
                format!("[{}] -- {} --", timestamp(e.t_ms), e.text),
            ));
        }
        rows.sort_by_key(|r| (r.0, r.1));
        for (_, _, l) in rows {
            s.push_str(&l);
            s.push('\n');
        }
        Ok(s.into_bytes())
    }
}

pub struct Json;
impl Exporter for Json {
    fn name(&self) -> &'static str {
        "JSON"
    }
    fn extension(&self) -> &'static str {
        "json"
    }
    fn export(&self, t: &Parsed) -> Result<Vec<u8>, ExportError> {
        use serde_json::json;
        let v = json!({
            "format": t.format,
            "meta": {
                "title": t.title, "date": t.date, "start": t.start, "end": t.end,
                "duration": t.duration, "status": t.status, "sources": t.sources,
                "speakers": t.speakers, "name_source": t.name_source,
                "engine": t.engine, "app": t.app,
            },
            "meeting": { "context": t.meeting_context },
            "segments": t.segments.iter().map(|g| json!({
                "id": g.id, "start_ms": g.start_ms, "end_ms": g.end_ms,
                "speaker": g.speaker, "source": g.source, "text": g.text,
            })).collect::<Vec<_>>(),
            "events": t.events.iter().map(|e| json!({"t_ms": e.t_ms, "type": e.text})).collect::<Vec<_>>(),
            "audio_sources": t.extras.audio_sources.iter().map(|a| json!({
                "app": a.app, "title": a.title, "start": a.start, "end": a.end, "duration": a.duration,
            })).collect::<Vec<_>>(),
        });
        serde_json::to_vec_pretty(&v).map_err(|e| ExportError::Build(e.to_string()))
    }
}

pub struct Docx;
impl Exporter for Docx {
    fn name(&self) -> &'static str {
        "Word document"
    }
    fn extension(&self) -> &'static str {
        "docx"
    }
    fn export(&self, t: &Parsed) -> Result<Vec<u8>, ExportError> {
        use docx_rs::{Docx as D, Paragraph, Run, Table, TableCell, TableRow};
        let cell = |s: &str| {
            TableCell::new().add_paragraph(Paragraph::new().add_run(Run::new().add_text(s)))
        };
        let mut doc = D::new()
            .add_paragraph(Paragraph::new().add_run(Run::new().add_text(&t.title).bold().size(36)));
        let rows: Vec<TableRow> = header_pairs(t)
            .into_iter()
            .filter(|(k, _)| *k != "Title")
            .map(|(k, v)| TableRow::new(vec![cell(k), cell(&v)]))
            .collect();
        doc = doc.add_table(Table::new(rows));
        if !t.meeting_context.is_empty() {
            doc = doc.add_paragraph(
                Paragraph::new().add_run(Run::new().add_text("Meeting context").bold()),
            );
            for l in t.meeting_context.lines().filter(|l| !l.trim().is_empty()) {
                doc = doc.add_paragraph(Paragraph::new().add_run(Run::new().add_text(l)));
            }
        }
        doc = doc.add_paragraph(Paragraph::new().add_run(Run::new().add_text("Transcript").bold()));
        for g in &t.segments {
            doc = doc.add_paragraph(
                Paragraph::new()
                    .add_run(Run::new().add_text(format!("[{}] ", timestamp(g.start_ms))))
                    .add_run(Run::new().add_text(format!("{}: ", g.speaker)).bold())
                    .add_run(Run::new().add_text(&g.text)),
            );
        }
        let mut buf = std::io::Cursor::new(Vec::new());
        doc.build()
            .pack(&mut buf)
            .map_err(|e| ExportError::Build(e.to_string()))?;
        Ok(buf.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Parsed {
        Parsed::parse(
            "---\nformat: sprknotes-transcript/1\ntitle: \"Sync\"\ndate: 2026-11-19\nstart: \"09:00\"\nstatus: complete\nspeakers: [\"Me\", \"Others\"]\n---\n# Sync\n\n## Transcript\n\n- `00:00:04` **Me:** hello\n\n> `00:00:10` Transcription paused\n\n- `00:00:20` **Others:** hi\n",
            None,
        )
    }

    #[test]
    fn txt_has_header_and_lines() {
        let s = String::from_utf8(Txt.export(&sample()).unwrap()).unwrap();
        assert!(s.starts_with("Title: Sync\nDate: 2026-11-19\n"));
        assert!(s.contains("[00:00:04] Me: hello\n"));
        assert!(s.contains("[00:00:10] -- Transcription paused --\n"));
        assert!(s.find("paused").unwrap() < s.find("Others: hi").unwrap());
    }

    #[test]
    fn json_matches_spec_shape() {
        let v: serde_json::Value =
            serde_json::from_slice(&Json.export(&sample()).unwrap()).unwrap();
        assert_eq!(v["format"], "sprknotes-transcript/1");
        assert_eq!(v["meta"]["title"], "Sync");
        assert_eq!(v["segments"][1]["speaker"], "Others");
        assert_eq!(v["segments"][0]["end_ms"], 20_000);
        assert_eq!(v["events"][0]["t_ms"], 10_000);
        assert!(v["audio_sources"].is_array());
    }

    #[test]
    fn docx_is_a_zip_with_document_xml() {
        let b = Docx.export(&sample()).unwrap();
        assert_eq!(&b[..2], b"PK");
        let mut z = zip_names(&b);
        z.sort();
        assert!(z.iter().any(|n| n == "word/document.xml"));
    }

    fn zip_names(b: &[u8]) -> Vec<String> {
        // Central directory entries carry the plain file names.
        let s = String::from_utf8_lossy(b);
        ["word/document.xml", "[Content_Types].xml"]
            .iter()
            .filter(|n| s.contains(**n))
            .map(|n| n.to_string())
            .collect()
    }

    #[test]
    fn lookup_by_extension() {
        assert!(by_extension(".TXT").is_some());
        assert!(by_extension("json").is_some());
        assert!(by_extension("pdf").is_none());
    }

    #[test]
    fn export_terms_are_clean() {
        // Terminology rule: user-facing export text never says "record".
        let s = String::from_utf8(Txt.export(&sample()).unwrap())
            .unwrap()
            .to_lowercase();
        assert!(!s.contains("record"));
    }
}

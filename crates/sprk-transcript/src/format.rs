//! Text formatting for the grammar in SPEC 5.1. Parsers rely on this exactly.

/// `HH:MM:SS` from elapsed milliseconds. Hours may exceed two digits.
pub fn timestamp(elapsed_ms: u64) -> String {
    let s = elapsed_ms / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

/// Collapse every run of whitespace (including line breaks) to one space.
pub fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `- \`HH:MM:SS\` **Speaker:** text`
pub fn statement_line(elapsed_ms: u64, speaker: &str, text: &str) -> String {
    format!(
        "- `{}` **{}:** {}",
        timestamp(elapsed_ms),
        single_line(speaker),
        single_line(text)
    )
}

/// `> \`HH:MM:SS\` event text`
pub fn event_line(elapsed_ms: u64, event: &str) -> String {
    format!("> `{}` {}", timestamp(elapsed_ms), single_line(event))
}

/// Quote a YAML scalar.
pub fn yaml_str(s: &str) -> String {
    format!(
        "\"{}\"",
        single_line(s).replace('\\', "\\\\").replace('"', "\\\"")
    )
}

/// Session status in the front matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    InProgress,
    Complete,
    Interrupted,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::InProgress => "in-progress",
            Status::Complete => "complete",
            Status::Interrupted => "interrupted",
        }
    }
}

/// Where the title came from (SPEC 6.2). Higher rank wins; `User` always wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NameSource {
    Default,
    AudioSource,
    Call,
    Calendar,
    User,
}

impl NameSource {
    pub fn as_str(self) -> &'static str {
        match self {
            NameSource::Default => "default",
            NameSource::AudioSource => "audio-source",
            NameSource::Call => "call",
            NameSource::Calendar => "calendar",
            NameSource::User => "user",
        }
    }
}

/// Front matter fields. Strings are supplied by the caller (no clock in this crate).
#[derive(Debug, Clone)]
pub struct Meta {
    pub title: String,
    pub date: String,
    pub start: String,
    pub end: Option<String>,
    pub duration: Option<String>,
    pub status: Status,
    pub sources: Vec<String>,
    pub speakers: Vec<String>,
    pub name_source: NameSource,
    pub engine: String,
    pub app: String,
}

impl Meta {
    pub fn front_matter(&self) -> String {
        self.front_matter_with("")
    }

    /// Front matter with extra YAML blocks (call, audio_sources) placed before `engine`.
    pub fn front_matter_with(&self, blocks: &str) -> String {
        let mut s = String::from("---\n");
        s.push_str(&format!("format: {}\n", crate::FORMAT));
        s.push_str(&format!("title: {}\n", yaml_str(&self.title)));
        s.push_str(&format!("date: {}\n", self.date));
        s.push_str(&format!("start: {}\n", yaml_str(&self.start)));
        if let Some(e) = &self.end {
            s.push_str(&format!("end: {}\n", yaml_str(e)));
        }
        if let Some(d) = &self.duration {
            s.push_str(&format!("duration: {}\n", yaml_str(d)));
        }
        s.push_str(&format!("status: {}\n", self.status.as_str()));
        s.push_str(&format!("sources: [{}]\n", self.sources.join(", ")));
        let sp: Vec<String> = self.speakers.iter().map(|x| yaml_str(x)).collect();
        s.push_str(&format!("speakers: [{}]\n", sp.join(", ")));
        s.push_str(&format!("name_source: {}\n", self.name_source.as_str()));
        s.push_str(blocks);
        s.push_str(&format!("engine: {}\n", yaml_str(&self.engine)));
        s.push_str(&format!("app: {}\n", yaml_str(&self.app)));
        s.push_str("---\n");
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps() {
        assert_eq!(timestamp(0), "00:00:00");
        assert_eq!(timestamp(61_999), "00:01:01");
        assert_eq!(timestamp(3_725_000), "01:02:05");
        assert_eq!(timestamp(100 * 3_600_000), "100:00:00");
    }

    #[test]
    fn statement_is_one_line() {
        let l = statement_line(4_000, "Me", "hello\n  there\r\nfriend");
        assert_eq!(l, "- `00:00:04` **Me:** hello there friend");
        assert!(!l.contains('\n'));
    }

    #[test]
    fn event_line_grammar() {
        assert_eq!(
            event_line(842_000, "Transcription paused"),
            "> `00:14:02` Transcription paused"
        );
    }

    #[test]
    fn yaml_escapes() {
        assert_eq!(yaml_str("a \"b\" \\ c"), "\"a \\\"b\\\" \\\\ c\"");
    }

    #[test]
    fn name_source_ranking() {
        assert!(NameSource::User > NameSource::Calendar);
        assert!(NameSource::Calendar > NameSource::Call);
        assert!(NameSource::Call > NameSource::AudioSource);
        assert!(NameSource::AudioSource > NameSource::Default);
    }
}

//! AI handoff (SPEC 9). Builds a prompt and a launch plan; never sends anything itself.
//! Launching (shell open, clipboard, Explorer reveal) is done by the app through
//! [`Launcher`], so everything here is testable without a desktop.
//!
//! No network: a plan may name an `https://` page to open in the user's own app or browser,
//! but SprkNotes itself makes no request.
//!
//! [`trash`] holds the other desktop-shell action the Library needs: moving a deleted
//! transcript to the Recycle Bin or trash.

pub mod detect;
#[cfg(windows)]
pub mod shell;
pub mod trash;
#[cfg(not(windows))]
pub mod unix;

/// Prompt text cap for a link (SPEC 9.1, about 14,000 characters).
pub const MAX_PROMPT_CHARS: usize = 14_000;

pub const DEFAULT_TEMPLATE: &str = include_str!("../../../prompts/summary.md");

pub trait HandoffTarget {
    fn name(&self) -> &'static str;
    /// Config key used by admin policy to hide the button (SPEC 10.2).
    fn policy_key(&self) -> &'static str;
    fn plan(&self, req: &Request, prompt: &str) -> Plan;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Absolute path of the transcript `.md`.
    pub file: String,
    /// Folder containing it.
    pub folder: String,
    pub title: String,
    pub date: String,
    /// Transcript text. Claude gets it on the clipboard, because a link cannot attach a file
    /// reliably and the app cannot read the user's folder.
    pub body: String,
}

impl Request {
    /// File name without directory or `.md`.
    pub fn basename(&self) -> String {
        let name = self.file.rsplit(['\\', '/']).next().unwrap_or(&self.file);
        name.strip_suffix(".md").unwrap_or(name).to_string()
    }
}

/// What the app should do, in order. Pure data, easy to assert on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Open this URL with the shell (a registered `claude://` handler or a page).
    OpenUrl(String),
    CopyToClipboard(String),
    /// `explorer /select,<file>`
    RevealInExplorer(String),
    /// A short, user-facing toast.
    Toast(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<Step>,
    /// Used when the handler is not registered or opening fails.
    pub fallback: Vec<Step>,
}

pub trait Launcher {
    /// Returns false when no handler is registered or the launch failed.
    fn open_url(&mut self, url: &str) -> bool;
    fn copy(&mut self, text: &str);
    fn reveal(&mut self, path: &str);
    fn toast(&mut self, text: &str);
}

/// Run a plan; if an `OpenUrl` step fails, run the fallback instead.
pub fn run(plan: &Plan, l: &mut dyn Launcher) {
    let mut ok = true;
    for s in &plan.steps {
        ok &= apply(s, l);
        if !ok {
            break;
        }
    }
    if !ok {
        for s in &plan.fallback {
            apply(s, l);
        }
    }
}

fn apply(s: &Step, l: &mut dyn Launcher) -> bool {
    match s {
        Step::OpenUrl(u) => return l.open_url(u),
        Step::CopyToClipboard(t) => l.copy(t),
        Step::RevealInExplorer(p) => l.reveal(p),
        Step::Toast(t) => l.toast(t),
    }
    true
}

/// Fill `{basename}`, `{folder}`, `{title}`, `{date}` (SPEC 9.3). Unknown braces are kept.
pub fn fill_template(template: &str, r: &Request) -> String {
    template
        .replace("{basename}", &r.basename())
        .replace("{folder}", &r.folder)
        .replace("{title}", &r.title)
        .replace("{date}", &r.date)
}

/// Template precedence: user, then admin, then the bundled default (SPEC 9.3).
pub fn pick_template(user: Option<String>, admin: Option<String>) -> String {
    [user, admin]
        .into_iter()
        .flatten()
        .find(|t| !t.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_TEMPLATE.to_string())
}

/// Percent-encode everything outside the RFC 3986 unreserved set.
pub fn url_encode(s: &str) -> String {
    let mut o = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

/// Cut to the cap on a character boundary, noting the cut so it is never silent.
pub fn cap_prompt(p: &str) -> String {
    if p.chars().count() <= MAX_PROMPT_CHARS {
        return p.to_string();
    }
    let keep: String = p.chars().take(MAX_PROMPT_CHARS - 40).collect();
    format!("{keep}\n[Prompt shortened to fit.]")
}

pub struct Claude;
impl HandoffTarget for Claude {
    fn name(&self) -> &'static str {
        "Claude"
    }
    fn policy_key(&self) -> &'static str {
        "HandoffClaude"
    }
    fn plan(&self, r: &Request, prompt: &str) -> Plan {
        // Everything Claude needs goes on the clipboard: the instructions and the transcript
        // itself. A file link is not reliable (Claude often cannot see the file), and a link
        // is cut near 2,000 characters on Windows, so the link carries only a one-line start.
        let clip = if r.body.trim().is_empty() {
            prompt.to_string()
        } else {
            format!("{prompt}\n\n--- Transcript ---\n\n{}", r.body)
        };
        let url = format!("claude://claude.ai/new?q={}", url_encode(START_LINE));
        Plan {
            steps: vec![
                Step::CopyToClipboard(clip.clone()),
                Step::OpenUrl(url),
                Step::Toast(
                    "Claude opened. Click the chat box, press Ctrl+V to add your transcript, then send."
                        .into(),
                ),
            ],
            fallback: vec![Step::Toast(
                "Claude did not open. Your transcript and instructions are copied. Open Claude and paste."
                    .into(),
            )],
        }
    }
}

/// The one line the Claude link fills in. The pasted text follows it.
pub const START_LINE: &str = "Please summarize this meeting transcript. It is pasted below.\n\n";

/// Microsoft 365 Copilot: no file-attach link exists, so copy, open, reveal, toast.
pub struct Copilot {
    /// Configured target (policy `CopilotTarget`); defaults to the Microsoft 365 Copilot app.
    pub target: String,
}
impl Default for Copilot {
    fn default() -> Self {
        Copilot {
            target: detect::COPILOT_APP_LINK.into(),
        }
    }
}
impl HandoffTarget for Copilot {
    fn name(&self) -> &'static str {
        "Microsoft 365 Copilot"
    }
    fn policy_key(&self) -> &'static str {
        "HandoffCopilot"
    }
    fn plan(&self, r: &Request, prompt: &str) -> Plan {
        let toast = Step::Toast("Prompt copied. Attach the highlighted file and paste.".into());
        Plan {
            steps: vec![
                Step::CopyToClipboard(prompt.to_string()),
                Step::OpenUrl(self.target.clone()),
                Step::RevealInExplorer(r.file.clone()),
                toast.clone(),
            ],
            fallback: vec![Step::RevealInExplorer(r.file.clone()), toast],
        }
    }
}

/// Whether a button is shown: setting on and not hidden by policy.
pub fn visible(setting_on: bool, policy_hidden: bool) -> bool {
    setting_on && !policy_hidden
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> Request {
        Request {
            file: r"C:\Users\x\Documents\SprkNotes\Transcripts\2026-11-19 0900 Sync.md".into(),
            folder: r"C:\Users\x\Documents\SprkNotes\Transcripts".into(),
            title: "Sync".into(),
            date: "2026-11-19".into(),
            body: "- `09:00:01` **Me:** hello".into(),
        }
    }

    #[derive(Default)]
    struct Fake {
        handler: bool,
        log: Vec<String>,
    }
    impl Launcher for Fake {
        fn open_url(&mut self, u: &str) -> bool {
            self.log
                .push(format!("open {}", u.split('?').next().unwrap_or(u)));
            self.handler
        }
        fn copy(&mut self, _: &str) {
            self.log.push("copy".into());
        }
        fn reveal(&mut self, _: &str) {
            self.log.push("reveal".into());
        }
        fn toast(&mut self, _: &str) {
            self.log.push("toast".into());
        }
    }

    #[test]
    fn basename_and_template() {
        let r = req();
        assert_eq!(r.basename(), "2026-11-19 0900 Sync");
        let p = fill_template(DEFAULT_TEMPLATE, &r);
        assert!(p.contains("2026-11-19 0900 Sync - Summary"));
        assert!(!p.contains("{basename}") && !p.contains("{folder}"));
        assert!(!p.contains("save it in"));
    }

    #[test]
    fn template_precedence() {
        assert_eq!(pick_template(Some("u".into()), Some("a".into())), "u");
        assert_eq!(pick_template(Some("  ".into()), Some("a".into())), "a");
        assert_eq!(pick_template(None, None), DEFAULT_TEMPLATE);
    }

    #[test]
    fn url_encoding() {
        assert_eq!(url_encode("a b&c/é"), "a%20b%26c%2F%C3%A9");
        assert_eq!(url_encode("Az09-._~"), "Az09-._~");
    }

    #[test]
    fn claude_gets_the_transcript_on_the_clipboard() {
        let plan = Claude.plan(&req(), "instructions");
        let Step::CopyToClipboard(c) = &plan.steps[0] else {
            panic!("expected a copy")
        };
        assert!(c.starts_with("instructions"));
        assert!(c.contains("**Me:** hello"));
        let Step::OpenUrl(u) = &plan.steps[1] else {
            panic!("expected a link")
        };
        assert!(u.starts_with("claude://claude.ai/new?q="));
        assert!(!u.contains("file=") && !u.contains("folder="));
        assert!(u.len() < 400);
    }

    #[test]
    fn long_prompts_are_capped_with_a_note() {
        let long = "x".repeat(20_000);
        let c = cap_prompt(&long);
        assert!(c.chars().count() <= MAX_PROMPT_CHARS);
        assert!(c.ends_with("[Prompt shortened to fit.]"));
        assert_eq!(cap_prompt("short"), "short");
    }

    #[test]
    fn claude_falls_back_when_no_handler() {
        let plan = Claude.plan(&req(), "p");
        let mut f = Fake::default();
        run(&plan, &mut f);
        assert_eq!(f.log, vec!["copy", "open claude://claude.ai/new", "toast"]);
        let mut ok = Fake {
            handler: true,
            ..Fake::default()
        };
        run(&plan, &mut ok);
        assert_eq!(ok.log, vec!["copy", "open claude://claude.ai/new", "toast"]);
    }

    #[test]
    fn copilot_copies_opens_reveals_and_toasts() {
        let plan = Copilot::default().plan(&req(), "p");
        let mut f = Fake {
            handler: true,
            ..Fake::default()
        };
        run(&plan, &mut f);
        assert_eq!(f.log, vec!["copy", "open ms-officeapp:", "reveal", "toast"]);
    }

    #[test]
    fn visibility_needs_setting_and_no_policy_hide() {
        assert!(visible(true, false));
        assert!(!visible(true, true));
        assert!(!visible(false, false));
        assert_eq!(Claude.policy_key(), "HandoffClaude");
    }
}

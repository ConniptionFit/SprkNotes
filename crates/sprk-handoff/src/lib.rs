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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    pub steps: Vec<Step>,
    /// Used when the handler is not registered or opening fails.
    pub fallback: Vec<Step>,
    /// Used instead of the rest of `steps` when the clipboard would not take the text, so the
    /// toast never says "copied" when nothing was.
    pub copy_failed: Vec<Step>,
}

pub trait Launcher {
    /// Returns false when no handler is registered or the launch failed.
    fn open_url(&mut self, url: &str) -> bool;
    /// Returns false when the clipboard would not take the text.
    fn copy(&mut self, text: &str) -> bool;
    fn reveal(&mut self, path: &str);
    fn toast(&mut self, text: &str);
}

/// Run a plan. If a copy fails, run `copy_failed` instead of the rest; if an `OpenUrl` step
/// fails, run `fallback` instead of the rest.
pub fn run(plan: &Plan, l: &mut dyn Launcher) {
    for s in &plan.steps {
        let rest = match s {
            Step::OpenUrl(u) if !l.open_url(u) => &plan.fallback,
            Step::CopyToClipboard(t) if !l.copy(t) => &plan.copy_failed,
            Step::OpenUrl(_) | Step::CopyToClipboard(_) => continue,
            Step::RevealInExplorer(p) => {
                l.reveal(p);
                continue;
            }
            Step::Toast(t) => {
                l.toast(t);
                continue;
            }
        };
        for s in rest {
            match s {
                Step::OpenUrl(u) => {
                    l.open_url(u);
                }
                Step::CopyToClipboard(t) => {
                    l.copy(t);
                }
                Step::RevealInExplorer(p) => l.reveal(p),
                Step::Toast(t) => l.toast(t),
            }
        }
        return;
    }
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
        // itself. A link cannot carry a file, and Claude warns about any prompt a link fills in
        // ("Use caution before running this prompt"), so the link only opens a new, empty chat.
        let clip = if r.body.trim().is_empty() {
            prompt.to_string()
        } else {
            format!("{prompt}\n\n--- Transcript ---\n\n{}", r.body)
        };
        Plan {
            steps: vec![
                Step::CopyToClipboard(clip),
                Step::OpenUrl(CLAUDE_APP_LINK.into()),
                Step::Toast(PASTE_INTO_CLAUDE.into()),
            ],
            // Claude Desktop did not open: the same empty chat in the browser.
            fallback: vec![
                Step::OpenUrl(CLAUDE_WEB_LINK.into()),
                Step::Toast(
                    "Transcript copied. Claude opened in your browser. Click the chat box, press Ctrl+V, then send."
                        .into(),
                ),
            ],
            copy_failed: vec![
                Step::OpenUrl(CLAUDE_APP_LINK.into()),
                Step::RevealInExplorer(r.file.clone()),
                Step::Toast(
                    "The transcript could not be copied. Its file is highlighted: drag it into Claude's chat box, then send."
                        .into(),
                ),
            ],
        }
    }
}

/// A new, empty chat in Claude Desktop. Nothing is filled in (see [`Claude`]).
pub const CLAUDE_APP_LINK: &str = "claude://claude.ai/new";
/// The same in the browser, when Claude Desktop does not open.
pub const CLAUDE_WEB_LINK: &str = "https://claude.ai/new";
pub const PASTE_INTO_CLAUDE: &str =
    "Transcript copied. In Claude, click the chat box, press Ctrl+V, then send.";

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
        // Copilot's chat box is too small for a whole transcript, so the prompt is pasted and the
        // file attached. Its link fills nothing in, so there is no prompt warning either.
        let toast = Step::Toast("Prompt copied. Attach the highlighted file and paste.".into());
        Plan {
            steps: vec![
                Step::CopyToClipboard(prompt.to_string()),
                Step::OpenUrl(self.target.clone()),
                Step::RevealInExplorer(r.file.clone()),
                toast.clone(),
            ],
            fallback: vec![Step::RevealInExplorer(r.file.clone()), toast],
            copy_failed: vec![
                Step::OpenUrl(self.target.clone()),
                Step::RevealInExplorer(r.file.clone()),
                Step::Toast(
                    "The prompt could not be copied. Attach the highlighted file and ask for a summary."
                        .into(),
                ),
            ],
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
        no_clipboard: bool,
        log: Vec<String>,
    }
    impl Launcher for Fake {
        fn open_url(&mut self, u: &str) -> bool {
            self.log
                .push(format!("open {}", u.split('?').next().unwrap_or(u)));
            self.handler
        }
        fn copy(&mut self, _: &str) -> bool {
            self.log.push("copy".into());
            !self.no_clipboard
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
        // The link fills nothing in, so Claude shows no prompt warning.
        assert_eq!(
            plan.steps[1],
            Step::OpenUrl("claude://claude.ai/new".into())
        );
        for s in plan
            .steps
            .iter()
            .chain(&plan.fallback)
            .chain(&plan.copy_failed)
        {
            if let Step::OpenUrl(u) = s {
                assert!(!u.contains('?'), "{u} carries text");
            }
        }
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
    fn claude_falls_back_to_the_browser_then_to_the_file() {
        let plan = Claude.plan(&req(), "p");
        let mut ok = Fake {
            handler: true,
            ..Fake::default()
        };
        run(&plan, &mut ok);
        assert_eq!(ok.log, vec!["copy", "open claude://claude.ai/new", "toast"]);
        // No Claude Desktop handler: the browser gets the empty chat (the fake fails it too).
        let mut f = Fake::default();
        run(&plan, &mut f);
        assert_eq!(
            f.log,
            vec![
                "copy",
                "open claude://claude.ai/new",
                "open https://claude.ai/new",
                "toast"
            ]
        );
        // Clipboard busy: never claim "copied"; highlight the file to drag in instead.
        let mut busy = Fake {
            handler: true,
            no_clipboard: true,
            ..Fake::default()
        };
        run(&plan, &mut busy);
        assert_eq!(
            busy.log,
            vec!["copy", "open claude://claude.ai/new", "reveal", "toast"]
        );
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

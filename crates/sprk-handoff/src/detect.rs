//! Which desktop clients can take a handoff, and what to do for each choice (SPEC 9).
//!
//! A client counts as installed when its link scheme has a registered handler. SprkNotes only
//! asks the OS; it never starts, scans or talks to the apps. Copilot on Linux has no desktop
//! client and Claude may not either, so the "none found" case has its own plan.
use crate::{HandoffTarget, Plan, Request, Step};

/// Link scheme Claude Desktop registers.
pub const CLAUDE_SCHEME: &str = "claude";
/// Microsoft 365 Copilot app (the work version, never consumer Copilot) launches from this link.
pub const COPILOT_SCHEME: &str = "ms-officeapp";
pub const COPILOT_APP_LINK: &str = "ms-officeapp:";

pub trait Probe {
    /// Is an app registered to open links of this scheme?
    fn scheme_registered(&self, scheme: &str) -> bool;
}

/// The real OS answer.
pub struct SystemProbe;

#[cfg(windows)]
impl Probe for SystemProbe {
    fn scheme_registered(&self, scheme: &str) -> bool {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::ERROR_SUCCESS;
        use windows::Win32::System::Registry::{
            RegCloseKey, RegOpenKeyExW, HKEY, HKEY_CLASSES_ROOT, KEY_READ,
        };
        let sub: Vec<u16> = format!("{scheme}\\shell\\open\\command")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut key = HKEY::default();
        // SAFETY: `sub` is NUL-terminated and outlives the call; the handle is closed below.
        let ok = unsafe {
            RegOpenKeyExW(
                HKEY_CLASSES_ROOT,
                PCWSTR(sub.as_ptr()),
                0,
                KEY_READ,
                &mut key,
            )
        } == ERROR_SUCCESS;
        if ok {
            // SAFETY: `key` was opened above.
            let _ = unsafe { RegCloseKey(key) };
        }
        ok
    }
}

#[cfg(not(windows))]
impl Probe for SystemProbe {
    fn scheme_registered(&self, scheme: &str) -> bool {
        // Schemes are our own constants; refuse anything else so nothing odd reaches the command.
        if !scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return false;
        }
        std::process::Command::new("xdg-mime")
            .args(["query", "default", &format!("x-scheme-handler/{scheme}")])
            .stderr(std::process::Stdio::null())
            .output()
            .map(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty())
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetInfo {
    pub id: &'static str,
    pub name: &'static str,
}

pub const CLAUDE: TargetInfo = TargetInfo {
    id: "claude",
    name: "Claude",
};
pub const COPILOT: TargetInfo = TargetInfo {
    id: "copilot",
    name: "Microsoft 365 Copilot",
};

/// Installed and not hidden by policy (`ShowClaudeHandoff`, `ShowCopilotHandoff` set to 0).
pub fn available(
    probe: &dyn Probe,
    copilot_scheme: &str,
    hide_claude: bool,
    hide_copilot: bool,
) -> Vec<TargetInfo> {
    let mut v = Vec::new();
    if !hide_claude && probe.scheme_registered(CLAUDE_SCHEME) {
        v.push(CLAUDE);
    }
    if !hide_copilot && probe.scheme_registered(copilot_scheme) {
        v.push(COPILOT);
    }
    v
}

/// What Summarize should do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Exactly one client: use it without asking.
    Run(TargetInfo),
    /// Both: the user chooses.
    Ask(Vec<TargetInfo>),
    /// Neither: copy the prompt and show the file.
    Fallback,
}

pub fn decide(found: &[TargetInfo]) -> Decision {
    match found {
        [] => Decision::Fallback,
        [one] => Decision::Run(one.clone()),
        many => Decision::Ask(many.to_vec()),
    }
}

/// No desktop client: the prompt goes on the clipboard and the file is highlighted, so the user
/// can attach both to any assistant they are signed in to.
pub fn fallback_plan(r: &Request, prompt: &str) -> Plan {
    let steps = vec![
        Step::CopyToClipboard(crate::cap_prompt(prompt)),
        Step::RevealInExplorer(r.file.clone()),
        Step::Toast(
            "No Claude or Copilot app found. Prompt copied and the file is highlighted. Open your assistant, attach the file and paste."
                .into(),
        ),
    ];
    Plan {
        steps,
        fallback: vec![],
    }
}

/// The plan for a chosen target id, or `None` for an unknown id.
pub fn plan_for(id: &str, copilot_target: Option<&str>, r: &Request, prompt: &str) -> Option<Plan> {
    match id {
        "claude" => Some(crate::Claude.plan(r, prompt)),
        "copilot" => {
            let c = copilot_target.map_or_else(crate::Copilot::default, |t| crate::Copilot {
                target: t.to_string(),
            });
            Some(c.plan(r, prompt))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(Vec<&'static str>);
    impl Probe for Fake {
        fn scheme_registered(&self, s: &str) -> bool {
            self.0.contains(&s)
        }
    }

    fn req() -> Request {
        Request {
            file: "/d/T/a.md".into(),
            folder: "/d/T".into(),
            title: "A".into(),
            date: "2026-11-19".into(),
        }
    }

    #[test]
    fn one_client_runs_without_asking() {
        let f = available(&Fake(vec!["claude"]), COPILOT_SCHEME, false, false);
        assert_eq!(decide(&f), Decision::Run(CLAUDE));
        let f = available(&Fake(vec![COPILOT_SCHEME]), COPILOT_SCHEME, false, false);
        assert_eq!(decide(&f), Decision::Run(COPILOT));
    }

    #[test]
    fn both_clients_ask_the_user() {
        let f = available(
            &Fake(vec!["claude", COPILOT_SCHEME]),
            COPILOT_SCHEME,
            false,
            false,
        );
        assert_eq!(decide(&f), Decision::Ask(vec![CLAUDE, COPILOT]));
    }

    #[test]
    fn policy_hides_a_client_even_when_installed() {
        let p = Fake(vec!["claude", COPILOT_SCHEME]);
        let f = available(&p, COPILOT_SCHEME, true, false);
        assert_eq!(decide(&f), Decision::Run(COPILOT));
        let f = available(&p, COPILOT_SCHEME, true, true);
        assert_eq!(decide(&f), Decision::Fallback);
    }

    #[test]
    fn none_found_falls_back_to_copy_and_reveal() {
        assert_eq!(
            decide(&available(&Fake(vec![]), COPILOT_SCHEME, false, false)),
            Decision::Fallback
        );
        let plan = fallback_plan(&req(), "prompt");
        assert!(matches!(plan.steps[0], Step::CopyToClipboard(_)));
        assert!(matches!(plan.steps[1], Step::RevealInExplorer(_)));
        assert!(matches!(plan.steps[2], Step::Toast(_)));
    }

    #[test]
    fn plans_by_id() {
        assert!(plan_for("claude", None, &req(), "p").is_some());
        assert!(plan_for("nope", None, &req(), "p").is_none());
        let p = plan_for("copilot", Some("myapp:"), &req(), "p").unwrap();
        assert!(p.steps.contains(&Step::OpenUrl("myapp:".into())));
    }
}

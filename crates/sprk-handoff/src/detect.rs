//! Which desktop clients can take a handoff, and what to do for each choice (SPEC 9).
//!
//! A client counts as installed when its link scheme has a registered handler, or (Windows)
//! when its Store/MSIX package is installed for the user. Store packages register their links
//! through the package manifest, not under `HKCR\<scheme>\shell\open\command`, so the
//! classic registry check alone misses them. SprkNotes only asks the OS; it never starts, scans
//! or talks to the apps. Copilot on Linux has no desktop client and Claude may not either, so
//! the "none found" case has its own plan.
use crate::{HandoffTarget, Plan, Request, Step};

/// Link scheme Claude Desktop registers.
pub const CLAUDE_SCHEME: &str = "claude";
/// Microsoft 365 Copilot app (the work version, never consumer Copilot) launches from this link.
pub const COPILOT_SCHEME: &str = "ms-officeapp";
pub const COPILOT_APP_LINK: &str = "ms-officeapp:";
/// Other links the Copilot app is known to register. Tried only when no `CopilotTarget` is set.
pub const COPILOT_ALT_SCHEMES: &[&str] = &["ms-copilot"];
/// Store package names (the part of the package full name before the first `_`) of the Claude
/// desktop app, matched case-insensitively.
pub const CLAUDE_PACKAGES: &[&str] = &["Claude", "AnthropicPBC.Claude"];
/// Store package names of the Microsoft 365 Copilot app.
pub const COPILOT_PACKAGES: &[&str] = &["Microsoft.MicrosoftOfficeHub", "Microsoft.Copilot"];

pub trait Probe {
    /// Is an app registered to open links of this scheme?
    fn scheme_registered(&self, scheme: &str) -> bool;
    /// Is a Store (MSIX) package with this name installed for the user? Windows only.
    fn package_installed(&self, _name: &str) -> bool {
        false
    }
}

/// The real OS answer.
pub struct SystemProbe;

#[cfg(windows)]
mod win {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CLASSES_ROOT,
        HKEY_CURRENT_USER, KEY_READ,
    };
    use windows::Win32::UI::Shell::{
        AssocQueryStringW, ASSOCF_IS_PROTOCOL, ASSOCSTR_FRIENDLYAPPNAME,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn open(root: HKEY, sub: &str) -> Option<HKEY> {
        let sub = wide(sub);
        let mut key = HKEY::default();
        // SAFETY: `sub` is NUL-terminated and outlives the call; the caller closes the handle.
        let ok = unsafe { RegOpenKeyExW(root, PCWSTR(sub.as_ptr()), 0, KEY_READ, &mut key) }
            == ERROR_SUCCESS;
        ok.then_some(key)
    }

    fn close(key: HKEY) {
        // SAFETY: `key` came from `open` and is closed once.
        let _ = unsafe { RegCloseKey(key) };
    }

    fn has_value(key: HKEY, name: &str) -> bool {
        let name = wide(name);
        // SAFETY: `name` is NUL-terminated; no data is requested, only existence.
        let r = unsafe { RegQueryValueExW(key, PCWSTR(name.as_ptr()), None, None, None, None) };
        r == ERROR_SUCCESS
    }

    /// Classic registration: `claude\shell\open\command`, or a `URL Protocol` marker.
    pub fn registry_scheme(scheme: &str) -> bool {
        if let Some(k) = open(
            HKEY_CLASSES_ROOT,
            &format!("{scheme}\\shell\\open\\command"),
        ) {
            close(k);
            return true;
        }
        if let Some(k) = open(HKEY_CLASSES_ROOT, scheme) {
            let marker = has_value(k, "URL Protocol");
            close(k);
            return marker;
        }
        false
    }

    /// The shell's own answer, which also knows Store apps that handle the link.
    pub fn shell_scheme(scheme: &str) -> bool {
        let s = wide(scheme);
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        // SAFETY: `s` is NUL-terminated; `buf` and `len` describe the output buffer.
        let r = unsafe {
            AssocQueryStringW(
                ASSOCF_IS_PROTOCOL,
                ASSOCSTR_FRIENDLYAPPNAME,
                PCWSTR(s.as_ptr()),
                PCWSTR::null(),
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
        };
        r.is_ok()
    }

    /// Packages registered for this user. Their full names look like
    /// `Claude_1.0.0.0_x64__pzs8sxrjxfjjc`; the name is the part before the first `_`.
    pub fn package_installed(name: &str) -> bool {
        let Some(root) = open(
            HKEY_CURRENT_USER,
            "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Packages",
        ) else {
            return false;
        };
        let mut found = false;
        let mut i = 0u32;
        loop {
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            // SAFETY: `buf` and `len` describe the output buffer; other pointers are null.
            let r = unsafe {
                RegEnumKeyExW(
                    root,
                    i,
                    PWSTR(buf.as_mut_ptr()),
                    &mut len,
                    None,
                    PWSTR::null(),
                    None,
                    None,
                )
            };
            if r != ERROR_SUCCESS {
                break;
            }
            let full = String::from_utf16_lossy(&buf[..len as usize]);
            if super::package_name_matches(&full, name) {
                found = true;
                break;
            }
            i += 1;
        }
        close(root);
        found
    }
}

/// Does a package full name (`Name_version_arch_resource_publisherid`) belong to package `name`?
pub fn package_name_matches(full: &str, name: &str) -> bool {
    full.split('_')
        .next()
        .is_some_and(|n| n.eq_ignore_ascii_case(name))
}

#[cfg(windows)]
impl Probe for SystemProbe {
    fn scheme_registered(&self, scheme: &str) -> bool {
        win::registry_scheme(scheme) || win::shell_scheme(scheme)
    }
    fn package_installed(&self, name: &str) -> bool {
        win::package_installed(name)
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

/// The Copilot link to open: the configured one when it is a link, else the first known link that
/// has a handler, else the Microsoft 365 Copilot app link.
pub fn default_copilot_link(probe: &dyn Probe) -> String {
    std::iter::once(COPILOT_SCHEME)
        .chain(COPILOT_ALT_SCHEMES.iter().copied())
        .find(|s| probe.scheme_registered(s))
        .map_or_else(|| COPILOT_APP_LINK.to_string(), |s| format!("{s}:"))
}

fn any_package(probe: &dyn Probe, names: &[&str]) -> bool {
    names.iter().any(|n| probe.package_installed(n))
}

/// Installed and not hidden by policy (`ShowClaudeHandoff`, `ShowCopilotHandoff` set to 0).
/// `copilot_scheme` is the link of an admin-set `CopilotTarget`, or [`COPILOT_SCHEME`] by default;
/// only the default also tries the alternate links and the Store package.
pub fn available(
    probe: &dyn Probe,
    copilot_scheme: &str,
    hide_claude: bool,
    hide_copilot: bool,
) -> Vec<TargetInfo> {
    let mut v = Vec::new();
    if !hide_claude
        && (probe.scheme_registered(CLAUDE_SCHEME) || any_package(probe, CLAUDE_PACKAGES))
    {
        v.push(CLAUDE);
    }
    let copilot = probe.scheme_registered(copilot_scheme)
        || (copilot_scheme == COPILOT_SCHEME
            && (COPILOT_ALT_SCHEMES
                .iter()
                .any(|s| probe.scheme_registered(s))
                || any_package(probe, COPILOT_PACKAGES)));
    if !hide_copilot && copilot {
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

    /// A Store-only install: no scheme in the classic registry, only the package.
    struct Pkgs(Vec<&'static str>);
    impl Probe for Pkgs {
        fn scheme_registered(&self, _: &str) -> bool {
            false
        }
        fn package_installed(&self, n: &str) -> bool {
            self.0.iter().any(|p| p.eq_ignore_ascii_case(n))
        }
    }

    fn req() -> Request {
        Request {
            file: "/d/T/a.md".into(),
            folder: "/d/T".into(),
            title: "A".into(),
            date: "2026-11-19".into(),
            body: String::new(),
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

    #[test]
    fn store_packages_count_as_installed() {
        let p = Pkgs(vec!["Claude", "Microsoft.MicrosoftOfficeHub"]);
        let f = available(&p, COPILOT_SCHEME, false, false);
        assert_eq!(decide(&f), Decision::Ask(vec![CLAUDE, COPILOT]));
        let only = Pkgs(vec!["Claude"]);
        assert_eq!(
            decide(&available(&only, COPILOT_SCHEME, false, false)),
            Decision::Run(CLAUDE)
        );
    }

    #[test]
    fn custom_copilot_link_ignores_the_store_package() {
        let p = Pkgs(vec!["Microsoft.MicrosoftOfficeHub"]);
        assert_eq!(
            decide(&available(&p, "myapp", false, false)),
            Decision::Fallback
        );
    }

    #[test]
    fn alternate_copilot_link_is_found_and_used() {
        let p = Fake(vec!["ms-copilot"]);
        assert_eq!(
            decide(&available(&p, COPILOT_SCHEME, false, false)),
            Decision::Run(COPILOT)
        );
        assert_eq!(default_copilot_link(&p), "ms-copilot:");
        assert_eq!(default_copilot_link(&Fake(vec![])), COPILOT_APP_LINK);
        assert_eq!(
            default_copilot_link(&Fake(vec![COPILOT_SCHEME])),
            "ms-officeapp:"
        );
    }

    #[test]
    fn package_full_names_match_on_the_name_only() {
        assert!(package_name_matches(
            "Claude_1.2.3.0_x64__pzs8sxrjxfjjc",
            "claude"
        ));
        assert!(!package_name_matches(
            "ClaudeHelper_1.0.0.0_x64__abc",
            "Claude"
        ));
        assert!(package_name_matches(
            "Microsoft.Copilot_1.0_arm64__8wekyb3d8bbwe",
            "Microsoft.Copilot"
        ));
    }
}

//! A keyboard shortcut that works while SprkNotes is in the background (SPEC 8.6). Off by
//! default. One key combination: it starts a transcript when none runs, and pauses or resumes
//! the one that does. Windows registers it with the system (`win_hotkey`); this module holds the
//! parts that are the same everywhere: reading and writing a combination, and what a press does.

/// A key combination such as Ctrl+Alt+S. At least one of Ctrl, Alt or Win is needed, so the
/// shortcut never takes a key that is typed on its own or with Shift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    /// The key's name as shown: "S", "7", "F9", "Space".
    pub key: String,
}

/// Why a combination was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyError {
    /// Not a combination SprkNotes accepts (no Ctrl, Alt or Win, or an unknown key).
    Invalid,
    /// Another app already holds it.
    Taken,
    /// The system would not register it for another reason.
    Failed,
}

impl std::fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            HotkeyError::Invalid => "invalid",
            HotkeyError::Taken => "taken",
            HotkeyError::Failed => "failed",
        })
    }
}

/// Windows modifier flags for RegisterHotKey.
pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;
/// Holding the keys down does not repeat the press.
pub const MOD_NOREPEAT: u32 = 0x4000;

impl Combo {
    /// Read "Ctrl+Alt+S" (any order of modifiers, any case, spaces allowed).
    pub fn parse(s: &str) -> Result<Combo, HotkeyError> {
        let mut c = Combo {
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
            key: String::new(),
        };
        for part in s.split('+').map(str::trim) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => c.ctrl = true,
                "alt" => c.alt = true,
                "shift" => c.shift = true,
                "win" | "meta" | "super" => c.win = true,
                _ if c.key.is_empty() && vk_of(part).is_some() => c.key = key_name(part),
                _ => return Err(HotkeyError::Invalid),
            }
        }
        if c.key.is_empty() || !(c.ctrl || c.alt || c.win) {
            return Err(HotkeyError::Invalid);
        }
        Ok(c)
    }

    /// The combination as shown in Settings: "Ctrl+Alt+S".
    pub fn label(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.win {
            parts.push("Win");
        }
        parts.push(&self.key);
        parts.join("+")
    }

    /// Modifier flags for RegisterHotKey, with MOD_NOREPEAT.
    pub fn mods(&self) -> u32 {
        let mut m = MOD_NOREPEAT;
        if self.alt {
            m |= MOD_ALT;
        }
        if self.ctrl {
            m |= MOD_CONTROL;
        }
        if self.shift {
            m |= MOD_SHIFT;
        }
        if self.win {
            m |= MOD_WIN;
        }
        m
    }

    /// The Windows virtual-key code of the key.
    pub fn vk(&self) -> u32 {
        vk_of(&self.key).unwrap_or(0)
    }
}

fn key_name(k: &str) -> String {
    if k.eq_ignore_ascii_case("space") {
        "Space".into()
    } else {
        k.to_ascii_uppercase()
    }
}

/// Virtual-key codes for the keys a shortcut may use: letters, digits, F1 to F24 and Space.
fn vk_of(k: &str) -> Option<u32> {
    let up = k.to_ascii_uppercase();
    let b = up.as_bytes();
    if b.len() == 1 && (b[0].is_ascii_uppercase() || b[0].is_ascii_digit()) {
        // VK_A..VK_Z and VK_0..VK_9 are the ASCII codes.
        return Some(u32::from(b[0]));
    }
    if up == "SPACE" {
        return Some(0x20);
    }
    let n: u32 = up.strip_prefix('F')?.parse().ok()?;
    (1..=24).contains(&n).then(|| 0x70 + n - 1)
}

/// What a press does, given the session state the UI shows ("ready", "transcribing", "paused").
pub fn action_for(status: &str) -> Option<&'static str> {
    match status {
        "ready" => Some("start"),
        "transcribing" => Some("pause"),
        "paused" => Some("resume"),
        _ => None,
    }
}

/// Presses closer together than this count once (a start takes a moment to show).
pub const PRESS_GAP_MS: u64 = 750;

/// Registers one system-wide shortcut. `set` replaces any earlier one; `None` removes it.
pub trait GlobalHotkey: Send {
    fn set(
        &mut self,
        combo: Option<&Combo>,
        on_press: Box<dyn Fn() + Send + 'static>,
    ) -> Result<(), HotkeyError>;
}

/// Test and non-Windows stand-in. `taken` plays another app that holds a combination.
#[derive(Default)]
pub struct FakeHotkey {
    pub current: Option<Combo>,
    pub taken: Vec<Combo>,
    on_press: Option<Box<dyn Fn() + Send>>,
}

impl FakeHotkey {
    /// Press the registered combination.
    pub fn press(&self) {
        if let Some(f) = &self.on_press {
            f();
        }
    }
}

impl GlobalHotkey for FakeHotkey {
    fn set(
        &mut self,
        combo: Option<&Combo>,
        on_press: Box<dyn Fn() + Send + 'static>,
    ) -> Result<(), HotkeyError> {
        self.current = None;
        self.on_press = None;
        let Some(c) = combo else {
            return Ok(());
        };
        if self.taken.contains(c) {
            return Err(HotkeyError::Taken);
        }
        self.current = Some(c.clone());
        self.on_press = Some(on_press);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn reads_and_writes_combinations() {
        let c = Combo::parse("ctrl + alt + s").unwrap();
        assert_eq!(c.label(), "Ctrl+Alt+S");
        assert_eq!(c.vk(), 0x53);
        assert_eq!(c.mods(), MOD_NOREPEAT | MOD_CONTROL | MOD_ALT);
        assert_eq!(
            Combo::parse("Win+Shift+F9").unwrap().label(),
            "Shift+Win+F9"
        );
        assert_eq!(Combo::parse("Win+Shift+F9").unwrap().vk(), 0x78);
        assert_eq!(Combo::parse("Alt+space").unwrap().label(), "Alt+Space");
        assert_eq!(Combo::parse("Ctrl+7").unwrap().vk(), 0x37);
    }

    #[test]
    fn refuses_keys_that_would_get_in_the_way_of_typing() {
        for bad in [
            "S", "Shift+S", "Ctrl", "Ctrl+Alt", "Ctrl+S+T", "Ctrl+F25", "Ctrl+Tab", "",
        ] {
            assert_eq!(Combo::parse(bad), Err(HotkeyError::Invalid), "{bad}");
        }
    }

    #[test]
    fn a_press_starts_pauses_and_resumes() {
        assert_eq!(action_for("ready"), Some("start"));
        assert_eq!(action_for("transcribing"), Some("pause"));
        assert_eq!(action_for("paused"), Some("resume"));
        assert_eq!(action_for("ending"), None);
    }

    #[test]
    fn fake_registers_replaces_and_reports_a_taken_key() {
        let presses = Arc::new(AtomicUsize::new(0));
        let mut h = FakeHotkey {
            taken: vec![Combo::parse("Ctrl+Alt+T").unwrap()],
            ..Default::default()
        };
        let p = presses.clone();
        let c = Combo::parse("Ctrl+Alt+S").unwrap();
        h.set(
            Some(&c),
            Box::new(move || {
                p.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .unwrap();
        h.press();
        assert_eq!(presses.load(Ordering::SeqCst), 1);
        let t = Combo::parse("Ctrl+Alt+T").unwrap();
        assert_eq!(h.set(Some(&t), Box::new(|| {})), Err(HotkeyError::Taken));
        assert!(h.current.is_none(), "a refused key leaves none registered");
        h.set(None, Box::new(|| {})).unwrap();
        h.press();
        assert_eq!(presses.load(Ordering::SeqCst), 1);
    }
}

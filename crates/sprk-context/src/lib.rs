//! Read-only inspection seams. Titles and participant names are PHI-grade:
//! they never appear in logs or error strings.
pub mod activity;
pub mod labeling;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub pid: u32,
    pub title: String,
}

pub trait WindowEnumerator: Send {
    fn windows(&self) -> Vec<Window>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaSession {
    pub app: String,
    pub title: Option<String>,
    pub playing: bool,
}

pub trait MediaSessionSource: Send {
    fn sessions(&self) -> Vec<MediaSession>;
}

/// Result of the silent Teams localhost probe. Failure is not an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallState {
    Unavailable,
    OneToOne,
    Group,
}

pub trait TeamsLocalApi: Send {
    fn call_state(&self) -> CallState;
}

pub struct FakeTeams(pub CallState);
impl TeamsLocalApi for FakeTeams {
    fn call_state(&self) -> CallState {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fake_teams_reports_state() {
        assert_eq!(FakeTeams(CallState::Group).call_state(), CallState::Group);
    }
}

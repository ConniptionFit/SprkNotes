//! Resolver skeleton (SPEC 16.4). No model path or name is hard-coded anywhere.
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Store {
    Portable,
    User,
    Machine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub id: String,
    pub version: String,
    pub store: Store,
    pub dir: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("no usable transcription model")]
    NoneAvailable,
}

/// Picks the active model from candidates the caller discovered. Real precedence
/// rules (active.json, fallback to previous good) land in M10.
pub fn resolve(candidates: &[Installed]) -> Result<&Installed, ModelError> {
    candidates
        .iter()
        .max_by_key(|c| c.store)
        .ok_or(ModelError::NoneAvailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn m(store: Store) -> Installed {
        Installed {
            id: "x".into(),
            version: "1".into(),
            store,
            dir: PathBuf::new(),
        }
    }
    #[test]
    fn empty_is_error() {
        assert!(resolve(&[]).is_err());
    }
    #[test]
    fn machine_store_wins_placeholder() {
        let c = [m(Store::User), m(Store::Machine)];
        assert_eq!(resolve(&c).map(|i| i.store).ok(), Some(Store::Machine));
    }
}

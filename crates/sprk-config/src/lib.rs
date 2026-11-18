//! Policy and secret seams (registry, Credential Manager on Windows).
use std::collections::HashMap;

pub trait PolicyStore: Send {
    fn get(&self, key: &str) -> Option<String>;
}
pub trait SecretStore: Send {
    fn get(&self, key: &str) -> Option<String>;
}

#[derive(Default)]
pub struct FakePolicy(pub HashMap<String, String>);
impl PolicyStore for FakePolicy {
    fn get(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fake_policy_lookup() {
        let mut p = FakePolicy::default();
        p.0.insert("k".into(), "v".into());
        assert_eq!(p.get("k").as_deref(), Some("v"));
        assert_eq!(p.get("missing"), None);
    }
}

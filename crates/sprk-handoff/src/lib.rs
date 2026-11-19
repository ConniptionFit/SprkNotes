//! Handoff targets. Never sends anything over the network itself.
pub trait HandoffTarget {
    fn name(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;
    struct T;
    impl HandoffTarget for T {
        fn name(&self) -> &'static str {
            "t"
        }
    }
    #[test]
    fn trait_is_usable() {
        assert_eq!(T.name(), "t");
    }
}

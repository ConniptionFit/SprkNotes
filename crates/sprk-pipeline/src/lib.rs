//! Session pipeline. Implemented from M3 onward.
/// Speaker label. Remote speakers are `Others` unless a confirmed 1:1 call says otherwise (SPEC 19.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Speaker {
    Me,
    Others,
    Named(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_remote_label_is_others() {
        assert_eq!(Speaker::Others, Speaker::Others);
    }
}

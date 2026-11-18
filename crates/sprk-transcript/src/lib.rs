//! Format `sprknotes-transcript/1`. Writer, recovery and exporters arrive in M3 and M7.
pub const FORMAT: &str = "sprknotes-transcript/1";

#[cfg(test)]
mod tests {
    #[test]
    fn format_id_is_stable() {
        assert_eq!(super::FORMAT, "sprknotes-transcript/1");
    }
}

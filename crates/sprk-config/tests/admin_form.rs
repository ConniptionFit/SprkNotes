//! The admin script form (tools/admin-form) lists every setting the app reads, and nothing else.
use std::collections::BTreeSet;

#[test]
fn admin_form_lists_exactly_the_policy_keys() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/admin-form/settings.js"
    );
    let text = std::fs::read_to_string(path).expect("tools/admin-form/settings.js");
    let form: BTreeSet<&str> = text
        .split("{ key: \"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect();
    let app: BTreeSet<&str> = sprk_config::KEYS
        .iter()
        .chain(sprk_config::UI_KEYS)
        .copied()
        .collect();
    assert_eq!(
        form, app,
        "update tools/admin-form/settings.js with sprk_config::KEYS and UI_KEYS"
    );
}

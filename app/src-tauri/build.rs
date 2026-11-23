//! Reads `distribution.toml` at the repository root and hands the repository address to the app
//! at compile time. `SPRK_REPO_URL` / `SPRK_REPO_PROVIDER` in the build environment win over the file.
use std::{fs, path::Path};

fn value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let l = l.trim();
        if l.starts_with('#') {
            return None;
        }
        let (k, v) = l.split_once('=')?;
        if k.trim() != key {
            return None;
        }
        let v = v.split('#').next()?.trim().trim_matches('"').trim();
        Some(v.to_string())
    })
}

fn main() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../distribution.toml");
    println!("cargo:rerun-if-changed={}", path.display());
    println!("cargo:rerun-if-env-changed=SPRK_REPO_URL");
    println!("cargo:rerun-if-env-changed=SPRK_REPO_PROVIDER");
    let text =
        fs::read_to_string(&path).expect("distribution.toml is missing from the repository root");
    let pick = |env: &str, key: &str| {
        std::env::var(env)
            .ok()
            .filter(|v| !v.is_empty())
            .or_else(|| value(&text, key))
            .unwrap_or_default()
    };
    let repo = pick("SPRK_REPO_URL", "repo_url");
    assert!(
        repo.starts_with("https://") && repo.matches('/').count() >= 4,
        "distribution.toml repo_url must look like https://host/owner/repo (got '{repo}')"
    );
    let repo = repo
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_string();
    let tag = value(&text, "models_release_tag")
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "models".into());
    println!("cargo:rustc-env=SPRK_DEFAULT_REPO_URL={repo}");
    println!(
        "cargo:rustc-env=SPRK_DEFAULT_PROVIDER={}",
        pick("SPRK_REPO_PROVIDER", "provider")
    );
    println!("cargo:rustc-env=SPRK_DEFAULT_MODELS_INDEX_URL={repo}/releases/download/{tag}/models-index.json");
    tauri_build::build()
}

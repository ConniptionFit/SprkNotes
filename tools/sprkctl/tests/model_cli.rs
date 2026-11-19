//! End-to-end `sprkctl model` run: pack, verify, install, list, rollback, bad pack exit codes.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;

fn sha(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

fn make_model(root: &Path, version: &str, weights: &[u8]) -> std::path::PathBuf {
    let dir = root.join(format!("src-{version}"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("weights.bin"), weights).unwrap();
    std::fs::write(dir.join("tokens.txt"), b"a 0\n").unwrap();
    let json = format!(
        r#"{{"schema":1,"id":"demo","version":"{version}","display_name":"Demo","engine":"sherpa-onnx/whisper",
"languages":["en"],"sample_rate":16000,"features":{{"punctuation":true,"casing":true,"streaming":false}},
"accelerators":["cpu"],"min_app_version":"0.0.1",
"files":{{"encoder":"weights.bin","tokens":"tokens.txt"}},
"sha256":{{"weights.bin":"{}","tokens.txt":"{}"}},"license":"MIT","source":"synthetic","notes":""}}"#,
        sha(weights),
        sha(b"a 0\n")
    );
    std::fs::write(dir.join("model.json"), json).unwrap();
    dir
}

fn ctl(args: &[&str]) -> (i32, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_sprkctl"))
        .args(args)
        .output()
        .unwrap();
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into(),
        String::from_utf8_lossy(&o.stderr).into(),
    )
}

#[test]
fn pack_install_list_rollback_and_exit_codes() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("store");
    let root_s = root.to_str().unwrap();
    let mut packs = Vec::new();
    for v in ["2026.11.0", "2027.01.0"] {
        let src = make_model(t.path(), v, v.as_bytes());
        let out = t.path().join(format!("demo@{v}.sprkmodel"));
        let (c, _, e) = ctl(&[
            "model",
            "pack",
            src.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ]);
        assert_eq!(c, 0, "{e}");
        let (c, o, _) = ctl(&["model", "verify", out.to_str().unwrap()]);
        assert_eq!(c, 0);
        assert!(o.starts_with("ok demo@"));
        packs.push(out);
    }
    for p in &packs {
        let (c, o, e) = ctl(&["model", "install", p.to_str().unwrap(), "--root", root_s]);
        assert_eq!(c, 0, "{e}");
        assert!(o.contains("activated=true"));
    }
    let (_, o, _) = ctl(&["model", "list", "--root", root_s]);
    assert!(
        o.contains("* demo@2027.01.0") && o.contains("  demo@2026.11.0"),
        "{o}"
    );
    let (c, o, _) = ctl(&["model", "rollback", "--root", root_s]);
    assert_eq!(c, 0);
    assert!(o.contains("demo@2026.11.0"));
    let (_, o, _) = ctl(&["model", "list", "--root", root_s, "--json"]);
    assert!(o.contains("\"active\": true"));

    // garbage pack: verify failed exit code
    let junk = t.path().join("junk.sprkmodel");
    std::fs::write(&junk, b"not a zip").unwrap();
    let (c, _, _) = ctl(&["model", "verify", junk.to_str().unwrap()]);
    assert_eq!(c, 10);
    let (c, _, _) = ctl(&["model", "install", junk.to_str().unwrap(), "--root", root_s]);
    assert_eq!(c, 10);
    assert!(root
        .join("incoming/rejected/junk.sprkmodel.reason.txt")
        .exists());
}

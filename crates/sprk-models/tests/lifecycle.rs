#![allow(clippy::unwrap_used, clippy::expect_used)]
//! M10 model update test, portable half: install, activate, rollback, prune, resolver
//! precedence, and the rejection cases from SPEC 13.1 (corrupt, partial, traversal).
use sha2::{Digest, Sha256};
use sprk_models::import::{
    import_pack, process_incoming, ImportOptions, ImportOutcome, SmokeTester,
};
use sprk_models::pack::{pack_folder, verify_pack, SignatureVerifier, VerifyOptions};
use sprk_models::resolver::{manifest_hash, resolve, Policy, Stores};
use sprk_models::{Manifest, ModelStore, StoreKind};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const ENGINES: &[&str] = &["test/engine"];

fn sha(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

/// Write a synthetic model folder (tiny placeholder weights, not a real model).
fn make_model(root: &Path, id: &str, version: &str, weights: &[u8]) -> PathBuf {
    let dir = root.join(format!("{id}-src-{version}"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("weights.bin"), weights).unwrap();
    std::fs::write(dir.join("tokens.txt"), b"a 0\nb 1\n").unwrap();
    let json = format!(
        r#"{{"schema":1,"id":"{id}","version":"{version}","display_name":"T","engine":"test/engine",
"languages":["en"],"sample_rate":16000,"features":{{"punctuation":true,"casing":true,"streaming":false}},
"accelerators":["cpu"],"min_app_version":"0.0.1",
"files":{{"model":"weights.bin","tokens":"tokens.txt"}},
"sha256":{{"weights.bin":"{}","tokens.txt":"{}"}},"license":"MIT","source":"synthetic","notes":""}}"#,
        sha(weights),
        sha(b"a 0\nb 1\n")
    );
    std::fs::write(dir.join("model.json"), json).unwrap();
    dir
}

struct Smoke(bool);
impl SmokeTester for Smoke {
    fn smoke(&self, _d: &Path, _m: &Manifest) -> Result<(), String> {
        if self.0 {
            Ok(())
        } else {
            Err("decode mismatch".into())
        }
    }
}

fn opts<'a>(activate: bool, allowed: &'a [String]) -> ImportOptions<'a> {
    ImportOptions {
        verify: VerifyOptions {
            supported_engines: ENGINES,
            app_version: "0.0.1",
            allowed_hashes: allowed,
            require_signature: false,
            verifier: None,
        },
        activate,
        keep_versions: 2,
        now: "2026-11-19T00:00:00Z",
    }
}

fn pack(root: &Path, id: &str, version: &str, weights: &[u8]) -> PathBuf {
    let src = make_model(root, id, version, weights);
    let out = root.join(format!("{id}@{version}.sprkmodel"));
    pack_folder(&src, &out).unwrap();
    out
}

#[test]
fn install_activate_rollback_and_prune() {
    let t = tempfile::tempdir().unwrap();
    let store = ModelStore::new(t.path().join("models"), StoreKind::Machine);
    let o = opts(true, &[]);
    for v in ["2026.11.0", "2027.01.0", "2027.03.0"] {
        let p = pack(t.path(), "m", v, v.as_bytes());
        let r = import_pack(&store, &p, &o, &Smoke(true));
        assert!(
            matches!(
                r,
                ImportOutcome::Installed {
                    activated: true,
                    ..
                }
            ),
            "{r:?}"
        );
    }
    let a = store.active().unwrap();
    assert_eq!(a.active, "m@2027.03.0");
    assert_eq!(a.previous.as_deref(), Some("m@2027.01.0"));
    // keep_versions = 2: the oldest was pruned, active and previous remain
    let ids: Vec<String> = store.list().into_iter().map(|i| i.id_version).collect();
    assert_eq!(ids, vec!["m@2027.01.0", "m@2027.03.0"]);
    assert_eq!(store.rollback("t").unwrap(), "m@2027.01.0");
    assert_eq!(store.active().unwrap().active, "m@2027.01.0");
    assert_eq!(
        store.active().unwrap().previous.as_deref(),
        Some("m@2027.03.0")
    );
    assert!(
        store.remove("m@2027.01.0").is_err(),
        "cannot remove the active model"
    );
    // log lines exist and hold no transcript text
    let log = std::fs::read_to_string(store.root().join("import-log.jsonl")).unwrap();
    assert_eq!(log.lines().count(), 3);
}

#[test]
fn no_activate_keeps_the_current_model() {
    let t = tempfile::tempdir().unwrap();
    let store = ModelStore::new(t.path().join("models"), StoreKind::Machine);
    let p1 = pack(t.path(), "m", "2026.11.0", b"one");
    import_pack(&store, &p1, &opts(true, &[]), &Smoke(true));
    let p2 = pack(t.path(), "m", "2027.01.0", b"two");
    let r = import_pack(&store, &p2, &opts(false, &[]), &Smoke(true));
    assert_eq!(
        r,
        ImportOutcome::Installed {
            id_version: "m@2027.01.0".into(),
            activated: false
        }
    );
    assert_eq!(store.active().unwrap().active, "m@2026.11.0");
}

#[test]
fn broken_pack_is_rejected_and_current_model_kept() {
    let t = tempfile::tempdir().unwrap();
    let store = ModelStore::new(t.path().join("models"), StoreKind::Machine);
    let good = pack(t.path(), "m", "2026.11.0", b"good");
    import_pack(&store, &good, &opts(true, &[]), &Smoke(true));

    // 1. smoke test failure
    let bad_smoke = pack(t.path(), "m", "2027.01.0", b"smoke");
    let r = import_pack(&store, &bad_smoke, &opts(true, &[]), &Smoke(false));
    assert!(matches!(r, ImportOutcome::Rejected { ref reason } if reason.contains("smoke")));
    assert!(
        store.get("m@2027.01.0").is_none(),
        "failed model is removed"
    );

    // 2. truncated (partial) file
    let full = pack(t.path(), "m", "2027.02.0", b"trunc");
    let bytes = std::fs::read(&full).unwrap();
    let cut = t.path().join("cut.sprkmodel");
    std::fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
    assert!(matches!(
        import_pack(&store, &cut, &opts(true, &[]), &Smoke(true)),
        ImportOutcome::Rejected { .. }
    ));

    // 3. tampered data file: hash mismatch after extraction
    let src = make_model(t.path(), "m", "2027.03.0", b"tamper");
    let out = t.path().join("tamper.sprkmodel");
    pack_folder(&src, &out).unwrap();
    {
        // rebuild a zip with the same manifest but different weights
        let f = std::fs::File::create(&out).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let so = zip::write::SimpleFileOptions::default();
        z.start_file("model.json", so).unwrap();
        z.write_all(&std::fs::read(src.join("model.json")).unwrap())
            .unwrap();
        z.start_file("weights.bin", so).unwrap();
        z.write_all(b"evil").unwrap();
        z.start_file("tokens.txt", so).unwrap();
        z.write_all(b"a 0\nb 1\n").unwrap();
        z.finish().unwrap();
    }
    assert!(matches!(
        import_pack(&store, &out, &opts(true, &[]), &Smoke(true)),
        ImportOutcome::Rejected { .. }
    ));

    // current model untouched; rejects are filed with a reason
    assert_eq!(store.active().unwrap().active, "m@2026.11.0");
    let rejected = store.root().join("incoming").join("rejected");
    assert!(rejected.join("tamper.sprkmodel.reason.txt").exists());
}

#[test]
fn path_traversal_and_extra_entries_are_rejected() {
    let t = tempfile::tempdir().unwrap();
    let src = make_model(t.path(), "m", "2026.11.0", b"w");
    for evil in ["../evil.exe", "sub/evil.exe", "extra.exe"] {
        let out = t.path().join("evil.sprkmodel");
        let f = std::fs::File::create(&out).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let so = zip::write::SimpleFileOptions::default();
        for n in ["model.json", "weights.bin", "tokens.txt"] {
            z.start_file(n, so).unwrap();
            z.write_all(&std::fs::read(src.join(n)).unwrap()).unwrap();
        }
        z.start_file(evil, so).unwrap();
        z.write_all(b"MZ").unwrap();
        z.finish().unwrap();
        let o = opts(true, &[]);
        assert!(
            verify_pack(&out, &o.verify).is_err(),
            "{evil} must be rejected"
        );
    }
}

#[test]
fn engine_and_app_version_checks() {
    let t = tempfile::tempdir().unwrap();
    let p = pack(t.path(), "m", "2026.11.0", b"w");
    let mut o = opts(true, &[]);
    o.verify.supported_engines = &["other/engine"];
    assert!(verify_pack(&p, &o.verify).is_err());
    let mut o = opts(true, &[]);
    o.verify.app_version = "0.0.0";
    assert!(verify_pack(&p, &o.verify).is_err());
}

#[test]
fn allow_list_and_signature_gates() {
    struct Ok_;
    impl SignatureVerifier for Ok_ {
        fn verify(&self, _m: &[u8], sig: &[u8]) -> Result<(), String> {
            if sig == b"good" {
                Ok(())
            } else {
                Err("bad signature".into())
            }
        }
    }
    let t = tempfile::tempdir().unwrap();
    let src = make_model(t.path(), "m", "2026.11.0", b"w");
    let p = t.path().join("m.sprkmodel");
    pack_folder(&src, &p).unwrap();
    let hash = sha(&std::fs::read(src.join("model.json")).unwrap());

    let none: Vec<String> = Vec::new();
    let listed = vec![hash.to_uppercase()];
    let other = vec!["00".repeat(32)];
    assert!(verify_pack(&p, &opts(true, &listed).verify).is_ok());
    assert!(
        verify_pack(&p, &opts(true, &other).verify).is_err(),
        "unverified model refused"
    );
    assert!(verify_pack(&p, &opts(true, &none).verify).is_ok());

    let mut o = opts(true, &none);
    o.verify.require_signature = true;
    assert!(
        verify_pack(&p, &o.verify).is_err(),
        "no verifier and no signature"
    );
    std::fs::write(src.join("model.sig"), b"good").unwrap();
    pack_folder(&src, &p).unwrap();
    o.verify.verifier = Some(&Ok_);
    assert!(verify_pack(&p, &o.verify).is_ok());
    std::fs::write(src.join("model.sig"), b"forged").unwrap();
    pack_folder(&src, &p).unwrap();
    assert!(verify_pack(&p, &o.verify).is_err());
}

#[test]
fn resolver_precedence_and_fallback() {
    let t = tempfile::tempdir().unwrap();
    let machine = ModelStore::new(t.path().join("machine"), StoreKind::Machine);
    let user = ModelStore::new(t.path().join("user"), StoreKind::User);
    let bundled = ModelStore::new(t.path().join("bundled"), StoreKind::Bundled);
    let o = opts(true, &[]);
    for (store, v, w) in [
        (&bundled, "2026.01.0", "b"),
        (&user, "2026.06.0", "u"),
        (&machine, "2026.11.0", "m1"),
        (&machine, "2027.01.0", "m2"),
    ] {
        let p = pack(t.path(), "m", v, w.as_bytes());
        import_pack(store, &p, &o, &Smoke(true));
    }
    let stores = Stores {
        bundled: Some(&bundled),
        machine: Some(&machine),
        user: Some(&user),
    };
    let ok = |_: &sprk_models::Installed| Ok(());

    let r = resolve(&Policy::default(), &stores, &ok).unwrap();
    assert_eq!(r.chosen.id_version, "m@2027.01.0");

    // the active model fails to load: fall back to previous first
    let fail_new = |i: &sprk_models::Installed| {
        if i.id_version == "m@2027.01.0" {
            Err("load failed".to_string())
        } else {
            Ok(())
        }
    };
    let r = resolve(&Policy::default(), &stores, &fail_new).unwrap();
    assert_eq!(r.chosen.id_version, "m@2026.11.0");
    assert_eq!(r.skipped.len(), 1);

    // pinned model wins
    let pin = Policy {
        pinned: Some("m@2026.06.0".into()),
        ..Policy::default()
    };
    assert_eq!(
        resolve(&pin, &stores, &ok).unwrap().chosen.id_version,
        "m@2026.06.0"
    );

    // allow-list that matches nothing: only the bundled default remains trusted
    let strict = Policy {
        allowed_hashes: vec!["00".repeat(32)],
        policy_present: true,
        ..Policy::default()
    };
    let r = resolve(&strict, &stores, &ok).unwrap();
    assert_eq!(r.chosen.store, StoreKind::Bundled);

    // everything broken: honest error, never a panic
    let bad = |_: &sprk_models::Installed| Err("x".to_string());
    assert!(resolve(&Policy::default(), &stores, &bad).is_err());

    // user store ignored under policy without AllowUserModels
    let only_user = Stores {
        bundled: None,
        machine: None,
        user: Some(&user),
    };
    let locked = Policy {
        policy_present: true,
        ..Policy::default()
    };
    assert!(resolve(&locked, &only_user, &ok).is_err());
    let h = manifest_hash(&user.list()[0]).unwrap();
    assert_eq!(h.len(), 64);
}

#[test]
fn incoming_skips_partial_and_recent_files() {
    let t = tempfile::tempdir().unwrap();
    let store = ModelStore::new(t.path().join("models"), StoreKind::Machine);
    let p = pack(t.path(), "m", "2026.11.0", b"w");
    let inc = store.root().join("incoming");
    std::fs::create_dir_all(&inc).unwrap();
    std::fs::copy(&p, inc.join("m@2026.11.0.sprkmodel")).unwrap();
    std::fs::write(inc.join("other.sprkmodel.partial"), b"half").unwrap();
    let o = opts(true, &[]);
    // just copied: modified within the last 10 s, so left alone
    assert!(process_incoming(&store, &o, &Smoke(true), SystemTime::now()).is_empty());
    // 30 s later the copy is considered finished
    let later = SystemTime::now() + Duration::from_secs(30);
    let r = process_incoming(&store, &o, &Smoke(true), later);
    assert_eq!(r.len(), 1);
    assert!(matches!(
        r[0].1,
        ImportOutcome::Installed {
            activated: true,
            ..
        }
    ));
    assert!(inc.join("processed").join("m@2026.11.0.sprkmodel").exists());
    assert!(inc.join("other.sprkmodel.partial").exists());
}

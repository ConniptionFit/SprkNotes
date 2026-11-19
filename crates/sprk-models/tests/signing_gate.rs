//! M15 gate, portable half: with signing required, only packs signed by a trusted key install,
//! rotation works with two keys, and the check is repeated when an installed model loads.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use ed25519_dalek::SigningKey;
use sha2::{Digest, Sha256};
use sprk_models::import::{import_pack, ImportOptions, ImportOutcome, SmokeTester};
use sprk_models::pack::{add_signature, pack_folder, read_manifest_bytes};
use sprk_models::sshsig::testing::sign;
use sprk_models::sshsig::{TrustedKey, NAMESPACE};
use sprk_models::trust::Trust;
use sprk_models::{Manifest, ModelStore, StoreKind};
use std::path::{Path, PathBuf};

struct Smoke;
impl SmokeTester for Smoke {
    fn smoke(&self, _d: &Path, _m: &Manifest) -> Result<(), String> {
        Ok(())
    }
}

fn make_pack(root: &Path, version: &str) -> PathBuf {
    let dir = root.join(format!("src-{version}"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("w.bin"), version.as_bytes()).unwrap();
    let sha = hex::encode(Sha256::digest(version.as_bytes()));
    let json = format!(
        r#"{{"schema":1,"id":"m","version":"{version}","display_name":"M","engine":"test/engine",
"languages":["en"],"sample_rate":16000,"features":{{"punctuation":true,"casing":true,"streaming":false}},
"accelerators":["cpu"],"min_app_version":"0.0.1","files":{{"model":"w.bin"}},
"sha256":{{"w.bin":"{sha}"}},"license":"MIT","source":"synthetic","notes":""}}"#
    );
    std::fs::write(dir.join("model.json"), json).unwrap();
    let out = root.join(format!("m@{version}.sprkmodel"));
    pack_folder(&dir, &out).unwrap();
    out
}

fn signed(pack: &Path, sk: &SigningKey, out: &Path) {
    let sig = sign(sk, NAMESPACE, &read_manifest_bytes(pack).unwrap());
    add_signature(pack, &sig, out).unwrap();
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn trust(app: &str, keys: &[&SigningKey]) -> Trust {
    Trust {
        app_version: app.into(),
        policy_require_signed: false,
        keys: keys
            .iter()
            .map(|k| TrustedKey(k.verifying_key().to_bytes()))
            .collect(),
        allowed_hashes: vec![],
    }
}

fn install(store: &ModelStore, t: &Trust, pack: &Path) -> ImportOutcome {
    let v = t.verifier();
    let engines = ["test/engine"];
    let o = ImportOptions {
        verify: t.verify_options(&engines, &v),
        activate: true,
        keep_versions: 3,
        now: "t",
    };
    import_pack(store, pack, &o, &Smoke)
}

#[test]
fn one_point_zero_installs_only_signed_packs() {
    let tmp = tempfile::tempdir().unwrap();
    let store = ModelStore::new(tmp.path().join("store"), StoreKind::Machine);
    let (release, backup, stranger) = (key(1), key(2), key(9));
    let t = trust("1.0.0", &[&release, &backup]);

    let unsigned = make_pack(tmp.path(), "2027.01.0");
    assert!(
        matches!(
            install(&store, &t, &unsigned),
            ImportOutcome::Rejected { .. }
        ),
        "unsigned refused"
    );

    let p = make_pack(tmp.path(), "2027.02.0");
    let bad = tmp.path().join("bad.sprkmodel");
    signed(&p, &stranger, &bad);
    assert!(
        matches!(install(&store, &t, &bad), ImportOutcome::Rejected { .. }),
        "untrusted key refused"
    );

    let p = make_pack(tmp.path(), "2027.03.0");
    let good = tmp.path().join("good.sprkmodel");
    signed(&p, &release, &good);
    assert!(matches!(
        install(&store, &t, &good),
        ImportOutcome::Installed {
            activated: true,
            ..
        }
    ));

    // the backup key also works
    let p = make_pack(tmp.path(), "2027.04.0");
    let good2 = tmp.path().join("good2.sprkmodel");
    signed(&p, &backup, &good2);
    assert!(matches!(
        install(&store, &t, &good2),
        ImportOutcome::Installed { .. }
    ));
}

#[test]
fn pilot_builds_install_unsigned_but_still_verify_a_present_signature_when_required() {
    let tmp = tempfile::tempdir().unwrap();
    let store = ModelStore::new(tmp.path().join("store"), StoreKind::Machine);
    let k = key(1);
    let pilot = trust("0.9.0", &[&k]);
    let unsigned = make_pack(tmp.path(), "2027.01.0");
    assert!(matches!(
        install(&store, &pilot, &unsigned),
        ImportOutcome::Installed { .. }
    ));
    let mut strict = trust("0.9.0", &[&k]);
    strict.policy_require_signed = true;
    let p = make_pack(tmp.path(), "2027.02.0");
    assert!(matches!(
        install(&store, &strict, &p),
        ImportOutcome::Rejected { .. }
    ));
}

#[test]
fn rotation_old_key_removed_by_policy_stops_verifying() {
    let tmp = tempfile::tempdir().unwrap();
    let store = ModelStore::new(tmp.path().join("store"), StoreKind::Machine);
    let (old, new) = (key(1), key(2));
    let p = make_pack(tmp.path(), "2027.01.0");
    let by_old = tmp.path().join("old.sprkmodel");
    signed(&p, &old, &by_old);
    assert!(matches!(
        install(&store, &trust("1.0.0", &[&old, &new]), &by_old),
        ImportOutcome::Installed { .. }
    ));
    // later: policy drops the old key; installed model fails the load-time check
    let t = trust("1.0.0", &[&new]);
    let inst = store.get("m@2027.01.0").unwrap();
    assert!(t.check_installed(&inst).is_err());
    assert!(trust("1.0.0", &[&old]).check_installed(&inst).is_ok());
}

#[test]
fn hand_placed_unsigned_model_fails_the_load_time_check() {
    let tmp = tempfile::tempdir().unwrap();
    let store = ModelStore::new(tmp.path().join("store"), StoreKind::Machine);
    let pilot = trust("0.9.0", &[]);
    let p = make_pack(tmp.path(), "2027.01.0");
    install(&store, &pilot, &p);
    let inst = store.get("m@2027.01.0").unwrap();
    assert!(pilot.check_installed(&inst).is_ok());
    let release = trust("1.0.0", &[&key(1)]);
    assert!(release
        .check_installed(&inst)
        .unwrap_err()
        .contains("not signed"));
}

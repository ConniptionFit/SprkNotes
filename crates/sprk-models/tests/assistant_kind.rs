//! ADR-010: assistant packs and transcription packs never mix. Each store accepts one kind.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use sha2::{Digest, Sha256};
use sprk_models::import::{import_pack, ImportOptions, ImportOutcome, SmokeTester};
use sprk_models::manifest::{KIND_ASSISTANT, KIND_TRANSCRIPTION};
use sprk_models::pack::pack_folder;
use sprk_models::trust::Trust;
use sprk_models::{Manifest, ModelStore, StoreKind};
use std::path::{Path, PathBuf};

struct Smoke;
impl SmokeTester for Smoke {
    fn smoke(&self, _d: &Path, _m: &Manifest) -> Result<(), String> {
        Ok(())
    }
}

fn assistant_pack(root: &Path) -> PathBuf {
    let dir = root.join("src-a");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("m.gguf"), b"weights").unwrap();
    let sha = hex::encode(Sha256::digest(b"weights"));
    let json = format!(
        r#"{{"schema":1,"kind":"assistant","id":"asst","version":"2027.01.0","display_name":"A",
"engine":"test/engine","min_app_version":"0.0.1","files":{{"weights":"m.gguf"}},
"sha256":{{"m.gguf":"{sha}"}},
"assistant":{{"tier":"light","prompt_format":"chatml","context_tokens":4096,"ram_estimate_mb":1500}},
"license":"Apache-2.0","source":"synthetic"}}"#
    );
    std::fs::write(dir.join("model.json"), json).unwrap();
    let out = root.join("asst@2027.01.0.sprkmodel");
    pack_folder(&dir, &out).unwrap();
    out
}

fn install(store: &ModelStore, pack: &Path) -> ImportOutcome {
    let t = Trust {
        app_version: "0.0.1".into(),
        policy_require_signed: false,
        keys: vec![],
        allowed_hashes: vec![],
    };
    let v = t.verifier();
    let engines = ["test/engine"];
    let o = ImportOptions {
        verify: t.verify_options(&engines, &v),
        activate: true,
        keep_versions: 2,
        now: "t",
    };
    import_pack(store, pack, &o, &Smoke)
}

#[test]
fn assistant_pack_installs_only_into_an_assistant_store() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = assistant_pack(tmp.path());
    let speech = ModelStore::new(tmp.path().join("speech"), StoreKind::User);
    assert_eq!(speech.pack_kind(), KIND_TRANSCRIPTION);
    match install(&speech, &pack) {
        ImportOutcome::Rejected { reason } => assert!(reason.contains("an assistant pack")),
        other => panic!("expected rejection, got {other:?}"),
    }
    assert!(speech.list().is_empty());

    let pack = assistant_pack(tmp.path());
    let asst = ModelStore::for_kind(tmp.path().join("asst"), StoreKind::User, KIND_ASSISTANT);
    assert!(matches!(
        install(&asst, &pack),
        ImportOutcome::Installed {
            activated: true,
            ..
        }
    ));
    assert_eq!(asst.list().len(), 1);
}

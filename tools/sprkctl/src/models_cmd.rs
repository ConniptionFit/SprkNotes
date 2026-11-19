//! `sprkctl model ...` (SPEC 16.7). Exit codes are RMM-friendly:
//! 0 ok, 10 verify failed, 11 smoke test failed, 12 incompatible, 20 not elevated.
use anyhow::{bail, Context, Result};
use sprk_models::import::{
    import_pack, process_incoming, ImportOptions, ImportOutcome, SmokeTester,
};
use sprk_models::pack::{
    add_signature, pack_folder, read_manifest_bytes, verify_pack, VerifyOptions,
};
use sprk_models::sshsig::{parse_keys, SshSigVerifier};
use sprk_models::{Manifest, ModelStore, StoreKind};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const EXIT_VERIFY: i32 = 10;
pub const EXIT_SMOKE: i32 = 11;
pub const EXIT_INCOMPATIBLE: i32 = 12;
pub const EXIT_NOT_ELEVATED: i32 = 20;

/// Engines this build can load. With the engine feature this is the live registry;
/// otherwise a fixed list so packs can still be checked and installed on any machine.
fn supported_engines() -> Vec<&'static str> {
    #[cfg(any(feature = "engine", feature = "engine-shared"))]
    {
        let mut reg = sprk_asr::Registry::default();
        sprk_asr::sherpa::register(&mut reg);
        reg.engines()
    }
    #[cfg(not(any(feature = "engine", feature = "engine-shared")))]
    {
        vec![
            "sherpa-onnx/offline-transducer",
            "sherpa-onnx/whisper",
            "sherpa-onnx/vad-silero",
        ]
    }
}

/// Smoke test: with the engine feature the model must load through its adapter. Builds
/// without an engine can only check file hashes (the pack was already hash-verified).
struct Smoke;
impl SmokeTester for Smoke {
    fn smoke(&self, dir: &Path, manifest: &Manifest) -> Result<(), String> {
        #[cfg(any(feature = "engine", feature = "engine-shared"))]
        {
            if manifest.engine == "sherpa-onnx/vad-silero" {
                return sprk_asr::sherpa::SileroVad::load(manifest, dir, 20.0)
                    .map(|_| ())
                    .map_err(|e| e.to_string());
            }
            let mut reg = sprk_asr::Registry::default();
            sprk_asr::sherpa::register(&mut reg);
            reg.load(manifest, dir, env!("CARGO_PKG_VERSION"))
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        #[cfg(not(any(feature = "engine", feature = "engine-shared")))]
        {
            let _ = dir;
            manifest.verify_files(dir).map_err(|e| e.to_string())
        }
    }
}

struct Flags {
    scope: Option<String>,
    root: Option<PathBuf>,
    output: Option<PathBuf>,
    no_activate: bool,
    json: bool,
    require_signature: bool,
    pubkey: Option<PathBuf>,
    keys: Option<PathBuf>,
    keep: usize,
    positional: Vec<String>,
}

fn parse(args: &[&str]) -> Result<Flags> {
    let mut f = Flags {
        scope: None,
        root: None,
        output: None,
        no_activate: false,
        json: false,
        require_signature: false,
        pubkey: None,
        keys: None,
        keep: 2,
        positional: vec![],
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--scope" => {
                f.scope = Some(
                    it.next()
                        .context("--scope needs machine or user")?
                        .to_string(),
                )
            }
            "--root" => f.root = Some(it.next().context("--root needs a folder")?.into()),
            "-o" => f.output = Some(it.next().context("-o needs a file")?.into()),
            "--no-activate" => f.no_activate = true,
            "--json" => f.json = true,
            "--require-signature" => f.require_signature = true,
            "--pubkey" => f.pubkey = Some(it.next().context("--pubkey needs a .pub file")?.into()),
            "--keys" => {
                f.keys = Some(
                    it.next()
                        .context("--keys needs an allowed_signers file")?
                        .into(),
                )
            }
            "--keep" => f.keep = it.next().context("--keep needs a number")?.parse()?,
            x if x.starts_with("--") => bail!("unknown option {x}"),
            x => f.positional.push(x.to_string()),
        }
    }
    Ok(f)
}

fn store_for(f: &Flags) -> Result<ModelStore> {
    let (root, kind) = if let Some(r) = &f.root {
        (
            r.clone(),
            if f.scope.as_deref() == Some("user") {
                StoreKind::User
            } else {
                StoreKind::Machine
            },
        )
    } else {
        match f.scope.as_deref() {
            Some("machine") => (
                std::env::var_os("PROGRAMDATA")
                    .map(|p| PathBuf::from(p).join("SprkNotes").join("models"))
                    .unwrap_or_else(|| "/var/lib/sprknotes/models".into()),
                StoreKind::Machine,
            ),
            Some("user") => (
                std::env::var_os("APPDATA")
                    .map(|p| PathBuf::from(p).join("SprkNotes").join("models"))
                    .or_else(|| {
                        std::env::var_os("HOME")
                            .map(|h| PathBuf::from(h).join(".local/share/sprknotes/models"))
                    })
                    .context("cannot find the user data folder")?,
                StoreKind::User,
            ),
            _ => bail!("--scope machine|user is required (or --root <folder>)"),
        }
    };
    Ok(ModelStore::new(root, kind))
}

/// The stores the app resolves from: bundled (next to the exe), machine, user.
fn list_all_stores() -> Vec<(&'static str, ModelStore)> {
    let mut v = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("models")))
    {
        v.push(("bundled", ModelStore::new(dir, StoreKind::Bundled)));
    }
    if let Some(p) = std::env::var_os("PROGRAMDATA") {
        v.push((
            "machine",
            ModelStore::new(
                PathBuf::from(p).join("SprkNotes").join("models"),
                StoreKind::Machine,
            ),
        ));
    }
    if let Some(p) = std::env::var_os("APPDATA") {
        v.push((
            "user",
            ModelStore::new(
                PathBuf::from(p).join("SprkNotes").join("models"),
                StoreKind::User,
            ),
        ));
    }
    v
}

fn iso_now() -> String {
    // Epoch seconds as a sortable stamp; the app formats dates for display.
    let s = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("epoch:{s}")
}

fn exit_for(reason: &str) -> i32 {
    let r = reason.to_lowercase();
    if r.contains("smoke") {
        EXIT_SMOKE
    } else if r.contains("needs app") || r.contains("unknown engine") {
        EXIT_INCOMPATIBLE
    } else {
        EXIT_VERIFY
    }
}

fn permission_denied(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::PermissionDenied
}

/// Run a `model` subcommand; returns the process exit code.
pub fn run(args: &[&str]) -> Result<i32> {
    let (cmd, rest) = args.split_first().context("missing model subcommand")?;
    let f = parse(rest)?;
    let engines = supported_engines();
    let app_version = env!("CARGO_PKG_VERSION");
    let none: Vec<String> = Vec::new();
    let key_text = match &f.keys {
        Some(p) => {
            std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?
        }
        None => String::new(),
    };
    let verifier = SshSigVerifier {
        trusted: parse_keys(&key_text),
    };
    // From 1.0 signatures are always required (SPEC 16.10); earlier builds follow the flag.
    let require = f.require_signature
        || app_version
            .split('.')
            .next()
            .and_then(|m| m.parse::<u32>().ok())
            .unwrap_or(0)
            >= 1;
    let vopts = VerifyOptions {
        supported_engines: &engines,
        app_version,
        allowed_hashes: &none,
        require_signature: require,
        verifier: Some(&verifier),
    };
    match *cmd {
        "pack" => {
            let folder = f.positional.first().context("missing <folder>")?;
            let out = f.output.clone().context("missing -o <file.sprkmodel>")?;
            match pack_folder(Path::new(folder), &out) {
                Ok(m) => {
                    println!("packed {}@{}", m.id, m.version);
                    Ok(0)
                }
                Err(e) => {
                    eprintln!("pack failed: {e}");
                    Ok(EXIT_VERIFY)
                }
            }
        }
        "verify" => {
            let target = Path::new(
                f.positional
                    .first()
                    .context("missing <file.sprkmodel|folder>")?,
            );
            if target.is_dir() {
                let text = std::fs::read_to_string(target.join("model.json"))
                    .context("reading model.json")?;
                let r = Manifest::from_json(&text)
                    .map_err(|e| e.to_string())
                    .and_then(|m| m.verify_files(target).map_err(|e| e.to_string()));
                return Ok(match r {
                    Ok(()) => {
                        println!("ok");
                        0
                    }
                    Err(e) => {
                        eprintln!("verify failed: {e}");
                        EXIT_VERIFY
                    }
                });
            }
            match verify_pack(target, &vopts) {
                Ok(v) => {
                    println!(
                        "ok {}@{} model.json sha256 {}",
                        v.manifest.id, v.manifest.version, v.manifest_hash
                    );
                    Ok(0)
                }
                Err(e) => {
                    eprintln!("verify failed: {e}");
                    Ok(exit_for(&e.to_string()))
                }
            }
        }
        "hash" => {
            let p = Path::new(f.positional.first().context("missing <file.sprkmodel>")?);
            match verify_pack(p, &vopts) {
                Ok(v) => {
                    println!("{}", v.manifest_hash);
                    Ok(0)
                }
                Err(e) => {
                    eprintln!("verify failed: {e}");
                    Ok(exit_for(&e.to_string()))
                }
            }
        }
        "install" => {
            let store = store_for(&f)?;
            if let Err(e) = std::fs::create_dir_all(store.root()) {
                eprintln!("cannot write the model store: {e}");
                return Ok(if permission_denied(&e) {
                    EXIT_NOT_ELEVATED
                } else {
                    EXIT_VERIFY
                });
            }
            let pack = Path::new(f.positional.first().context("missing <file.sprkmodel>")?);
            let now = iso_now();
            let o = ImportOptions {
                verify: vopts,
                activate: !f.no_activate,
                keep_versions: f.keep,
                now: &now,
            };
            match import_pack(&store, pack, &o, &Smoke) {
                ImportOutcome::Installed {
                    id_version,
                    activated,
                } => {
                    println!("installed {id_version} activated={activated}");
                    Ok(0)
                }
                ImportOutcome::Rejected { reason } => {
                    eprintln!("rejected: {reason}");
                    Ok(exit_for(&reason))
                }
            }
        }
        "import" => {
            let store = store_for(&f)?;
            let now = iso_now();
            let o = ImportOptions {
                verify: vopts,
                activate: !f.no_activate,
                keep_versions: f.keep,
                now: &now,
            };
            let results = process_incoming(&store, &o, &Smoke, SystemTime::now());
            let mut code = 0;
            for (p, r) in &results {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                match r {
                    ImportOutcome::Installed {
                        id_version,
                        activated,
                    } => println!("{name}: installed {id_version} activated={activated}"),
                    ImportOutcome::Rejected { reason } => {
                        eprintln!("{name}: rejected: {reason}");
                        code = code.max(exit_for(reason));
                    }
                }
            }
            Ok(code)
        }
        "list" => {
            // No scope and no --root: show every store the app looks in, bundled one included.
            let stores: Vec<(&str, ModelStore)> = if f.scope.is_none() && f.root.is_none() {
                list_all_stores()
            } else {
                vec![(
                    match f.scope.as_deref() {
                        Some("user") => "user",
                        Some("machine") => "machine",
                        _ => "root",
                    },
                    store_for(&f)?,
                )]
            };
            let mut rows = Vec::new();
            for (name, store) in &stores {
                let active = store.active().map(|a| a.active);
                for i in store.list() {
                    let is_active = active.as_deref() == Some(i.id_version.as_str());
                    rows.push((*name, i.id_version, is_active));
                }
            }
            if f.json {
                let v: Vec<_> = rows
                    .iter()
                    .map(|(s, id, a)| serde_json::json!({ "store": s, "id_version": id, "active": a }))
                    .collect();
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                for (s, id, a) in &rows {
                    println!("{} {id}  ({s})", if *a { "*" } else { " " });
                }
            }
            Ok(0)
        }
        "activate" | "rollback" | "remove" => {
            let store = store_for(&f)?;
            let now = iso_now();
            let r = match *cmd {
                "activate" => store
                    .activate(f.positional.first().context("missing <id@version>")?, &now)
                    .map(|_| "activated".to_string()),
                "remove" => store
                    .remove(f.positional.first().context("missing <id@version>")?)
                    .map(|_| "removed".to_string()),
                _ => store.rollback(&now).map(|p| format!("rolled back to {p}")),
            };
            match r {
                Ok(msg) => {
                    println!("{msg}");
                    Ok(0)
                }
                Err(sprk_models::StoreError::Io(e)) if permission_denied(&e) => {
                    eprintln!("not elevated: {e}");
                    Ok(EXIT_NOT_ELEVATED)
                }
                Err(e) => {
                    eprintln!("{e}");
                    Ok(EXIT_VERIFY)
                }
            }
        }
        "sign" => {
            // Signing goes through ssh-keygen and the 1Password SSH agent; this code never
            // reads or stores a private key.
            let pack = Path::new(f.positional.first().context("missing <file.sprkmodel>")?);
            let pubkey = f.pubkey.clone().context("--pubkey <key.pub> is required")?;
            let json = read_manifest_bytes(pack).context("reading model.json from the pack")?;
            let work = std::env::temp_dir().join(format!("sprkctl-sign-{}", std::process::id()));
            std::fs::create_dir_all(&work)?;
            let json_path = work.join("model.json");
            std::fs::write(&json_path, &json)?;
            let status = std::process::Command::new("ssh-keygen")
                .args(["-Y", "sign", "-n", sprk_models::sshsig::NAMESPACE, "-f"])
                .arg(&pubkey)
                .arg(&json_path)
                .status()
                .context(
                    "running ssh-keygen (is OpenSSH installed and the 1Password agent running?)",
                )?;
            let result = if status.success() {
                let sig = std::fs::read(work.join("model.json.sig"))
                    .context("ssh-keygen wrote no signature")?;
                let tmp = pack.with_extension("sprkmodel.signing");
                add_signature(pack, &sig, &tmp).map_err(|e| anyhow::anyhow!("{e}"))?;
                std::fs::rename(&tmp, pack)?;
                println!("signed {}", pack.display());
                0
            } else {
                eprintln!("ssh-keygen failed to sign");
                1
            };
            let _ = std::fs::remove_dir_all(&work);
            Ok(result)
        }
        "allow" | "bench" | "sync" => {
            eprintln!("`model {cmd}` is not available in this build yet");
            Ok(1)
        }
        other => bail!("unknown model subcommand '{other}'"),
    }
}

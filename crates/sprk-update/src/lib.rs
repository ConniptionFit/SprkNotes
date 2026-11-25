//! App update check (ADR-011). Pure logic: versions and channels, release sources (Forgejo now,
//! GitHub ready), the signed update manifest and the decision "is there something newer for me".
//! The network (WinHTTP) and the install step (msiexec) live in the app; this crate only decides
//! and verifies, so all of it is testable on any host.
//!
//! Nothing here is sent to the release host except plain GETs. No user, PC or transcript data.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sprk_models::remote::{check_https, Fetcher, RemoteError};
use sprk_models::sshsig::{self, TrustedKey};
use std::cmp::Ordering;

pub const MANIFEST_SCHEMA: u32 = 1;
/// Release asset names. Hosts that only take a few file types get the MSI as `.zip`; the
/// manifest names the exact asset and its SHA-256.
pub const MANIFEST_ASSET: &str = "update-manifest.json";
pub const MANIFEST_SIG_ASSET: &str = "update-manifest.json.sig";
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
pub const MAX_RELEASES_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_PACKAGE_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum UpdateError {
    #[error("update source is not valid")]
    BadSource,
    #[error("the update server sent something unexpected")]
    BadReply,
    #[error("update signature check failed: {0}")]
    Signature(String),
    #[error("the update did not match its checksum")]
    Checksum,
    #[error("could not reach the update server")]
    Network,
}

impl From<RemoteError> for UpdateError {
    fn from(e: RemoteError) -> Self {
        match e {
            RemoteError::Checksum => UpdateError::Checksum,
            RemoteError::NotHttps => UpdateError::BadSource,
            _ => UpdateError::Network,
        }
    }
}

// ---------- channels and versions ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    /// Only final releases.
    Stable,
    /// Stable plus beta pre-releases.
    Beta,
    /// Everything: stable, beta and alpha.
    Alpha,
}

impl Channel {
    pub fn parse(s: &str) -> Option<Channel> {
        match s.trim().to_ascii_lowercase().as_str() {
            "stable" => Some(Channel::Stable),
            "beta" => Some(Channel::Beta),
            "alpha" => Some(Channel::Alpha),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
            Channel::Alpha => "alpha",
        }
    }
}

/// Release stage of one version. Order: alpha < beta < stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Stage {
    Alpha,
    Beta,
    Stable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    core: [u64; 3],
    stage: Stage,
    pre_n: u64,
}

impl Version {
    /// Accepts `1.2.3`, `v1.2.3`, `1.2.3-beta.4`, `1.2.3-alpha`. Other suffixes (`-pilot1`, `-rc.1`)
    /// are not on any channel and return `None`.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim().trim_start_matches(['v', 'V']);
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (s, None),
        };
        let mut nums = [0u64; 3];
        let mut it = core.split('.');
        for n in nums.iter_mut() {
            *n = it.next()?.parse().ok()?;
        }
        if it.next().is_some() {
            return None;
        }
        let (stage, pre_n) = match pre {
            None => (Stage::Stable, 0),
            Some(p) => {
                let (name, n) = match p.split_once('.') {
                    Some((a, b)) => (a, b.parse().ok()?),
                    None => (p, 0),
                };
                match name {
                    "alpha" => (Stage::Alpha, n),
                    "beta" => (Stage::Beta, n),
                    _ => return None,
                }
            }
        };
        Some(Version {
            core: nums,
            stage,
            pre_n,
        })
    }

    pub fn is_on(&self, channel: Channel) -> bool {
        match self.stage {
            Stage::Stable => true,
            Stage::Beta => channel >= Channel::Beta,
            Stage::Alpha => channel >= Channel::Alpha,
        }
    }

    /// MSI ProductVersion cannot carry a pre-release tag, so the release script maps it:
    /// stable `x.y.z`, beta `x.y.z.(1000+n)`, alpha `x.y.z.n`. Windows Installer ignores the
    /// fourth field when ordering upgrades, so the MSI allows same-version upgrades and this
    /// crate decides what is newer.
    pub fn msi_version(&self) -> String {
        let c = self.core;
        match self.stage {
            Stage::Stable => format!("{}.{}.{}", c[0], c[1], c[2]),
            Stage::Beta => format!("{}.{}.{}.{}", c[0], c[1], c[2], 1000 + self.pre_n.min(999)),
            Stage::Alpha => format!("{}.{}.{}.{}", c[0], c[1], c[2], self.pre_n.min(999)),
        }
    }
}

impl Ord for Version {
    fn cmp(&self, o: &Self) -> Ordering {
        (self.core, self.stage, self.pre_n).cmp(&(o.core, o.stage, o.pre_n))
    }
}
impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

// ---------- release sources ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Forgejo,
    Github,
}

/// Where releases come from. `parse` takes the repository address an admin would paste.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub provider: Provider,
    /// `https://host` without a trailing slash.
    pub base: String,
    pub owner: String,
    pub repo: String,
}

impl Source {
    /// `https://git.pwrs.dev/powers/sprknotes` or `https://github.com/owner/repo`. github.com is
    /// recognised by host; anything else is treated as Forgejo (Gitea API) unless `provider`
    /// says otherwise.
    pub fn parse(url: &str, provider: Option<&str>) -> Result<Source, UpdateError> {
        check_https(url).map_err(|_| UpdateError::BadSource)?;
        let rest = &url[8..];
        let rest = rest.trim_end_matches('/').trim_end_matches(".git");
        let mut it = rest.split('/');
        let host = it.next().unwrap_or("");
        let owner = it.next().unwrap_or("");
        let repo = it.next().unwrap_or("");
        let ok = |s: &str| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                && !s.starts_with('.')
        };
        let host_ok = !host.is_empty()
            && !host.contains('@')
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':'));
        if !host_ok || !ok(owner) || !ok(repo) || it.next().is_some() {
            return Err(UpdateError::BadSource);
        }
        let provider = match provider.map(|p| p.trim().to_ascii_lowercase()) {
            Some(p) if p == "github" => Provider::Github,
            Some(p) if p == "forgejo" || p == "gitea" => Provider::Forgejo,
            Some(_) => return Err(UpdateError::BadSource),
            None if host.eq_ignore_ascii_case("github.com") => Provider::Github,
            None => Provider::Forgejo,
        };
        Ok(Source {
            provider,
            base: format!("https://{host}"),
            owner: owner.into(),
            repo: repo.into(),
        })
    }

    /// The list-releases API address.
    pub fn releases_url(&self) -> String {
        match self.provider {
            Provider::Forgejo => format!(
                "{}/api/v1/repos/{}/{}/releases?limit=50",
                self.base, self.owner, self.repo
            ),
            Provider::Github => {
                let api = if self.base.eq_ignore_ascii_case("https://github.com") {
                    "https://api.github.com".to_string()
                } else {
                    format!("{}/api/v3", self.base)
                };
                format!(
                    "{api}/repos/{}/{}/releases?per_page=50",
                    self.owner, self.repo
                )
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

impl Release {
    fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// Forgejo and GitHub return the same fields we use, so one parser serves both.
pub fn parse_releases(bytes: &[u8]) -> Result<Vec<Release>, UpdateError> {
    serde_json::from_slice(bytes).map_err(|_| UpdateError::BadReply)
}

/// Newest release on `channel` that carries an update manifest. Drafts and tags that are not on
/// any channel (`v0.0.1-pilot1`, `models`) are skipped.
pub fn newest_for_channel(releases: &[Release], channel: Channel) -> Option<(Version, &Release)> {
    releases
        .iter()
        .filter(|r| !r.draft && r.asset(MANIFEST_ASSET).is_some())
        .filter_map(|r| Version::parse(&r.tag_name).map(|v| (v, r)))
        .filter(|(v, _)| v.is_on(channel))
        .max_by(|a, b| a.0.cmp(&b.0))
}

// ---------- manifest ----------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Package {
    /// `x64` or `arm64`.
    pub arch: String,
    /// Release asset name of the installer (an MSI; hosts may store it with a `.zip` suffix).
    pub asset: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub version: String,
    pub channel: Channel,
    #[serde(default)]
    pub notes_url: String,
    pub packages: Vec<Package>,
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Manifest, UpdateError> {
        let m: Manifest = serde_json::from_slice(bytes).map_err(|_| UpdateError::BadReply)?;
        let hex = |h: &str| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit());
        if m.schema != MANIFEST_SCHEMA || Version::parse(&m.version).is_none() {
            return Err(UpdateError::BadReply);
        }
        for p in &m.packages {
            if !hex(&p.sha256)
                || p.size == 0
                || p.size > MAX_PACKAGE_BYTES
                || p.asset.contains(['/', '\\'])
                || p.asset.contains("..")
            {
                return Err(UpdateError::BadReply);
            }
        }
        Ok(m)
    }
}

/// What the checker found: an installable package for this PC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Update {
    pub version: String,
    pub channel: Channel,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub notes_url: String,
}

/// Trust rules for update manifests.
pub struct Trust {
    /// Admin policy `UpdatePublicKeys` (OpenSSH `ssh-ed25519 ...` lines).
    pub keys: Vec<TrustedKey>,
    /// Unsigned manifests are accepted only while this is false (pilot builds). From 1.0 the app
    /// forces it to true.
    pub require_signature: bool,
}

/// Everything the check needs from the outside.
pub struct Query<'a> {
    pub source: &'a Source,
    pub channel: Channel,
    /// This build's version (`CARGO_PKG_VERSION`, possibly with a pre-release tag).
    pub installed: &'a str,
    /// `x64` or `arm64`.
    pub arch: &'a str,
    pub trust: &'a Trust,
}

/// Look for a newer release on the channel. `Ok(None)` means up to date. Downgrades are never
/// offered: moving to a lower channel keeps the current build until a newer one appears there.
pub fn check(f: &dyn Fetcher, q: &Query) -> Result<Option<Update>, UpdateError> {
    let installed = Version::parse(q.installed).ok_or(UpdateError::BadSource)?;
    let list = f.get_bytes(&q.source.releases_url(), MAX_RELEASES_BYTES)?;
    let releases = parse_releases(&list)?;
    let Some((version, release)) = newest_for_channel(&releases, q.channel) else {
        return Ok(None);
    };
    if version <= installed {
        return Ok(None);
    }
    let man_asset = release.asset(MANIFEST_ASSET).ok_or(UpdateError::BadReply)?;
    check_https(&man_asset.browser_download_url)?;
    let man_bytes = f.get_bytes(&man_asset.browser_download_url, MAX_MANIFEST_BYTES)?;

    match release.asset(MANIFEST_SIG_ASSET) {
        Some(sig) => {
            check_https(&sig.browser_download_url)?;
            let sig_bytes = f.get_bytes(&sig.browser_download_url, 16 * 1024)?;
            sshsig::verify_in(
                sshsig::UPDATE_NAMESPACE,
                &man_bytes,
                &sig_bytes,
                &q.trust.keys,
            )
            .map_err(UpdateError::Signature)?;
        }
        None if q.trust.require_signature => {
            return Err(UpdateError::Signature("the update is not signed".into()))
        }
        None => {}
    }

    let m = Manifest::parse(&man_bytes)?;
    // The signed manifest must agree with the release tag it was found under.
    if Version::parse(&m.version).as_ref() != Some(&version) {
        return Err(UpdateError::BadReply);
    }
    let Some(pkg) = m.packages.iter().find(|p| p.arch == q.arch) else {
        return Ok(None);
    };
    let asset = release.asset(&pkg.asset).ok_or(UpdateError::BadReply)?;
    check_https(&asset.browser_download_url)?;
    Ok(Some(Update {
        version: m.version,
        channel: q.channel,
        url: asset.browser_download_url.clone(),
        sha256: pkg.sha256.to_ascii_lowercase(),
        size: pkg.size,
        notes_url: m.notes_url,
    }))
}

/// SHA-256 of a downloaded installer, to compare against `Update::sha256`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn verify_download(update: &Update, bytes: &[u8]) -> Result<(), UpdateError> {
    if bytes.len() as u64 != update.size || sha256_hex(bytes) != update.sha256 {
        return Err(UpdateError::Checksum);
    }
    Ok(())
}

// ---------- tests ----------

/// The command line that installs an update and starts the app again ("Update now"). It runs in
/// `cmd.exe /d /s /c` after SprkNotes has exited: wait a moment for the app's files to close,
/// run the installer with Windows Installer's own progress window (`/passive`: a bar, no
/// questions), wait for it, then start the app. The app starts again even if the install failed
/// (Windows Installer rolls back), so the user is never left without it. `None` when a path holds
/// a character cmd would read as syntax (`"` or `%`); the caller then installs without relaunch.
pub fn install_then_relaunch(msi: &str, log: &str, exe: &str) -> Option<String> {
    if [msi, log, exe]
        .iter()
        .any(|p| p.is_empty() || p.contains(['"', '%', '\n', '\r']))
    {
        return None;
    }
    Some(format!(
        "\"ping -n 3 127.0.0.1 >nul & start \"\" /wait msiexec.exe /i \"{msi}\" /passive /norestart /l*v \"{log}\" & start \"\" \"{exe}\"\""
    ))
}

/// Download progress as a whole percent, or `None` when the size is unknown.
pub fn percent(done: u64, total: Option<u64>) -> Option<u8> {
    let t = total.filter(|t| *t > 0)?;
    Some((done.min(t) * 100 / t) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use sprk_models::sshsig::testing::sign;
    use std::collections::HashMap;
    use std::path::Path;

    struct Fake(HashMap<String, Vec<u8>>);
    impl Fetcher for Fake {
        fn get_bytes(&self, url: &str, _max: u64) -> Result<Vec<u8>, RemoteError> {
            self.0.get(url).cloned().ok_or(RemoteError::Network)
        }
        fn get_to_file(
            &self,
            _: &str,
            _: &Path,
            _: u64,
            _: &mut dyn FnMut(u64, Option<u64>),
        ) -> Result<(), RemoteError> {
            Err(RemoteError::Network)
        }
    }

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn versions_order_alpha_beta_stable() {
        assert!(v("0.2.0-alpha.1") < v("0.2.0-alpha.2"));
        assert!(v("0.2.0-alpha.9") < v("0.2.0-beta.1"));
        assert!(v("0.2.0-beta.3") < v("0.2.0"));
        assert!(v("0.2.0") < v("0.2.1-alpha.1"));
        assert!(v("v1.0.0") > v("0.9.9"));
        assert!(Version::parse("0.0.1-pilot1").is_none());
        assert!(Version::parse("models").is_none());
        assert!(Version::parse("1.2").is_none());
    }

    #[test]
    fn channels_include_the_more_stable_ones() {
        assert!(v("1.0.0").is_on(Channel::Stable));
        assert!(!v("1.0.0-beta.1").is_on(Channel::Stable));
        assert!(v("1.0.0-beta.1").is_on(Channel::Beta));
        assert!(!v("1.0.0-alpha.1").is_on(Channel::Beta));
        assert!(v("1.0.0-alpha.1").is_on(Channel::Alpha));
        assert_eq!(Channel::parse(" Beta "), Some(Channel::Beta));
        assert_eq!(Channel::parse("nightly"), None);
    }

    #[test]
    fn msi_versions_follow_the_release_script_mapping() {
        let a = v("0.2.0-alpha.3").msi_version();
        let b = v("0.2.0-beta.1").msi_version();
        let s = v("0.2.0").msi_version();
        assert_eq!(
            (a.as_str(), b.as_str(), s.as_str()),
            ("0.2.0.3", "0.2.0.1001", "0.2.0")
        );
    }

    #[test]
    fn sources_parse_and_reject_junk() {
        let s = Source::parse("https://git.pwrs.dev/powers/sprknotes", None).unwrap();
        assert_eq!(s.provider, Provider::Forgejo);
        assert_eq!(
            s.releases_url(),
            "https://git.pwrs.dev/api/v1/repos/powers/sprknotes/releases?limit=50"
        );
        let g = Source::parse("https://github.com/acme/sprknotes.git", None).unwrap();
        assert_eq!(g.provider, Provider::Github);
        assert_eq!(
            g.releases_url(),
            "https://api.github.com/repos/acme/sprknotes/releases?per_page=50"
        );
        let e = Source::parse("https://ghe.corp.example/acme/app", Some("github")).unwrap();
        assert!(e
            .releases_url()
            .starts_with("https://ghe.corp.example/api/v3/repos/acme/app"));
        assert!(Source::parse("http://git.pwrs.dev/a/b", None).is_err());
        assert!(Source::parse("https://user@host/a/b", None).is_err());
        assert!(Source::parse("https://host/a", None).is_err());
        assert!(Source::parse("https://host/a/b/c", None).is_err());
        assert!(Source::parse("https://host/../b", None).is_err());
        assert!(Source::parse("https://host/a/b", Some("svn")).is_err());
    }

    const SRC: &str = "https://git.pwrs.dev/powers/sprknotes";

    fn world(sk: Option<&SigningKey>, tamper: bool) -> (Fake, Vec<u8>) {
        let msi = b"msi-bytes".to_vec();
        let man = serde_json::json!({
            "schema": 1, "version": "0.3.0-beta.1", "channel": "beta", "notes_url": "https://x/notes",
            "packages": [{"arch": "x64", "asset": "SprkNotes_0.3.0-beta.1_x64.msi.zip",
                "sha256": sha256_hex(&msi), "size": msi.len()}]
        })
        .to_string()
        .into_bytes();
        let mut assets = vec![
            serde_json::json!({"name": MANIFEST_ASSET, "browser_download_url": "https://h/m", "size": 1}),
            serde_json::json!({"name": "SprkNotes_0.3.0-beta.1_x64.msi.zip", "browser_download_url": "https://h/pkg", "size": 9}),
        ];
        let mut map: HashMap<String, Vec<u8>> = HashMap::new();
        map.insert("https://h/m".into(), man.clone());
        if let Some(sk) = sk {
            let signed = if tamper {
                b"other".to_vec()
            } else {
                man.clone()
            };
            map.insert(
                "https://h/s".into(),
                sign(sk, sshsig::UPDATE_NAMESPACE, &signed),
            );
            assets.push(serde_json::json!({"name": MANIFEST_SIG_ASSET, "browser_download_url": "https://h/s", "size": 1}));
        }
        let rels = serde_json::json!([
            {"tag_name": "models", "assets": []},
            {"tag_name": "v0.0.1-pilot1", "assets": []},
            {"tag_name": "v0.2.0", "assets": [{"name": MANIFEST_ASSET, "browser_download_url": "https://h/old", "size": 1}]},
            {"tag_name": "v0.3.0-beta.1", "prerelease": true, "assets": assets},
            {"tag_name": "v0.9.0", "draft": true, "assets": [{"name": MANIFEST_ASSET, "browser_download_url": "https://h/d", "size": 1}]}
        ]);
        map.insert(
            Source::parse(SRC, None).unwrap().releases_url(),
            rels.to_string().into_bytes(),
        );
        (Fake(map), msi)
    }

    fn q<'a>(src: &'a Source, ch: Channel, inst: &'a str, t: &'a Trust) -> Query<'a> {
        Query {
            source: src,
            channel: ch,
            installed: inst,
            arch: "x64",
            trust: t,
        }
    }

    #[test]
    fn beta_user_gets_the_signed_beta_and_stable_user_does_not() {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let (f, msi) = world(Some(&sk), false);
        let src = Source::parse(SRC, None).unwrap();
        let t = Trust {
            keys: vec![TrustedKey(sk.verifying_key().to_bytes())],
            require_signature: true,
        };
        let u = check(&f, &q(&src, Channel::Beta, "0.2.0", &t))
            .unwrap()
            .unwrap();
        assert_eq!(u.version, "0.3.0-beta.1");
        assert_eq!(u.url, "https://h/pkg");
        verify_download(&u, &msi).unwrap();
        assert_eq!(verify_download(&u, b"evil"), Err(UpdateError::Checksum));
        // stable channel: nothing newer than 0.2.0
        assert_eq!(
            check(&f, &q(&src, Channel::Stable, "0.2.0", &t)).unwrap(),
            None
        );
        // already on it, or on something newer: no downgrade
        assert_eq!(
            check(&f, &q(&src, Channel::Beta, "0.3.0-beta.1", &t)).unwrap(),
            None
        );
        assert_eq!(
            check(&f, &q(&src, Channel::Beta, "0.3.0", &t)).unwrap(),
            None
        );
        // wrong architecture published
        let mut qq = q(&src, Channel::Beta, "0.2.0", &t);
        qq.arch = "arm64";
        assert_eq!(check(&f, &qq).unwrap(), None);
    }

    #[test]
    fn bad_or_missing_signatures_are_refused_when_required() {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let other = SigningKey::from_bytes(&[9u8; 32]);
        let src = Source::parse(SRC, None).unwrap();
        let good = Trust {
            keys: vec![TrustedKey(sk.verifying_key().to_bytes())],
            require_signature: true,
        };
        // signature over different bytes
        let (f, _) = world(Some(&sk), true);
        assert!(matches!(
            check(&f, &q(&src, Channel::Beta, "0.2.0", &good)),
            Err(UpdateError::Signature(_))
        ));
        // signed by a key nobody trusts
        let (f, _) = world(Some(&other), false);
        assert!(matches!(
            check(&f, &q(&src, Channel::Beta, "0.2.0", &good)),
            Err(UpdateError::Signature(_))
        ));
        // unsigned while signatures are required
        let (f, _) = world(None, false);
        assert!(matches!(
            check(&f, &q(&src, Channel::Beta, "0.2.0", &good)),
            Err(UpdateError::Signature(_))
        ));
        // unsigned is fine on pilot builds
        let lax = Trust {
            keys: vec![],
            require_signature: false,
        };
        assert!(check(&f, &q(&src, Channel::Beta, "0.2.0", &lax))
            .unwrap()
            .is_some());
    }

    #[test]
    fn a_model_signature_cannot_pass_as_an_update_signature() {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let msg = b"{}";
        let model_sig = sign(&sk, sshsig::NAMESPACE, msg);
        let keys = [TrustedKey(sk.verifying_key().to_bytes())];
        assert!(sshsig::verify_in(sshsig::UPDATE_NAMESPACE, msg, &model_sig, &keys).is_err());
    }

    #[test]
    fn manifest_rejects_path_tricks_and_bad_hashes() {
        let base = |asset: &str, sha: &str| {
            serde_json::json!({"schema":1,"version":"1.0.0","channel":"stable",
                "packages":[{"arch":"x64","asset":asset,"sha256":sha,"size":5}]})
            .to_string()
            .into_bytes()
        };
        let sha = "a".repeat(64);
        assert!(Manifest::parse(&base("a.msi", &sha)).is_ok());
        assert!(Manifest::parse(&base("../a.msi", &sha)).is_err());
        assert!(Manifest::parse(&base("a\\b.msi", &sha)).is_err());
        assert!(Manifest::parse(&base("a.msi", "zz")).is_err());
    }

    #[test]
    fn relaunch_line_waits_installs_with_progress_then_starts_the_app() {
        let line = install_then_relaunch(
            r"C:\Users\Jo Smith\AppData\Roaming\SprkNotes\updates\pending.msi",
            r"C:\Users\Jo Smith\AppData\Roaming\SprkNotes\updates\install.log",
            r"C:\Users\Jo Smith\AppData\Local\SprkNotes\SprkNotes.exe",
        )
        .unwrap();
        assert!(line.starts_with("\"ping -n 3 127.0.0.1 >nul & start \"\" /wait msiexec.exe /i "));
        assert!(line.contains("\"C:\\Users\\Jo Smith\\AppData\\Roaming\\SprkNotes\\updates\\pending.msi\" /passive /norestart"));
        assert!(line.ends_with(
            "& start \"\" \"C:\\Users\\Jo Smith\\AppData\\Local\\SprkNotes\\SprkNotes.exe\"\""
        ));
        assert_eq!(install_then_relaunch(r"C:\100%\a.msi", "l", "e"), None);
        assert_eq!(install_then_relaunch("a.msi", "l", ""), None);
    }

    #[test]
    fn percent_is_bounded_and_needs_a_size() {
        assert_eq!(percent(50, Some(200)), Some(25));
        assert_eq!(percent(300, Some(200)), Some(100));
        assert_eq!(percent(5, None), None);
        assert_eq!(percent(5, Some(0)), None);
    }
}

//! Trust tiers (SPEC 16.10). From 1.0 every pack must carry a valid Ed25519 signature and
//! policy cannot switch that off. Pilot (0.x) builds may rely on the hash allow-list.
use crate::pack::VerifyOptions;
use crate::resolver::manifest_hash;
use crate::sshsig::{self, SshSigVerifier, TrustedKey};
use crate::store::{Installed, StoreKind};

pub struct Trust {
    pub app_version: String,
    /// Policy `RequireSignedModels`. Ignored (always on) from 1.0.
    pub policy_require_signed: bool,
    /// `ModelPublicKeys` from policy plus the keys compiled into the app (fallback root).
    pub keys: Vec<TrustedKey>,
    /// `AllowedModelHashes`: an extra restriction when non-empty.
    pub allowed_hashes: Vec<String>,
}

fn major(v: &str) -> u32 {
    v.split('.')
        .next()
        .and_then(|m| m.parse().ok())
        .unwrap_or(0)
}

impl Trust {
    /// True from 1.0 on, no matter what policy says.
    pub fn require_signed(&self) -> bool {
        major(&self.app_version) >= 1 || self.policy_require_signed
    }

    pub fn verifier(&self) -> SshSigVerifier {
        SshSigVerifier {
            trusted: self.keys.clone(),
        }
    }

    /// Options for `verify_pack` / `import_pack`.
    pub fn verify_options<'a>(
        &'a self,
        engines: &'a [&'a str],
        verifier: &'a SshSigVerifier,
    ) -> VerifyOptions<'a> {
        VerifyOptions {
            supported_engines: engines,
            app_version: &self.app_version,
            allowed_hashes: &self.allowed_hashes,
            require_signature: self.require_signed(),
            verifier: Some(verifier),
        }
    }

    /// Re-check an installed model at load time (the resolver's `validate` callback uses this),
    /// so a model dropped into a store by hand is held to the same rules. The bundled default
    /// ships inside the signed installer and stays trusted.
    pub fn check_installed(&self, inst: &Installed) -> Result<(), String> {
        if inst.store == StoreKind::Bundled {
            return Ok(());
        }
        if !self.allowed_hashes.is_empty() {
            let h = manifest_hash(inst).ok_or("model.json unreadable")?;
            if !self
                .allowed_hashes
                .iter()
                .any(|a| a.eq_ignore_ascii_case(&h))
            {
                return Err("Unverified: not on the allow-list".into());
            }
        }
        if self.require_signed() {
            let json = std::fs::read(inst.dir.join("model.json"))
                .map_err(|_| "model.json unreadable".to_string())?;
            let sig = std::fs::read(inst.dir.join("model.sig"))
                .map_err(|_| "model is not signed".to_string())?;
            sshsig::verify(&json, &sig, &self.keys)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(ver: &str, policy: bool) -> Trust {
        Trust {
            app_version: ver.into(),
            policy_require_signed: policy,
            keys: vec![],
            allowed_hashes: vec![],
        }
    }

    #[test]
    fn signing_is_always_required_from_1_0() {
        assert!(!t("0.9.0", false).require_signed());
        assert!(t("0.9.0", true).require_signed());
        assert!(
            t("1.0.0", false).require_signed(),
            "policy cannot turn it off"
        );
        assert!(t("2.3.1", false).require_signed());
    }
}

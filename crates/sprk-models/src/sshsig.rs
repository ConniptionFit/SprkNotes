//! OpenSSH signature (SSHSIG) verification for Ed25519 (SPEC 16.10, PROTOCOL.sshsig).
//! Verify only: private keys never touch this code. Signing is done by `ssh-keygen -Y sign`
//! against the 1Password SSH agent, so the key stays in 1Password.
use crate::pack::SignatureVerifier;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256, Sha512};

pub const NAMESPACE: &str = "sprknotes-model";
/// Namespace for app update manifests, so a model signature can never pass as an update signature.
pub const UPDATE_NAMESPACE: &str = "sprknotes-update";
const BEGIN: &str = "-----BEGIN SSH SIGNATURE-----";
const END: &str = "-----END SSH SIGNATURE-----";
const MAGIC: &[u8] = b"SSHSIG";

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }
    fn u32(&mut self) -> Option<u32> {
        let b = self.take(4)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn string(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }
}

fn put_string(out: &mut Vec<u8>, s: &[u8]) {
    out.extend_from_slice(&(s.len() as u32).to_be_bytes());
    out.extend_from_slice(s);
}

/// Wire form of an Ed25519 public key, as embedded in SSHSIG and `.pub` files.
pub fn ed25519_wire(key: &[u8; 32]) -> Vec<u8> {
    let mut b = Vec::new();
    put_string(&mut b, b"ssh-ed25519");
    put_string(&mut b, key);
    b
}

/// A trusted public key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedKey(pub [u8; 32]);

/// SHA-256 fingerprint in the OpenSSH style (`SHA256:` + unpadded base64), for logs.
pub fn fingerprint(key: &TrustedKey) -> String {
    let h = Sha256::digest(ed25519_wire(&key.0));
    format!("SHA256:{}", STANDARD.encode(h).trim_end_matches('='))
}

/// Parse `allowed_signers` style lines (`principal [options] ssh-ed25519 AAAA... [comment]`)
/// or plain `.pub` lines. Lines for other key types and comments are ignored.
pub fn parse_keys(text: &str) -> Vec<TrustedKey> {
    let mut keys = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        let Some(pos) = toks.iter().position(|t| *t == "ssh-ed25519") else {
            continue;
        };
        let Some(b64) = toks.get(pos + 1) else {
            continue;
        };
        let Ok(blob) = STANDARD.decode(b64) else {
            continue;
        };
        let mut r = Reader(&blob);
        if r.string() != Some(b"ssh-ed25519") {
            continue;
        }
        if let Some(k) = r.string().and_then(|k| <[u8; 32]>::try_from(k).ok()) {
            keys.push(TrustedKey(k));
        }
    }
    keys
}

fn dearmor(sig: &[u8]) -> Result<Vec<u8>, String> {
    let text = std::str::from_utf8(sig).map_err(|_| "signature is not text".to_string())?;
    let body: String = text
        .lines()
        .map(str::trim)
        .skip_while(|l| *l != BEGIN)
        .skip(1)
        .take_while(|l| *l != END)
        .collect();
    if body.is_empty() {
        return Err("no SSH signature block".into());
    }
    STANDARD
        .decode(body)
        .map_err(|_| "signature is not valid base64".to_string())
}

/// Result of a successful verification: which key signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    pub key: TrustedKey,
}

/// Verify an armored SSHSIG over `message` against the trusted keys. The namespace must be
/// `sprknotes-model` so a signature made for another purpose cannot be replayed.
pub fn verify(message: &[u8], armored: &[u8], trusted: &[TrustedKey]) -> Result<Verified, String> {
    verify_in(NAMESPACE, message, armored, trusted)
}

/// Like [`verify`] for an explicit namespace (`UPDATE_NAMESPACE` for update manifests).
pub fn verify_in(
    expected_namespace: &str,
    message: &[u8],
    armored: &[u8],
    trusted: &[TrustedKey],
) -> Result<Verified, String> {
    let blob = dearmor(armored)?;
    let mut r = Reader(&blob);
    if r.take(6) != Some(MAGIC) {
        return Err("not an SSHSIG".into());
    }
    if r.u32() != Some(1) {
        return Err("unsupported SSHSIG version".into());
    }
    let pk_blob = r.string().ok_or("truncated signature")?;
    let namespace = r.string().ok_or("truncated signature")?;
    let reserved = r.string().ok_or("truncated signature")?;
    let hash_alg = r.string().ok_or("truncated signature")?;
    let sig_blob = r.string().ok_or("truncated signature")?;
    if namespace != expected_namespace.as_bytes() {
        return Err("wrong signature namespace".into());
    }
    let digest: Vec<u8> = match hash_alg {
        b"sha512" => Sha512::digest(message).to_vec(),
        b"sha256" => Sha256::digest(message).to_vec(),
        _ => return Err("unsupported hash algorithm".into()),
    };
    let mut pk = Reader(pk_blob);
    if pk.string() != Some(b"ssh-ed25519") {
        return Err("only Ed25519 keys are accepted".into());
    }
    let key_bytes: [u8; 32] = pk
        .string()
        .and_then(|k| k.try_into().ok())
        .ok_or("bad public key")?;
    let key = TrustedKey(key_bytes);
    if !trusted.contains(&key) {
        return Err("signed by a key that is not trusted".into());
    }
    let mut sr = Reader(sig_blob);
    if sr.string() != Some(b"ssh-ed25519") {
        return Err("signature is not Ed25519".into());
    }
    let sig_bytes: [u8; 64] = sr
        .string()
        .and_then(|s| s.try_into().ok())
        .ok_or("bad signature length")?;
    let mut signed = Vec::new();
    signed.extend_from_slice(MAGIC);
    put_string(&mut signed, namespace);
    put_string(&mut signed, reserved);
    put_string(&mut signed, hash_alg);
    put_string(&mut signed, &digest);
    let vk = VerifyingKey::from_bytes(&key_bytes).map_err(|_| "bad public key".to_string())?;
    vk.verify_strict(&signed, &Signature::from_bytes(&sig_bytes))
        .map_err(|_| "signature does not match model.json".to_string())?;
    Ok(Verified { key })
}

/// `SignatureVerifier` backed by a set of trusted keys (policy `ModelPublicKeys` plus the
/// keys compiled into the app as a fallback trust root).
pub struct SshSigVerifier {
    pub trusted: Vec<TrustedKey>,
}

impl SignatureVerifier for SshSigVerifier {
    fn verify(&self, model_json: &[u8], signature: &[u8]) -> Result<(), String> {
        verify(model_json, signature, &self.trusted).map(|_| ())
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod testing {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    /// Builds an SSHSIG exactly as `ssh-keygen -Y sign` does. Test support only: the shipped
    /// app and sprkctl are built without this feature and never sign in-process.
    pub fn sign(sk: &SigningKey, namespace: &str, message: &[u8]) -> Vec<u8> {
        let digest = Sha512::digest(message);
        let mut signed = Vec::new();
        signed.extend_from_slice(MAGIC);
        put_string(&mut signed, namespace.as_bytes());
        put_string(&mut signed, b"");
        put_string(&mut signed, b"sha512");
        put_string(&mut signed, &digest);
        let sig = sk.sign(&signed);
        let mut sig_blob = Vec::new();
        put_string(&mut sig_blob, b"ssh-ed25519");
        put_string(&mut sig_blob, &sig.to_bytes());
        let mut blob = Vec::new();
        blob.extend_from_slice(MAGIC);
        blob.extend_from_slice(&1u32.to_be_bytes());
        put_string(&mut blob, &ed25519_wire(&sk.verifying_key().to_bytes()));
        put_string(&mut blob, namespace.as_bytes());
        put_string(&mut blob, b"");
        put_string(&mut blob, b"sha512");
        put_string(&mut blob, &sig_blob);
        let b64 = STANDARD.encode(blob);
        let mut out = String::from(BEGIN);
        out.push('\n');
        for chunk in b64.as_bytes().chunks(70) {
            out.push_str(std::str::from_utf8(chunk).unwrap_or(""));
            out.push('\n');
        }
        out.push_str(END);
        out.push('\n');
        out.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::sign;
    use super::*;
    use ed25519_dalek::SigningKey;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn trusted(sk: &SigningKey) -> Vec<TrustedKey> {
        vec![TrustedKey(sk.verifying_key().to_bytes())]
    }

    #[test]
    fn valid_signature_verifies() {
        let sk = key(7);
        let sig = sign(&sk, NAMESPACE, b"{\"id\":\"m\"}");
        let v = verify(b"{\"id\":\"m\"}", &sig, &trusted(&sk)).unwrap();
        assert_eq!(v.key, trusted(&sk)[0]);
        assert!(fingerprint(&v.key).starts_with("SHA256:"));
    }

    #[test]
    fn tampered_manifest_fails() {
        let sk = key(7);
        let sig = sign(&sk, NAMESPACE, b"original");
        assert!(verify(b"changed", &sig, &trusted(&sk))
            .unwrap_err()
            .contains("does not match"));
    }

    #[test]
    fn untrusted_key_fails() {
        let (a, b) = (key(1), key(2));
        let sig = sign(&a, NAMESPACE, b"m");
        assert!(verify(b"m", &sig, &trusted(&b))
            .unwrap_err()
            .contains("not trusted"));
        assert!(verify(b"m", &sig, &[]).is_err());
    }

    #[test]
    fn wrong_namespace_is_not_replayable() {
        let sk = key(3);
        let sig = sign(&sk, "git", b"m");
        assert!(verify(b"m", &sig, &trusted(&sk))
            .unwrap_err()
            .contains("namespace"));
    }

    #[test]
    fn two_keys_rotation() {
        let (old, new) = (key(4), key(5));
        let both = vec![trusted(&old)[0].clone(), trusted(&new)[0].clone()];
        assert!(verify(b"m", &sign(&old, NAMESPACE, b"m"), &both).is_ok());
        assert!(verify(b"m", &sign(&new, NAMESPACE, b"m"), &both).is_ok());
        // old key removed by policy: packs signed only by it stop verifying
        assert!(verify(b"m", &sign(&old, NAMESPACE, b"m"), &trusted(&new)).is_err());
    }

    #[test]
    fn garbage_is_rejected_not_panicking() {
        let sk = key(6);
        for bad in [
            &b""[..],
            b"hello",
            b"-----BEGIN SSH SIGNATURE-----\n!!!\n-----END SSH SIGNATURE-----\n",
        ] {
            assert!(verify(b"m", bad, &trusted(&sk)).is_err());
        }
        let mut sig = sign(&sk, NAMESPACE, b"m");
        sig.truncate(sig.len() / 2);
        assert!(verify(b"m", &sig, &trusted(&sk)).is_err());
    }

    #[test]
    fn parses_allowed_signers_and_pub_lines() {
        let sk = key(9);
        let b64 = STANDARD.encode(ed25519_wire(&sk.verifying_key().to_bytes()));
        let text = format!(
            "# comment\nrelease@example namespaces=\"sprknotes-model\" ssh-ed25519 {b64}\nssh-ed25519 {b64} laptop\nssh-rsa AAAA x\n"
        );
        let keys = parse_keys(&text);
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0], trusted(&sk)[0]);
    }

    #[test]
    fn verifier_trait_plugs_into_pack_verification() {
        let sk = key(8);
        let v = SshSigVerifier {
            trusted: trusted(&sk),
        };
        assert!(v.verify(b"m", &sign(&sk, NAMESPACE, b"m")).is_ok());
        assert!(v.verify(b"x", &sign(&sk, NAMESPACE, b"m")).is_err());
    }
}

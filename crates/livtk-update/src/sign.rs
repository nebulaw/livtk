//! Signed manifests. A manifest holds every artifact's SHA-256, so its one signature covers the
//! build: a [minisign] signature (Ed25519, over a BLAKE2b prehash) in a file beside it,
//! `<manifest>.minisig`. An app trusts the public keys built into it ([`Keys`]); a manifest no
//! trusted key signed is refused, never offered.
//!
//! [minisign]: https://jedisct1.github.io/minisign/

use minisign_verify::{PublicKey, Signature};

use crate::{Error, Result};

/// The name of the signature beside the manifest named `manifest`.
pub fn signature_name(manifest: &str) -> String {
    format!("{manifest}.minisig")
}

/// The public keys an app trusts. With none, nothing is trusted.
#[derive(Clone, Default)]
pub struct Keys(Vec<PublicKey>);

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Keys({} trusted)", self.0.len())
    }
}

impl Keys {
    /// Trust each of `keys`: minisign public keys in base64 (`RW…`, the second line of a `.pub`
    /// file). An empty one is skipped, so a key slot not filled yet trusts nothing.
    pub fn new<'a>(keys: impl IntoIterator<Item = &'a str>) -> Result<Keys> {
        let mut out = Vec::new();
        for k in keys.into_iter().map(str::trim).filter(|k| !k.is_empty()) {
            out.push(
                PublicKey::from_base64(k)
                    .map_err(|e| Error::Signature(format!("a trusted key is not one: {e}")))?,
            );
        }
        Ok(Keys(out))
    }

    /// Whether any key is trusted at all.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Check that one of the trusted keys signed `manifest` with `signature` (the text of a
    /// `.minisig` file). Only prehashed signatures are taken, as minisign makes by default.
    pub fn verify(&self, manifest: &[u8], signature: &str) -> Result<()> {
        if self.0.is_empty() {
            return Err(Error::Signature("no key is trusted".into()));
        }
        let sig = Signature::decode(signature)
            .map_err(|e| Error::Signature(format!("not a signature: {e}")))?;
        if self
            .0
            .iter()
            .any(|k| k.verify(manifest, &sig, false).is_ok())
        {
            Ok(())
        } else {
            Err(Error::Signature(
                "no trusted key signed it, or it was changed after".into(),
            ))
        }
    }
}

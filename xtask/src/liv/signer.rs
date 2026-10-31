//! The keys Liv's builds are signed with (minisign):
//! - this machine's own key, made once in `.liv-dev/dev.key` at the repository's root (never
//!   committed), for the builds published to the local channel;
//! - the release key, for releases on GitHub: its public half is `crates/liv/release.pub`
//!   (committed, built into Liv), its secret half is given to the release workflow only, as the
//!   repository's `LIV_SIGNING_KEY` secret.

use std::path::Path;

use anyhow::{Context, Result, anyhow};

/// A key to sign with, and the public key that checks it.
pub struct Signer {
    pub public: String,
    secret: minisign::SecretKey,
}

impl Signer {
    /// This machine's own key: made on first use.
    pub fn dev(root: &Path) -> Result<Signer> {
        let dir = root.join(".liv-dev");
        let (public, secret) = (dir.join("dev.pub"), dir.join("dev.key"));
        if !secret.exists() {
            make(
                &public,
                &secret,
                "Liv's local signing key: this machine only",
            )?;
            println!("made Liv's local signing key in {}", dir.display());
        }
        Signer::read(&public, &secret)
    }

    /// The release key, its secret half in the variable `var` (the release workflow's secret),
    /// its public half in `public`: they must belong together.
    pub fn from_env(var: &str, public: &Path) -> Result<Signer> {
        let text = std::env::var(var).with_context(|| format!("the secret key in ${var}"))?;
        let signer = Signer {
            public: std::fs::read_to_string(public)?.trim().to_owned(),
            secret: secret_of(&text).with_context(|| format!("${var}"))?,
        };
        signer.check()?;
        Ok(signer)
    }

    fn read(public: &Path, secret: &Path) -> Result<Signer> {
        let signer = Signer {
            public: std::fs::read_to_string(public)?.trim().to_owned(),
            secret: secret_of(&std::fs::read_to_string(secret)?)
                .with_context(|| secret.display().to_string())?,
        };
        signer.check()?;
        Ok(signer)
    }

    /// The two halves belong together: what the secret signs, the public key verifies.
    fn check(&self) -> Result<()> {
        let keys = livtk_update::sign::Keys::new([self.public.as_str()])?;
        keys.verify(b"check", &self.sign(b"check")?).map_err(|_| {
            anyhow!(
                "the secret key doesn't belong to the public key {}",
                self.public
            )
        })
    }

    /// A signature of `bytes`, as a `.minisig` file holds it.
    pub fn sign(&self, bytes: &[u8]) -> Result<String> {
        Ok(
            minisign::sign(None, &self.secret, std::io::Cursor::new(bytes), None, None)
                .map_err(|e| anyhow!("signing: {e}"))?
                .into_string(),
        )
    }
}

fn secret_of(text: &str) -> Result<minisign::SecretKey> {
    minisign::SecretKeyBox::from_string(text)
        .and_then(|b| b.into_unencrypted_secret_key())
        .map_err(|e| anyhow!("not a minisign secret key: {e}"))
}

/// Make a key pair: its public half at `public` (base64, as Liv reads it), its secret half at
/// `secret`, readable by its owner only from the moment it exists, never overwritten.
pub fn make(public: &Path, secret: &Path, comment: &str) -> Result<()> {
    if let Some(dir) = secret.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let kp = minisign::KeyPair::generate_unencrypted_keypair()
        .map_err(|e| anyhow!("making a key: {e}"))?;
    let boxed = kp.sk.to_box(Some(comment)).map_err(|e| anyhow!("{e}"))?;
    let mut file = std::fs::OpenOptions::new();
    file.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut file, 0o600);
    std::io::Write::write_all(
        &mut file
            .open(secret)
            .with_context(|| format!("{}: never overwritten", secret.display()))?,
        boxed.to_string().as_bytes(),
    )?;
    std::fs::write(public, format!("{}\n", kp.pk.to_base64()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_made_key_signs_what_its_public_half_checks_and_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let (public, secret) = (dir.path().join("k.pub"), dir.path().join("k.key"));
        make(&public, &secret, "test").unwrap();
        let s = Signer::read(&public, &secret).unwrap();
        let keys = livtk_update::sign::Keys::new([s.public.as_str()]).unwrap();
        assert!(keys.verify(b"body", &s.sign(b"body").unwrap()).is_ok());
        assert!(
            make(&public, &secret, "again").is_err(),
            "an existing key is never replaced"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&secret).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "the secret is its owner's only");
        }
    }

    #[test]
    fn a_secret_that_isn_t_the_public_key_s_half_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (a_pub, a_key) = (dir.path().join("a.pub"), dir.path().join("a.key"));
        let (b_pub, b_key) = (dir.path().join("b.pub"), dir.path().join("b.key"));
        make(&a_pub, &a_key, "a").unwrap();
        make(&b_pub, &b_key, "b").unwrap();
        assert!(Signer::read(&a_pub, &b_key).is_err());
    }
}

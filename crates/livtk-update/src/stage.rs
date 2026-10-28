//! Staging an update: its artifact checked against the manifest (size, then SHA-256), unpacked
//! beside the others, and the unpacked app checked to be the app and build the manifest names.
//! Nothing is installed here; a staged build waits for "Restart to Update".

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use crate::{Artifact, Error, Found, Manifest, Result};

/// `found`'s artifact as a file: staging needs it here (`fetch::download` brings a URL's).
fn file_of(found: &Found) -> Result<&Path> {
    match &found.artifact {
        Artifact::File(p) => Ok(p),
        Artifact::Url(u) => Err(Error::Failed(format!("{u}: download it before staging"))),
    }
}

/// A build ready to install: its manifest and the unpacked app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Staged {
    pub manifest: Manifest,
    pub app: PathBuf,
}

/// The SHA-256 of a file, lowercase hex.
pub fn sha256_of(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Check `found`'s artifact against its manifest: its size, then its hash.
pub fn verify(found: &Found) -> Result<()> {
    let m = &found.manifest;
    let file = file_of(found)?;
    let size = std::fs::metadata(file)?.len();
    if size != m.size {
        return Err(Error::Mismatch(format!(
            "{} bytes, the manifest says {}",
            size, m.size
        )));
    }
    let hash = sha256_of(file)?;
    if !hash.eq_ignore_ascii_case(&m.sha256) {
        return Err(Error::Mismatch(format!(
            "its SHA-256 is {hash}, the manifest says {}",
            m.sha256
        )));
    }
    Ok(())
}

/// Verify `found`, unpack it under `staging`, and check the app inside is the one the manifest
/// names. A build already staged is staged again from scratch.
pub fn stage(found: &Found, staging: &Path) -> Result<Staged> {
    verify(found)?;
    let m = &found.manifest;
    let dir = staging.join(format!("build-{}", m.build));
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    unpack(file_of(found)?, &dir)?;
    let app = find_app(&dir)?;
    let (id, build) = identity(&app)?;
    if id != m.app || build != m.build {
        return Err(Error::Mismatch(format!(
            "the app inside is {id} build {build}, the manifest says {} build {}",
            m.app, m.build
        )));
    }
    Ok(Staged {
        manifest: m.clone(),
        app,
    })
}

/// Unpack a zipped app with `ditto`, as macOS packs it (resource forks, signatures kept).
fn unpack(artifact: &Path, into: &Path) -> Result<()> {
    if !cfg!(target_os = "macos") {
        return Err(Error::Unsupported("unpacking is written for macOS only"));
    }
    run(Command::new("ditto")
        .arg("-x")
        .arg("-k")
        .arg(artifact)
        .arg(into))
}

/// The one app bundle unpacked into `dir`.
fn find_app(dir: &Path) -> Result<PathBuf> {
    std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .ok_or_else(|| Error::Mismatch("no app inside".into()))
}

/// An app bundle's identifier and build number, from its Info.plist.
pub fn identity(app: &Path) -> Result<(String, u64)> {
    let plist = app.join("Contents/Info.plist");
    let key = |k: &str| -> Result<String> {
        let out = Command::new("plutil")
            .args(["-extract", k, "raw", "-o", "-"])
            .arg(&plist)
            .output()?;
        if !out.status.success() {
            return Err(Error::Mismatch(format!("its Info.plist has no {k}")));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    };
    let id = key("CFBundleIdentifier")?;
    let build = key("CFBundleVersion")?
        .parse()
        .map_err(|_| Error::Mismatch("its build number is not a number".into()))?;
    Ok((id, build))
}

fn run(cmd: &mut Command) -> Result<()> {
    let out = cmd.output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(Error::Failed(
            String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        ))
    }
}

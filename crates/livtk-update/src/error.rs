use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{path}: not a manifest: {why}")]
    Manifest { path: PathBuf, why: String },
    #[error("the update is not signed by a key this app trusts: {0}")]
    Signature(String),
    #[error("the update does not match its manifest: {0}")]
    Mismatch(String),
    #[error("updates cannot be installed on this system yet: {0}")]
    Unsupported(&'static str),
    #[error("{0}")]
    Failed(String),
}

pub type Result<T> = std::result::Result<T, Error>;

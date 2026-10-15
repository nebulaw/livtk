use std::path::PathBuf;

/// Every failure the core can report. Messages say what is wrong and, where possible, how to fix it.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no .livtk directory here or in any parent; run `livtk init` in the project root")]
    NotFound,
    #[error("{} already holds a livtk project", .0.display())]
    AlreadyInitialised(PathBuf),
    #[error("no node `{0}`; `livtk list --all` shows every key")]
    UnknownNode(String),
    /// The input is malformed.
    #[error("{0}")]
    Invalid(String),
    /// The input is well formed, but a rule of the system refuses it.
    #[error("{0}")]
    Refused(String),
    #[error("config {}: {msg}", path.display())]
    Config { path: PathBuf, msg: String },
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

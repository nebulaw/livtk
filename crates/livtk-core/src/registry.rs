//! The projects this machine knows: `projects.toml` in livtk's config directory. `livtk init`
//! adds to it; the app lists it. A convenience, never the truth: a project is its `.livtk/`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    projects: Vec<PathBuf>,
}

/// livtk's config directory: `$LIVTK_CONFIG_DIR`, else `$XDG_CONFIG_HOME/livtk`, else
/// `~/.config/livtk`.
pub fn config_dir() -> Option<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    env("LIVTK_CONFIG_DIR")
        .or_else(|| env("XDG_CONFIG_HOME").map(|d| d.join("livtk")))
        .or_else(|| env("HOME").map(|d| d.join(".config/livtk")))
}

/// The registered project directories, in the order they were added. Ones that no longer hold a
/// livtk project are left out, but kept in the file in case they come back (a remounted disk).
pub fn projects(config: &Path) -> Result<Vec<PathBuf>> {
    Ok(read(config)?
        .projects
        .into_iter()
        .filter(|p| p.join(crate::DIR_NAME).join("config.toml").is_file())
        .collect())
}

/// Remember a project directory. Idempotent.
pub fn add(config: &Path, project: &Path) -> Result<()> {
    let project = project.canonicalize()?;
    let mut file = read(config)?;
    if !file.projects.contains(&project) {
        file.projects.push(project);
        write(config, &file)?;
    }
    Ok(())
}

/// Forget a project directory. Its `.livtk/` is untouched.
pub fn remove(config: &Path, project: &Path) -> Result<()> {
    let project = project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf());
    let mut file = read(config)?;
    let before = file.projects.len();
    file.projects.retain(|p| p != &project);
    if file.projects.len() != before {
        write(config, &file)?;
    }
    Ok(())
}

fn path(config: &Path) -> PathBuf {
    config.join("projects.toml")
}

fn read(config: &Path) -> Result<File> {
    match fs::read_to_string(path(config)) {
        Ok(text) => toml::from_str(&text).map_err(|e| Error::Config {
            path: path(config),
            msg: e.to_string(),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(File::default()),
        Err(e) => Err(e.into()),
    }
}

/// Write through a temporary file and a rename, so a crash never leaves half a list.
fn write(config: &Path, file: &File) -> Result<()> {
    fs::create_dir_all(config)?;
    let text = toml::to_string(file).map_err(|e| Error::Config {
        path: path(config),
        msg: e.to_string(),
    })?;
    let tmp = config.join(format!("projects.toml.{}", std::process::id()));
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path(config))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(root: &Path, name: &str) -> PathBuf {
        let p = root.join(name);
        fs::create_dir_all(p.join(".livtk")).unwrap();
        fs::write(
            p.join(".livtk/config.toml"),
            "version = 1\nprefix = \"X\"\n",
        )
        .unwrap();
        p.canonicalize().unwrap()
    }

    #[test]
    fn adds_once_lists_live_projects_and_removes() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config");
        assert!(
            projects(&config).unwrap().is_empty(),
            "no file yet is an empty list"
        );
        let (a, b) = (project(root.path(), "a"), project(root.path(), "b"));
        add(&config, &a).unwrap();
        add(&config, &b).unwrap();
        add(&config, &a).unwrap();
        assert_eq!(projects(&config).unwrap(), [a.clone(), b.clone()]);

        fs::remove_dir_all(b.join(".livtk")).unwrap();
        assert_eq!(
            projects(&config).unwrap(),
            std::slice::from_ref(&a),
            "a vanished project is hidden"
        );
        fs::create_dir_all(b.join(".livtk")).unwrap();
        fs::write(
            b.join(".livtk/config.toml"),
            "version = 1\nprefix = \"X\"\n",
        )
        .unwrap();
        assert_eq!(
            projects(&config).unwrap(),
            [a.clone(), b.clone()],
            "and back when it returns"
        );

        remove(&config, &a).unwrap();
        assert_eq!(projects(&config).unwrap(), [b]);
    }

    #[test]
    fn a_broken_file_is_an_error_that_names_it() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("projects.toml"), "projects = 3").unwrap();
        let err = projects(root.path()).unwrap_err().to_string();
        assert!(err.contains("projects.toml"), "{err}");
    }
}

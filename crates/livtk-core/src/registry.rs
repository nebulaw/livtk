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
/// Scratch projects (`scratch`) are left out and dropped from the file: they were never yours.
pub fn projects(config: &Path) -> Result<Vec<PathBuf>> {
    let mut file = read(config)?;
    let before = file.projects.len();
    file.projects.retain(|p| !scratch(config, p));
    if file.projects.len() != before {
        write(config, &file)?;
    }
    Ok(file
        .projects
        .into_iter()
        .filter(|p| p.join(crate::DIR_NAME).join("config.toml").is_file())
        .collect())
}

/// The system's folders for temporary files, as their real paths (macOS links `/tmp` and
/// `/var` into `/private`).
fn temp_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = [
        std::env::temp_dir(),
        PathBuf::from("/tmp"),
        PathBuf::from("/var/folders"),
    ]
    .into_iter()
    .flat_map(|p| {
        let real = p.canonicalize().ok();
        [Some(p), real]
    })
    .flatten()
    .collect();
    roots.dedup();
    roots
}

fn under_temp(path: &Path) -> bool {
    let real = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    temp_roots()
        .iter()
        .any(|t| path.starts_with(t) || real.starts_with(t))
}

/// A scratch project: one in a temporary folder (an agent's or a test's), listed in a config
/// that is not. It works when opened, but is never remembered in your list, so the app never
/// opens it again (TAS-124). A config that is itself temporary (a test's) keeps its projects.
pub fn scratch(config: &Path, project: &Path) -> bool {
    under_temp(project) && !under_temp(config)
}

/// Remember a project directory. Idempotent.
pub fn add(config: &Path, project: &Path) -> Result<()> {
    let project = project.canonicalize()?;
    if scratch(config, &project) {
        return Ok(());
    }
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
    fn a_scratch_project_is_never_remembered_and_is_dropped_from_the_list() {
        let scratch_dir = tempfile::tempdir().unwrap();
        let p = project(scratch_dir.path(), "scratch");
        // Your config, outside the temporary folders: here, a folder in this crate's target.
        let home = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/registry-test-config")
            .join(std::process::id().to_string());
        let _ = fs::remove_dir_all(&home);
        assert!(scratch(&home, &p));
        add(&home, &p).unwrap();
        assert!(projects(&home).unwrap().is_empty(), "never remembered");
        // A list that holds one from before is cleaned as it is read, and written back so.
        let kept = Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap();
        write(
            &home,
            &File {
                projects: vec![p.clone(), kept.clone()],
            },
        )
        .unwrap();
        let _ = projects(&home).unwrap();
        assert_eq!(
            read(&home).unwrap().projects,
            [kept],
            "dropped from the file"
        );
        // A temporary config (a test's) keeps its own scratch projects.
        let config = scratch_dir.path().join("config");
        assert!(!scratch(&config, &p));
        add(&config, &p).unwrap();
        assert_eq!(projects(&config).unwrap(), [p]);
        let _ = fs::remove_dir_all(&home);
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

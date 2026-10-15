//! `livtk embed`: the local model that ranks search by meaning. The download uses the system
//! `curl`, so livtk itself carries no network code.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use livtk_core::Workspace;
use livtk_core::search::{DEFAULT_MODEL, models_dir};
use serde_json::json;

use crate::EmbedAction;

/// The files a model2vec model consists of, smallest first.
const FILES: [&str; 3] = ["config.json", "tokenizer.json", "model.safetensors"];

pub fn run(action: &EmbedAction, cwd: &Path, json: bool) -> Result<()> {
    let project = Workspace::open(cwd).ok();
    match action {
        EmbedAction::Setup { model } => {
            let name = model
                .clone()
                .or_else(|| project.as_ref().map(|ws| ws.config().memory.model.clone()))
                .unwrap_or_else(|| DEFAULT_MODEL.to_owned());
            setup(&name)?;
            status(project, Some(&name), json)
        }
        EmbedAction::Status => status(project, None, json),
    }
}

fn setup(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    {
        bail!("`{name}` is not a model name like potion-retrieval-32M");
    }
    let dir = models_dir()
        .context("no models directory: set LIVTK_MODELS_DIR, XDG_CACHE_HOME or HOME")?
        .join(name);
    fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    for file in FILES {
        let target = dir.join(file);
        if target.is_file() {
            continue;
        }
        let part = dir.join(format!("{file}.part"));
        let url = format!("https://huggingface.co/minishlab/{name}/resolve/main/{file}");
        eprintln!("livtk: fetching {url}");
        let ok = Command::new("curl")
            .args(["-fL", "--retry", "2", "--progress-bar", "-o"])
            .arg(&part)
            .arg(&url)
            .status()
            .context("cannot run curl; install it, or place the model files by hand")?
            .success();
        if !ok {
            let _ = fs::remove_file(&part);
            bail!("download failed: {url}");
        }
        // Renamed only when complete, so an interrupted download is never mistaken for a model.
        fs::rename(&part, &target)?;
    }
    Ok(())
}

fn status(project: Option<Workspace>, name: Option<&str>, json: bool) -> Result<()> {
    let (name, path, loads) = match project {
        Some(mut ws) => ws.model_status(),
        None => {
            let name = name.unwrap_or(DEFAULT_MODEL).to_owned();
            let path = models_dir().map(|d| d.join(&name));
            let present = path
                .as_ref()
                .is_some_and(|p| p.join("model.safetensors").is_file());
            (name, path, present)
        }
    };
    let installed = path
        .as_ref()
        .is_some_and(|p| p.join("model.safetensors").is_file());
    if json {
        println!(
            "{}",
            json!({ "model": name, "path": path, "installed": installed, "loads": loads })
        );
        return Ok(());
    }
    let place = path.map(|p| p.display().to_string()).unwrap_or_default();
    match (installed, loads) {
        (true, true) => {
            println!("{name}: installed at {place}; search ranks by meaning and keywords")
        }
        (true, false) => {
            println!("{name}: files at {place} but it does not load; run `livtk embed setup` again")
        }
        (false, _) => {
            println!("{name}: not installed; search uses keywords only. Run `livtk embed setup`")
        }
    }
    Ok(())
}

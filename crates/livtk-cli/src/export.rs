//! `livtk export`: the core's markdown export, told on the command line.

use std::path::Path;

use anyhow::{Result, bail};
use livtk_core::Workspace;

pub fn run(ws: &Workspace, dir: Option<&Path>, check: bool) -> Result<()> {
    let r = livtk_core::export::export(ws, dir, &std::env::current_dir()?, check)?;
    if check {
        if r.stale == 0 {
            println!("{} is current", r.dir.display());
            return Ok(());
        }
        bail!(
            "{} is stale: {} file(s) differ from the log; run `livtk export` again",
            r.dir.display(),
            r.stale
        );
    }
    println!(
        "exported {} node(s) to {} ({} file(s) written)",
        r.nodes,
        r.dir.display(),
        r.stale
    );
    Ok(())
}

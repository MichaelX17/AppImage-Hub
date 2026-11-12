use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Command, ExitStatus};

/// Launch with no shell — arguments are passed literally, immune to injection.
pub fn launch(appimage: &Path, args: &[String]) -> Result<ExitStatus> {
    Command::new(appimage)
        .args(args)
        .status()
        .with_context(|| format!("failed to spawn {}", appimage.display()))
}

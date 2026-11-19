use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Child, Command};

/// Starts the AppImage without waiting for it to exit — arguments are passed
/// literally (no shell), immune to injection.
///
/// The caller owns the child and should reap it (for example on a background
/// thread) so a long-running application never blocks the UI or the backend.
pub fn spawn(appimage: &Path, args: &[String]) -> Result<Child> {
    Command::new(appimage)
        .args(args)
        .spawn()
        .with_context(|| format!("failed to launch {}", appimage.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn spawn_returns_without_waiting_for_child() {
        use std::os::unix::fs::PermissionsExt as _;
        use std::time::{Duration, Instant};

        let dir = std::env::temp_dir().join(format!("aim-launch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("app.appimage");
        std::fs::write(&script, "#!/bin/sh\nsleep 5\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let started = Instant::now();
        let mut child = spawn(&script, &[]).unwrap();
        let elapsed = started.elapsed();

        // The child is still sleeping; spawn must not have waited for it.
        assert!(
            elapsed < Duration::from_secs(2),
            "spawn blocked for {elapsed:?}"
        );

        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }
}

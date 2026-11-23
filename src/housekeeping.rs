//! Keeping the store in line with `config.toml`: re-read the config, apply its storage
//! settings and prune old rounds. Runs at startup, daily, and when the window gains focus
//! (ADR 0007); there is no file watcher.

use std::{path::Path, time::Duration};

use crate::{config, desk::Desk};

/// How often the store is pruned while the app runs.
const PRUNE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Reads the config at `path`, applies its storage settings to `desk` and prunes. A
/// config that cannot be read changes nothing.
pub fn run(desk: &Desk, path: &Path) -> Result<(), String> {
    let config = config::load_from(path)?;
    desk.apply_storage(config.storage());
    desk.prune(config.retention_days);
    Ok(())
}

/// Runs [`run`] now and then daily, on a thread of its own.
pub fn spawn_daily(desk: Desk, path: std::path::PathBuf) {
    std::thread::Builder::new()
        .name("housekeeping".into())
        .spawn(move || {
            loop {
                if let Err(e) = run(&desk, &path) {
                    log::warn!("not pruning: {e}");
                }
                std::thread::sleep(PRUNE_INTERVAL);
            }
        })
        .expect("housekeeping thread starts");
}

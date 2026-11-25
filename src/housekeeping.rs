//! Pruning old rounds: re-read the retention from `config.toml` and prune. Runs at startup
//! and daily (ADR 0007); there is no file watcher. Storage settings apply at startup only.

use std::{path::Path, time::Duration};

use crate::{config, desk::Desk};

/// How often the store is pruned while the app runs.
const PRUNE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Reads the config at `path` and prunes with its retention. A config that cannot be
/// read prunes nothing.
pub fn run(desk: &Desk, path: &Path) -> Result<(), String> {
    let config = config::load_from(path)?;
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

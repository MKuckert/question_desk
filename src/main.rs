//! The Question Desk app: MCP server on a background runtime, window on the main thread.

use question_desk::{config, desk::Desk, housekeeping, server::ServerControl, ui};

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,rmcp=warn"))
        .init();

    let config = config::load();
    // An unreadable config still stores rounds where they were by default.
    let storage = config.clone().unwrap_or_default().storage();
    if storage.persistence && storage.data_dir.is_none() {
        log::warn!("no home directory: question rounds are kept in memory only");
    }
    let desk = Desk::new(storage);
    // Prunes now and daily, re-reading the config each time.
    if let Some(path) = config::path() {
        housekeeping::spawn_daily(desk.clone(), path);
    }
    let server = ServerControl::new(desk.clone());
    ui::run(desk, server, config)
}

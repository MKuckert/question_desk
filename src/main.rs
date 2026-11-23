//! The Question Desk app: MCP server on a background runtime, window on the main thread.

use question_desk::{config, desk::Desk, server::ServerControl, store, ui};

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,rmcp=warn"))
        .init();

    let desk = match store::default_data_dir() {
        Some(dir) => Desk::with_store(store::Store::new(dir)),
        None => {
            log::warn!("no home directory: question rounds are kept in memory only");
            Desk::default()
        }
    };
    let server = ServerControl::new(desk.clone());
    ui::run(desk, server, config::load())
}

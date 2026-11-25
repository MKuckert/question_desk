//! The Question Desk app: MCP server on a background runtime, window on the main thread.
// No console window next to the GUI on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use question_desk::{config, desk::Desk, server::ServerControl, ui};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,rmcp=warn"))
        .init();

    let desk = Desk::default();
    let server = ServerControl::new(desk.clone());
    ui::run(desk, server, config::load())
}

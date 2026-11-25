//! The Question Desk app: MCP server on a background runtime, window on the main thread.

use question_desk::{config, desk::Desk, server::ServerControl, ui};

fn main() -> Result<(), slint::PlatformError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,rmcp=warn"))
        .init();

    let desk = Desk::default();
    let server = ServerControl::new(desk.clone());
    ui::run(desk, server, config::load())
}

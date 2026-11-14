//! The Question Desk app: MCP server on a background runtime, window on the main thread.

use question_desk::{
    config,
    desk::Desk,
    server::{self, ServerSettings},
    ui,
};

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,rmcp=warn"))
        .init();

    let desk = Desk::default();
    let status = match start_server(desk.clone()) {
        Ok(url) => {
            log::info!("listening on {url}");
            format!("Agents connect to {url}")
        }
        Err(e) => {
            log::error!("{e}");
            format!("The MCP server did not start: {e}")
        }
    };
    ui::run(desk, status)
}

/// Starts the MCP server on its own thread and returns its URL once it listens.
fn start_server(desk: Desk) -> Result<String, String> {
    let config = config::load()?;
    let settings = ServerSettings {
        port: config.port,
        bearer_token: config.bearer_token,
        ..ServerSettings::default()
    };
    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("tokio runtime: {e}"))?;
    let server = runtime
        .block_on(server::start(desk, settings))
        .map_err(|e| format!("cannot listen on 127.0.0.1:{}: {e}", config.port))?;
    let url = format!("http://{}/mcp", server.addr());
    std::thread::Builder::new()
        .name("mcp-server".into())
        .spawn(move || runtime.block_on(server.wait()))
        .map_err(|e| format!("server thread: {e}"))?;
    Ok(url)
}

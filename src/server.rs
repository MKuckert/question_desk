//! The MCP server: stateless Streamable HTTP on `127.0.0.1` (ADR 0001, 0002), with the
//! `ask_user_question` and `wait_for_answers` tools (ADR 0003, 0004).

use std::{net::SocketAddr, sync::Arc, time::Duration};

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::common::{schema_for_output, schema_for_type},
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    },
    service::RequestContext,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde_json::Value;
use tokio::task::JoinHandle;

use crate::{
    desk::Desk,
    format::{self, RoundResult},
    schema::{self, AskInput, Output},
};

#[derive(Debug, Clone)]
pub struct ServerSettings {
    /// `0` picks a free port.
    pub port: u16,
    pub bearer_token: Option<String>,
    /// Interval between Heartbeat progress notifications.
    pub heartbeat: Duration,
    /// How long `wait_for_answers` blocks before returning `pending`.
    pub pickup_wait: Duration,
}

pub const DEFAULT_PORT: u16 = 7457;

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            bearer_token: None,
            heartbeat: Duration::from_secs(15),
            pickup_wait: Duration::from_secs(50),
        }
    }
}

pub struct RunningServer {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl RunningServer {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Serves until the process ends or the server fails.
    pub async fn wait(mut self) {
        let _ = (&mut self.task).await;
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Binds `127.0.0.1:<port>` and serves the MCP endpoint at `/mcp` on the current runtime.
pub async fn start(desk: Desk, settings: ServerSettings) -> std::io::Result<RunningServer> {
    let settings = Arc::new(settings);
    let handler = Handler { desk, settings: settings.clone() };
    let config = StreamableHttpServerConfig::default().with_legacy_session_mode(false);
    let service: StreamableHttpService<Handler, LocalSessionManager> =
        StreamableHttpService::new(move || Ok(handler.clone()), Default::default(), config);
    let app = axum::Router::new().nest_service("/mcp", service);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", settings.port)).await?;
    let addr = listener.local_addr()?;
    let task = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            log::error!("MCP server stopped: {e}");
        }
    });
    Ok(RunningServer { addr, task })
}

#[derive(Clone)]
struct Handler {
    desk: Desk,
    settings: Arc<ServerSettings>,
}

const ASK: &str = "ask_user_question";
const WAIT: &str = "wait_for_answers";

fn tools() -> Vec<Tool> {
    let mut ask = Tool::new(ASK, ASK_DESCRIPTION, schema_for_type::<AskInput>());
    ask.output_schema = Some(schema_for_output::<Output>());
    vec![ask]
}

const ASK_DESCRIPTION: &str = "Ask the user one or more questions and wait for the answers.";

fn tool_error(message: impl Into<String>) -> CallToolResponse {
    CallToolResult::error(vec![ContentBlock::text(message.into())]).into()
}

fn finished(result: &RoundResult) -> CallToolResponse {
    let mut response = CallToolResult::success(vec![ContentBlock::text(format::text(result))]);
    response.structured_content =
        Some(serde_json::to_value(format::structured(result)).expect("output serializes"));
    response.into()
}

impl ServerHandler for Handler {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("question-desk", env!("CARGO_PKG_VERSION")))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(tools()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let args = Value::Object(request.arguments.unwrap_or_default());
        match &*request.name {
            ASK => Ok(self.ask(args, context).await),
            other => Ok(tool_error(format!("unknown tool {other:?}"))),
        }
    }
}

impl Handler {
    async fn ask(&self, args: Value, context: RequestContext<RoleServer>) -> CallToolResponse {
        let input: AskInput = match schema::parse(args) {
            Ok(input) => input,
            Err(e) => return tool_error(e),
        };
        if let Err(e) = input.validate() {
            return tool_error(e);
        }
        let client = context.client_info().map(|c| c.title.unwrap_or(c.name));
        let mut attachment = match self.desk.ask(input, client) {
            Ok(attachment) => attachment,
            Err(e) => return tool_error(e),
        };
        finished(&attachment.finished().await)
    }
}

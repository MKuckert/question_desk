//! The MCP server: stateless Streamable HTTP on `127.0.0.1` (ADR 0001, 0002), with the
//! `ask_user_question` and `wait_for_answers` tools (ADR 0003, 0004).

use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::common::{schema_for_output, schema_for_type},
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ProgressNotificationParam, ServerCapabilities, ServerConfig, Tool,
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
    desk::Attachment,
    schema::{self, AskInput, Output, WaitInput},
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
    let app = axum::Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(settings.clone(), guard));

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", settings.port)).await?;
    let addr = listener.local_addr()?;
    let task = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            log::error!("MCP server stopped: {e}");
        }
    });
    Ok(RunningServer { addr, task })
}

/// Rejects browsers (any `Origin` header, 403) and clients without the bearer token (401).
async fn guard(State(settings): State<Arc<ServerSettings>>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    if headers.contains_key(header::ORIGIN) {
        return (StatusCode::FORBIDDEN, "Origin not allowed").into_response();
    }
    if let Some(token) = &settings.bearer_token {
        let expected = format!("Bearer {token}");
        if headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) != Some(&expected) {
            return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        }
    }
    next.run(request).await
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
    let mut wait = Tool::new(WAIT, WAIT_DESCRIPTION, schema_for_type::<WaitInput>());
    wait.output_schema = Some(schema_for_output::<Output>());
    vec![ask, wait]
}

const ASK_DESCRIPTION: &str = "\
Ask the user questions in the Question Desk window and wait for the answers. Use it instead of \
the built-in AskUserQuestion; the fields are the same, without its limits.

How to ask well:
- Bundle all related decisions into one call: any number of `questions`, each with any number \
of `options` (zero options for an open question answered in free text).
- Put context, trade-offs and consequences into the question's `body` (Markdown). Keep \
`question` a single clear sentence and `header` a short chip label.
- Always set `recommended` to your suggested answer: an option label, or free text for a \
nuanced recommendation.
- Use `multiSelect` when several options can apply; give options a `preview` (code, mockup) \
when the user should compare artifacts.
- Set `title` (what the round is about) and `source` (project or working directory).

Tickets: `ticket` is required. Choose a short, unique id with a random suffix, e.g. \
`db-choice-7f3`. This call blocks until the user submits or cancels, which can take many \
minutes. If the call is interrupted or times out, do not ask again with a new ticket: call \
`wait_for_answers` with the same ticket (or repeat this exact call) to pick up the answers.

The result has the AskUserQuestion shape, plus `structuredContent` with `status` \
(`answered` | `cancelled`), `answers`, `annotations` (the user's notes) and `unanswered`. \
The user may leave questions unanswered or cancel; follow up on those instead of guessing.";

const WAIT_DESCRIPTION: &str = "\
Pick up the answers of a question round by the ticket you chose in `ask_user_question`, \
after that call was interrupted or timed out. Blocks up to about 50 seconds and returns the \
answers, or status `pending` with the round's title and questions so you can confirm it is \
yours; call it again while pending. An unknown ticket is an error.";

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
            WAIT => Ok(self.wait_for_answers(args, context).await),
            other => Ok(tool_error(format!("unknown tool {other:?}"))),
        }
    }
}

impl Handler {
    async fn ask(&self, args: Value, context: RequestContext<RoleServer>) -> CallToolResponse {
        let input = match schema::parse_ask(args) {
            Ok(input) => input,
            Err(e) => return tool_error(e),
        };
        // Only the per-request `_meta` identifies the client; in stateless mode the peer
        // info is a placeholder (ADR 0001).
        let client = context.meta.client_info().map(|c| c.title.unwrap_or(c.name));
        let mut attachment = match self.desk.ask(input, client) {
            Ok(attachment) => attachment,
            Err(e) => return tool_error(e),
        };
        match self.wait(&mut attachment, None, &context).await {
            Some(result) => finished(&result),
            None => tool_error("the call was cancelled; the round stays open for pickup"),
        }
    }

    async fn wait_for_answers(&self, args: Value, context: RequestContext<RoleServer>) -> CallToolResponse {
        let input: WaitInput = match schema::parse(args) {
            Ok(input) => input,
            Err(e) => return tool_error(e),
        };
        let Some(mut attachment) = self.desk.pickup(&input.ticket) else {
            return tool_error(format!(
                "unknown ticket {:?}: no question round was asked with it since the Question Desk started",
                input.ticket
            ));
        };
        match self.wait(&mut attachment, Some(self.settings.pickup_wait), &context).await {
            Some(result) => finished(&result),
            None => {
                let (text, output) =
                    format::pending(&input.ticket, attachment.title(), attachment.questions());
                let mut response = CallToolResult::success(vec![ContentBlock::text(text)]);
                response.structured_content =
                    Some(serde_json::to_value(output).expect("output serializes"));
                response.into()
            }
        }
    }

    /// Waits for the round's result until the call is cancelled or `deadline` passes,
    /// sending a Heartbeat meanwhile if the client gave a progress token.
    async fn wait(
        &self,
        attachment: &mut Attachment,
        deadline: Option<Duration>,
        context: &RequestContext<RoleServer>,
    ) -> Option<RoundResult> {
        let deadline = async {
            match deadline {
                Some(deadline) => tokio::time::sleep(deadline).await,
                None => std::future::pending().await,
            }
        };
        tokio::pin!(deadline);
        let progress_token = context.meta.get_progress_token();
        let interval = self.settings.heartbeat;
        let mut heartbeat = tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
        let started = Instant::now();
        let mut beats = 0u32;
        let ticket = attachment.ticket().to_owned();
        let finished = attachment.finished();
        tokio::pin!(finished);
        loop {
            tokio::select! {
                result = &mut finished => return Some(result),
                _ = context.ct.cancelled() => return None,
                _ = &mut deadline => return None,
                _ = heartbeat.tick(), if progress_token.is_some() => {
                    beats += 1;
                    let elapsed = started.elapsed().as_secs();
                    let param = ProgressNotificationParam::new(
                        progress_token.clone().expect("guarded by the select condition"),
                        f64::from(beats),
                    )
                    .with_message(format!(
                        "Waiting for the user to answer question round {ticket:?} ({}m{:02}s so far)",
                        elapsed / 60,
                        elapsed % 60
                    ));
                    if let Err(e) = context.peer.notify_progress(param).await {
                        log::warn!("heartbeat for {ticket:?} failed: {e}");
                    }
                }
            }
        }
    }
}

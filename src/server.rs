//! The MCP server: stateless Streamable HTTP, on `127.0.0.1` by default (ADR 0001, 0002), with the
//! `ask_user_question` and `wait_for_answers` tools (ADR 0003, 0004).

use std::{
    net::{IpAddr, SocketAddr},
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
        ListToolsResult, PaginatedRequestParams, ProgressNotificationParam, ServerCapabilities,
        ServerConfig, Tool,
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
    desk::{Attachment, Client},
    format::{self, RoundResult},
    schema::{self, AskInput, Output, WaitInput},
};

#[derive(Debug, Clone)]
pub struct ServerSettings {
    /// Address or name to listen on.
    pub host: String,
    /// `0` picks a free port.
    pub port: u16,
    pub bearer_token: Option<String>,
    /// Interval between Heartbeat progress notifications.
    pub heartbeat: Duration,
    /// How long `wait_for_answers` blocks before returning `pending`.
    pub pickup_wait: Duration,
}

pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 7457;

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            host: DEFAULT_HOST.to_owned(),
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

    /// Stops accepting connections and frees the port. Calls already in flight keep their
    /// connection and finish normally.
    pub async fn stop(mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The app's MCP server on a runtime of its own, restartable with new settings.
pub struct ServerControl {
    runtime: tokio::runtime::Runtime,
    desk: Desk,
    running: Option<(RunningServer, ServerSettings)>,
}

impl ServerControl {
    pub fn new(desk: Desk) -> Self {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime starts");
        Self {
            runtime,
            desk,
            running: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// Stops the running server, if any, and starts one with `settings`; returns its MCP
    /// URL. If the new one cannot start, the previous one is started again.
    pub fn restart(&mut self, settings: ServerSettings) -> Result<String, String> {
        let previous = self.running.take().map(|(server, previous)| {
            self.runtime.block_on(server.stop());
            previous
        });
        self.start(settings).inspect_err(|_| {
            if let Some(previous) = previous {
                let _ = self.start(previous);
            }
        })
    }

    fn start(&mut self, settings: ServerSettings) -> Result<String, String> {
        let server = self
            .runtime
            .block_on(start(self.desk.clone(), settings.clone()))
            .map_err(|e| format!("cannot listen on {}:{}: {e}", settings.host, settings.port))?;
        let url = format!("http://{}/mcp", server.addr());
        // A restore after a failed restart must come back on the same port.
        let settings = ServerSettings {
            port: server.addr().port(),
            ..settings
        };
        self.running = Some((server, settings));
        Ok(url)
    }
}

/// Binds `<host>:<port>` and serves the MCP endpoint at `/mcp` on the current runtime.
pub async fn start(desk: Desk, settings: ServerSettings) -> std::io::Result<RunningServer> {
    let settings = Arc::new(settings);
    let handler = Handler {
        desk,
        settings: settings.clone(),
    };
    let config = StreamableHttpServerConfig::default().with_legacy_session_mode(false);
    let config = match settings.host.parse::<IpAddr>() {
        // Clients reach a wildcard address under names we cannot list; the Origin check
        // still keeps browsers (and with them DNS rebinding) out.
        Ok(ip) if ip.is_unspecified() => config.disable_allowed_hosts(),
        _ => {
            let mut hosts = config.allowed_hosts.clone();
            hosts.push(settings.host.clone());
            config.with_allowed_hosts(hosts)
        }
    };
    let service: StreamableHttpService<Handler, LocalSessionManager> =
        StreamableHttpService::new(move || Ok(handler.clone()), Default::default(), config);
    let app = axum::Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(settings.clone(), guard));

    let listener = tokio::net::TcpListener::bind((settings.host.as_str(), settings.port)).await?;
    let addr = listener.local_addr()?;
    let task = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            log::error!("MCP server stopped: {e}");
        }
    });
    Ok(RunningServer { addr, task })
}

/// Rejects browsers (any `Origin` header, 403) and clients without the bearer token (401).
async fn guard(
    State(settings): State<Arc<ServerSettings>>,
    request: Request,
    next: Next,
) -> Response {
    let headers = request.headers();
    if headers.contains_key(header::ORIGIN) {
        return (StatusCode::FORBIDDEN, "Origin not allowed").into_response();
    }
    if let Some(token) = &settings.bearer_token {
        let expected = format!("Bearer {token}");
        if headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            != Some(&expected)
        {
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

Delivery: once the answers were sent to you, the round is delivered and its ticket is \
released; a later call with that ticket and different questions opens a new round. Asking \
for a delivered round's answers again (same questions, or `wait_for_answers`) needs the \
user's confirmation: the call waits for it, and if the user declines it fails with an \
error telling you to ask again with a new ticket.

The result has the AskUserQuestion shape, plus `structuredContent` with `status` \
(`answered` | `cancelled`), `answers`, `annotations` (the user's notes) and `unanswered`. \
The user may leave questions unanswered or cancel; follow up on those instead of guessing.";

const WAIT_DESCRIPTION: &str = "\
Pick up the answers of a question round by the ticket you chose in `ask_user_question`, \
after that call was interrupted or timed out. Blocks up to about 50 seconds and returns the \
answers, or status `pending` with the round's title and questions so you can confirm it is \
yours; call it again while pending. An unknown ticket is an error. If the answers were \
already delivered to you once, the user must confirm sending them again; until then the \
status is `pending`, and a declined request is an error.";

fn tool_error(message: impl Into<String>) -> CallToolResponse {
    CallToolResult::error(vec![ContentBlock::text(message.into())]).into()
}

/// The response for a finished wait: the result (the round is Delivered by now, see
/// [`Attachment::finished`]), or the error of a declined Re-delivery Request.
fn deliver(outcome: Result<RoundResult, String>) -> CallToolResponse {
    match outcome {
        Ok(result) => success(format::text(&result), format::structured(&result)),
        Err(e) => tool_error(e),
    }
}

fn success(text: String, output: Output) -> CallToolResponse {
    let mut response = CallToolResult::success(vec![ContentBlock::text(text)]);
    response.structured_content = Some(serde_json::to_value(output).expect("output serializes"));
    response.into()
}

impl ServerHandler for Handler {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("question-desk", env!("CARGO_PKG_VERSION")),
        )
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
        let client = context.meta.client_info().map(|c| Client {
            name: c.name,
            title: c.title,
        });
        let mut attachment = match self.desk.ask(input, client) {
            Ok(attachment) => attachment,
            Err(e) => return tool_error(e),
        };
        match self.wait(&mut attachment, None, &context).await {
            Some(outcome) => deliver(outcome),
            None => tool_error("the call was cancelled; the round stays open for pickup"),
        }
    }

    async fn wait_for_answers(
        &self,
        args: Value,
        context: RequestContext<RoleServer>,
    ) -> CallToolResponse {
        let input: WaitInput = match schema::parse(args) {
            Ok(input) => input,
            Err(e) => return tool_error(e),
        };
        let client = context.meta.client_info().map(|c| Client {
            name: c.name,
            title: c.title,
        });
        let Some(mut attachment) = self.desk.pickup(&input.ticket, client) else {
            return tool_error(format!(
                "unknown ticket {:?}: the Question Desk has no question round with this ticket",
                input.ticket
            ));
        };
        match self
            .wait(&mut attachment, Some(self.settings.pickup_wait), &context)
            .await
        {
            Some(outcome) => deliver(outcome),
            None => {
                let pending = if attachment.is_redelivery() {
                    format::pending_redelivery
                } else {
                    format::pending
                };
                let (text, output) =
                    pending(&input.ticket, attachment.title(), attachment.questions());
                success(text, output)
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
    ) -> Option<Result<RoundResult, String>> {
        let deadline = async {
            match deadline {
                Some(deadline) => tokio::time::sleep(deadline).await,
                None => std::future::pending().await,
            }
        };
        tokio::pin!(deadline);
        let progress_token = context.meta.get_progress_token();
        let interval = self.settings.heartbeat;
        let mut heartbeat =
            tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
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

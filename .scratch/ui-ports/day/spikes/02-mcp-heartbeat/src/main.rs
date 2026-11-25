//! Spike 2: a multi-minute blocking MCP tool call with Heartbeat, over
//! stateless Streamable HTTP, driven by a real Claude Code session.
//!
//! The server terminal is the "GUI": every `ask_user_question` call opens a
//! round with a Ticket and blocks until you type `a <ticket> <answer>` or
//! `c <ticket>` on stdin. Everything the client does is logged with
//! timestamps, so idle timeouts, auto-backgrounding and disconnects show up
//! in the log.

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, CancelledNotificationParam,
        ContentBlock, Implementation, InitializeRequestParams, InitializeResult,
        ListToolsResult, PaginatedRequestParams, ProgressNotificationParam, ServerCapabilities,
        ServerConfig, Tool,
    },
    service::{NotificationContext, RequestContext},
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::watch,
};
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Options

#[derive(Clone, Debug)]
struct Opts {
    port: u16,
    /// Seconds between Heartbeat progress notifications; 0 disables them.
    heartbeat: u64,
    /// Seconds between SSE keep-alive comments; 0 disables them.
    sse_keep_alive: u64,
    /// How long `wait_for_answers` blocks before returning "pending".
    pickup_wait: u64,
    /// Answer every round automatically after this many seconds.
    auto_answer: Option<u64>,
    token: Option<String>,
}

fn parse_opts() -> Opts {
    let mut opts = Opts {
        port: 7457,
        heartbeat: 15,
        sse_keep_alive: 15,
        pickup_wait: 50,
        auto_answer: None,
        token: std::env::var("SPIKE_TOKEN").ok().filter(|t| !t.is_empty()),
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| usage(&format!("{flag} needs a value")));
        match flag.as_str() {
            "--port" => opts.port = value().parse().unwrap_or_else(|_| usage("bad --port")),
            "--heartbeat" => opts.heartbeat = value().parse().unwrap_or_else(|_| usage("bad --heartbeat")),
            "--sse-keep-alive" => {
                opts.sse_keep_alive = value().parse().unwrap_or_else(|_| usage("bad --sse-keep-alive"))
            }
            "--pickup-wait" => opts.pickup_wait = value().parse().unwrap_or_else(|_| usage("bad --pickup-wait")),
            "--auto-answer" => {
                opts.auto_answer = Some(value().parse().unwrap_or_else(|_| usage("bad --auto-answer")))
            }
            "--token" => opts.token = Some(value()),
            "-h" | "--help" => usage(""),
            other => usage(&format!("unknown flag {other}")),
        }
    }
    opts
}

fn usage(error: &str) -> ! {
    if !error.is_empty() {
        eprintln!("error: {error}\n");
    }
    eprintln!(
        "usage: spike-mcp-heartbeat [--port 7457] [--heartbeat 15] [--sse-keep-alive 15]\n\
         \x20                          [--pickup-wait 50] [--auto-answer SECS] [--token TOKEN]\n\
         \n\
         --heartbeat 0 / --sse-keep-alive 0 disable them. SPIKE_TOKEN sets the token too."
    );
    std::process::exit(if error.is_empty() { 0 } else { 2 })
}

// ---------------------------------------------------------------------------
// Rounds (a stand-in for the desk)

#[derive(Clone, Debug)]
enum Outcome {
    Answered(String),
    Cancelled,
}

struct Round {
    questions: Vec<String>,
    opened: Instant,
    outcome: watch::Sender<Option<Outcome>>,
    /// Number of tool calls currently blocked on this round.
    waiting_calls: usize,
    delivered: bool,
}

#[derive(Clone, Default)]
struct Desk {
    rounds: Arc<Mutex<BTreeMap<String, Round>>>,
    next_ticket: Arc<AtomicU64>,
}

impl Desk {
    fn open(&self, questions: Vec<String>) -> (String, watch::Receiver<Option<Outcome>>) {
        let ticket = format!("t{}", self.next_ticket.fetch_add(1, Ordering::Relaxed) + 1);
        let (tx, rx) = watch::channel(None);
        self.rounds.lock().unwrap().insert(
            ticket.clone(),
            Round { questions, opened: Instant::now(), outcome: tx, waiting_calls: 1, delivered: false },
        );
        (ticket, rx)
    }

    fn subscribe(&self, ticket: &str) -> Option<watch::Receiver<Option<Outcome>>> {
        let mut rounds = self.rounds.lock().unwrap();
        let round = rounds.get_mut(ticket)?;
        round.waiting_calls += 1;
        Some(round.outcome.subscribe())
    }

    fn leave(&self, ticket: &str, delivered: bool) {
        if let Some(round) = self.rounds.lock().unwrap().get_mut(ticket) {
            round.waiting_calls -= 1;
            round.delivered |= delivered;
        }
    }

    fn resolve(&self, ticket: &str, outcome: Outcome) -> bool {
        match self.rounds.lock().unwrap().get(ticket) {
            Some(round) if round.outcome.borrow().is_none() => {
                round.outcome.send_replace(Some(outcome));
                true
            }
            _ => false,
        }
    }

    fn result(&self, ticket: &str, outcome: &Outcome) -> CallToolResult {
        let rounds = self.rounds.lock().unwrap();
        let questions = rounds.get(ticket).map(|r| r.questions.clone()).unwrap_or_default();
        let (text, structured) = match outcome {
            Outcome::Answered(answer) => {
                let pairs: Vec<String> =
                    questions.iter().map(|q| format!("\"{q}\"=\"{answer}\"")).collect();
                let answers: serde_json::Map<String, Value> =
                    questions.iter().map(|q| (q.clone(), json!(answer))).collect();
                (
                    format!(
                        "Your questions have been answered: {}. You can now continue with these answers in mind.",
                        pairs.join(", ")
                    ),
                    json!({ "status": "answered", "ticket": ticket, "answers": answers }),
                )
            }
            Outcome::Cancelled => (
                "The user cancelled the question round without answering.".to_string(),
                json!({ "status": "cancelled", "ticket": ticket, "answers": {} }),
            ),
        };
        let mut result = CallToolResult::success(vec![ContentBlock::text(text)]);
        result.structured_content = Some(structured);
        result
    }

    fn print_list(&self) {
        let rounds = self.rounds.lock().unwrap();
        if rounds.is_empty() {
            println!("  (no rounds)");
        }
        for (ticket, r) in rounds.iter() {
            let state = match &*r.outcome.borrow() {
                None => "open".to_string(),
                Some(Outcome::Answered(a)) => format!("answered {a:?}"),
                Some(Outcome::Cancelled) => "cancelled".to_string(),
            };
            println!(
                "  {ticket}  {:>6}  {state}  calls-waiting={} delivered={}  {:?}",
                fmt_elapsed(r.opened),
                r.waiting_calls,
                r.delivered,
                r.questions
            );
        }
    }
}

fn fmt_elapsed(since: Instant) -> String {
    let s = since.elapsed().as_secs();
    format!("{}m{:02}s", s / 60, s % 60)
}

// ---------------------------------------------------------------------------
// MCP handler

#[derive(Clone)]
struct Spike {
    desk: Desk,
    opts: Arc<Opts>,
}

const ASK_DESCRIPTION: &str = "Ask the user one or more questions and wait for the answers. \
Same shape as the built-in AskUserQuestion tool. This call blocks until the user answers, which can take many minutes. \
Every round gets a Ticket (shown in progress messages and in the result). If this call is interrupted or times out, \
call `wait_for_answers` with the Ticket to pick up the answers instead of asking again.";

const WAIT_DESCRIPTION: &str = "Pick up the answers of a question round by its Ticket. \
Blocks up to about 50 seconds and returns either the answers or status `pending`; call it again while pending.";

fn tools() -> Vec<Tool> {
    let ask_input = json!({
        "type": "object",
        "properties": {
            "questions": {
                "type": "array",
                "minItems": 1,
                "items": {
                    "type": "object",
                    "properties": {
                        "question": { "type": "string" },
                        "header": { "type": "string" },
                        "options": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "label": { "type": "string" },
                                    "description": { "type": "string" }
                                },
                                "required": ["label"]
                            }
                        },
                        "multiSelect": { "type": "boolean" }
                    },
                    "required": ["question"]
                }
            }
        },
        "required": ["questions"]
    });
    let wait_input = json!({
        "type": "object",
        "properties": { "ticket": { "type": "string" } },
        "required": ["ticket"]
    });
    let output = json!({
        "type": "object",
        "properties": {
            "status": { "type": "string", "enum": ["answered", "cancelled", "pending"] },
            "ticket": { "type": "string" },
            "answers": { "type": "object", "additionalProperties": { "type": "string" } }
        },
        "required": ["status", "ticket"]
    });
    let as_obj = |v: Value| Arc::new(v.as_object().unwrap().clone());
    let mut ask = Tool::new("ask_user_question", ASK_DESCRIPTION, as_obj(ask_input));
    ask.output_schema = Some(as_obj(output.clone()));
    let mut wait = Tool::new("wait_for_answers", WAIT_DESCRIPTION, as_obj(wait_input));
    wait.output_schema = Some(as_obj(output));
    vec![ask, wait]
}

fn tool_error(message: impl Into<String>) -> Result<CallToolResponse, McpError> {
    Ok(CallToolResult::error(vec![ContentBlock::text(message.into())]).into())
}

impl Spike {
    /// Blocks until the round resolves, the client goes away, or `deadline`
    /// passes, sending Heartbeats meanwhile. Shared by both tools.
    async fn wait(
        &self,
        tool: &str,
        ticket: &str,
        mut rx: watch::Receiver<Option<Outcome>>,
        deadline: Option<Duration>,
        context: &RequestContext<RoleServer>,
    ) -> Option<Outcome> {
        let started = Instant::now();
        let token = context.meta.get_progress_token();
        match &token {
            Some(t) => info!(tool, ticket, progress_token = ?t.0, "call blocking; client sent a progressToken"),
            None => warn!(tool, ticket, "call blocking; client sent NO progressToken, so no Heartbeat is possible"),
        }
        let heartbeat = Duration::from_secs(self.opts.heartbeat.max(1));
        let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + heartbeat, heartbeat);
        let sleep = tokio::time::sleep(deadline.unwrap_or(Duration::from_secs(u64::MAX / 4)));
        tokio::pin!(sleep);
        let mut beats = 0u64;

        loop {
            if let Some(outcome) = rx.borrow_and_update().clone() {
                return Some(outcome);
            }
            tokio::select! {
                changed = rx.changed() => {
                    if changed.is_err() { return None; }
                }
                _ = ticker.tick(), if self.opts.heartbeat > 0 && token.is_some() => {
                    beats += 1;
                    let param = ProgressNotificationParam::new(token.clone().unwrap(), beats as f64)
                        .with_message(format!(
                            "Waiting for the user to answer (ticket {ticket}, {} so far)",
                            fmt_elapsed(started)
                        ));
                    match context.peer.notify_progress(param).await {
                        Ok(()) => info!(tool, ticket, beat = beats, elapsed = %fmt_elapsed(started), "heartbeat sent"),
                        Err(e) => warn!(tool, ticket, beat = beats, elapsed = %fmt_elapsed(started), error = %e, "heartbeat FAILED"),
                    }
                }
                _ = context.ct.cancelled() => {
                    warn!(tool, ticket, elapsed = %fmt_elapsed(started), "call CANCELLED (client disconnected or sent notifications/cancelled); round stays open for pickup");
                    return None;
                }
                _ = &mut sleep => {
                    info!(tool, ticket, elapsed = %fmt_elapsed(started), "pickup wait elapsed; returning pending");
                    return None;
                }
            }
        }
    }
}

impl ServerHandler for Spike {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("spike-mcp-heartbeat", "0.1.0"))
    }

    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        info!(
            client = ?request.client_info,
            protocol_version = %request.protocol_version,
            capabilities = %serde_json::to_string(&request.capabilities).unwrap_or_default(),
            "initialize"
        );
        context.peer.set_peer_info(request.clone());
        let result = self.negotiate_initialize(&request);
        if let Ok(r) = &result {
            info!(negotiated = %r.protocol_version, "initialize answered");
        }
        result
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        info!("tools/list");
        Ok(ListToolsResult::with_all_items(tools()))
    }

    async fn on_cancelled(
        &self,
        notification: CancelledNotificationParam,
        _context: NotificationContext<RoleServer>,
    ) {
        warn!(request_id = ?notification.request_id, reason = ?notification.reason, "notifications/cancelled received");
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let args = Value::Object(request.arguments.clone().unwrap_or_default());
        let peer = context.peer.peer_info().map(|p| format!("{:?}", p.client_info));
        info!(
            tool = %request.name,
            request_id = ?context.id,
            peer_info = ?peer,
            meta = %serde_json::to_string(&context.meta).unwrap_or_default(),
            "tools/call"
        );

        match &*request.name {
            "ask_user_question" => {
                let Some(qs) = args.get("questions").and_then(Value::as_array).filter(|a| !a.is_empty()) else {
                    return tool_error("`questions` must be a non-empty array");
                };
                let questions: Vec<String> = qs
                    .iter()
                    .map(|q| q.get("question").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| q.to_string()))
                    .collect();
                let (ticket, rx) = self.desk.open(questions.clone());
                println!(
                    "\n>>> NEW ROUND {ticket}: {questions:?}\n    answer with `a {ticket} <text>`, cancel with `c {ticket}`\n"
                );
                if let Some(secs) = self.opts.auto_answer {
                    let (desk, t) = (self.desk.clone(), ticket.clone());
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(secs)).await;
                        if desk.resolve(&t, Outcome::Answered(format!("auto-answer after {secs}s"))) {
                            info!(ticket = %t, "auto-answered");
                        }
                    });
                }
                let outcome = self.wait("ask_user_question", &ticket, rx, None, &context).await;
                self.desk.leave(&ticket, outcome.is_some());
                match outcome {
                    Some(o) => {
                        info!(ticket = %ticket, "ask_user_question returning result");
                        Ok(self.desk.result(&ticket, &o).into())
                    }
                    None => tool_error(format!("call interrupted; pick up with wait_for_answers(ticket={ticket:?})")),
                }
            }
            "wait_for_answers" => {
                let Some(ticket) = args.get("ticket").and_then(Value::as_str).map(str::to_owned) else {
                    return tool_error("`ticket` is required");
                };
                let Some(rx) = self.desk.subscribe(&ticket) else {
                    warn!(ticket = %ticket, "unknown ticket");
                    return tool_error(format!("unknown ticket {ticket:?}"));
                };
                let deadline = Duration::from_secs(self.opts.pickup_wait);
                let outcome = self.wait("wait_for_answers", &ticket, rx, Some(deadline), &context).await;
                self.desk.leave(&ticket, outcome.is_some());
                match outcome {
                    Some(o) => {
                        info!(ticket = %ticket, "wait_for_answers returning result (PICKUP)");
                        Ok(self.desk.result(&ticket, &o).into())
                    }
                    None => {
                        let mut result = CallToolResult::success(vec![ContentBlock::text(format!(
                            "Still pending: the user has not answered ticket {ticket} yet. Call wait_for_answers again."
                        ))]);
                        result.structured_content = Some(json!({ "status": "pending", "ticket": ticket }));
                        Ok(result.into())
                    }
                }
            }
            other => tool_error(format!("unknown tool {other:?}")),
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP: auth, Origin rejection, request/stream logging

static NEXT_HTTP_ID: AtomicU64 = AtomicU64::new(1);

/// Logs when the response body is dropped, i.e. the stream finished or the
/// client went away.
struct StreamEnd {
    id: u64,
    started: Instant,
}

impl Drop for StreamEnd {
    fn drop(&mut self) {
        info!(http = self.id, open_for = %fmt_elapsed(self.started), "response stream closed");
    }
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
    headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("-")
}

async fn guard_and_log(State(opts): State<Arc<Opts>>, request: Request, next: Next) -> Response {
    let id = NEXT_HTTP_ID.fetch_add(1, Ordering::Relaxed);
    let (parts, body) = request.into_parts();
    let headers = &parts.headers;

    let bytes = axum::body::to_bytes(body, 4 * 1024 * 1024).await.unwrap_or_default();
    let rpc: Option<Value> = serde_json::from_slice(&bytes).ok();
    let rpc_method = rpc.as_ref().and_then(|v| v.get("method")).and_then(Value::as_str).unwrap_or("-");
    let rpc_id = rpc.as_ref().and_then(|v| v.get("id")).map(|v| v.to_string()).unwrap_or_else(|| "-".into());
    info!(
        http = id,
        method = %parts.method,
        uri = %parts.uri,
        rpc_method,
        rpc_id,
        protocol = header_str(headers, "mcp-protocol-version"),
        session = header_str(headers, "mcp-session-id"),
        accept = header_str(headers, "accept"),
        user_agent = header_str(headers, "user-agent"),
        "http request"
    );
    tracing::debug!(http = id, body = %String::from_utf8_lossy(&bytes), "http request body");

    if headers.contains_key(header::ORIGIN) {
        warn!(http = id, origin = header_str(headers, "origin"), "rejected: Origin header present");
        return (StatusCode::FORBIDDEN, "Origin not allowed").into_response();
    }
    if let Some(token) = &opts.token {
        let expected = format!("Bearer {token}");
        if headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) != Some(expected.as_str()) {
            warn!(http = id, "rejected: missing or wrong bearer token");
            return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        }
    }

    let started = Instant::now();
    let response = next.run(Request::from_parts(parts, Body::from(bytes))).await;
    let content_type = header_str(response.headers(), "content-type").to_owned();
    info!(http = id, status = %response.status(), content_type, "http response started");

    let (parts, body) = response.into_parts();
    let end = StreamEnd { id, started };
    let body = Body::from_stream(body.into_data_stream().map(move |chunk| {
        let _keep = &end;
        chunk
    }));
    Response::from_parts(parts, body)
}

// ---------------------------------------------------------------------------
// stdin console

async fn console(desk: Desk) {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    println!("console: `ls`, `a <ticket> <answer>`, `c <ticket>`, `help`");
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        let (ticket, text) = rest.trim().split_once(' ').unwrap_or((rest.trim(), ""));
        match cmd {
            "" => {}
            "ls" => desk.print_list(),
            "a" if !ticket.is_empty() => {
                let answer = if text.is_empty() { "yes" } else { text };
                if desk.resolve(ticket, Outcome::Answered(answer.to_owned())) {
                    info!(ticket, answer, "SUBMITTED by user");
                } else {
                    println!("  no open round {ticket}");
                }
            }
            "c" if !ticket.is_empty() => {
                if desk.resolve(ticket, Outcome::Cancelled) {
                    info!(ticket, "CANCELLED by user");
                } else {
                    println!("  no open round {ticket}");
                }
            }
            _ => println!("  `ls` | `a <ticket> <answer>` | `c <ticket>`"),
        }
    }
}

// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `tracing`'s `log` feature forwards events to env_logger. RUST_LOG works as
    // usual; `RUST_LOG=info,spike_mcp_heartbeat=debug` also logs request bodies.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,rmcp=warn,tracing=warn")).init();

    let opts = Arc::new(parse_opts());
    info!(?opts, "starting");
    let desk = Desk::default();

    let keep_alive = (opts.sse_keep_alive > 0).then(|| Duration::from_secs(opts.sse_keep_alive));
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_sse_keep_alive(keep_alive);
    let spike = Spike { desk: desk.clone(), opts: opts.clone() };
    let service: StreamableHttpService<Spike, LocalSessionManager> =
        StreamableHttpService::new(move || Ok(spike.clone()), Default::default(), config);

    let app = axum::Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(opts.clone(), guard_and_log));

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", opts.port)).await?;
    info!("listening on http://{}/mcp", listener.local_addr()?);
    tokio::spawn(console(desk));
    axum::serve(listener, app).await?;
    Ok(())
}

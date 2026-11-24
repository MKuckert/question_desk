//! The headless Question Desk shared by the integration tests: a real server on an
//! ephemeral port plus the `desk`, optionally on a data dir.
#![allow(dead_code)]

pub use std::{sync::Arc, time::Duration};

use question_desk::{
    desk::{Desk, RoundSummary},
    server::{self, RunningServer, ServerSettings},
    store::Store,
};
use rmcp::{
    ClientHandler, RoleClient,
    model::{
        CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, Implementation,
        ProgressNotificationParam, ProtocolVersion,
    },
    service::{ClientLifecycleMode, ClientServiceExt, NotificationContext, RunningService},
    transport::StreamableHttpClientTransport,
};
use serde_json::{Value, json};
use tokio::sync::Mutex;

pub struct Harness {
    pub desk: Desk,
    pub server: RunningServer,
}

pub async fn start() -> Harness {
    start_with(fast_settings()).await
}

pub fn fast_settings() -> ServerSettings {
    ServerSettings {
        heartbeat: Duration::from_millis(50),
        pickup_wait: Duration::from_millis(300),
        ..ServerSettings::default()
    }
}

pub async fn start_with(settings: ServerSettings) -> Harness {
    serve(Desk::default(), settings).await
}

/// A desk storing its rounds in `data_dir`. A "restart" is dropping the harness (after
/// [`Desk::close`], as on quit) and starting a new one on the same dir.
pub async fn start_in(data_dir: &std::path::Path) -> Harness {
    serve(Desk::with_store(Store::new(data_dir)), fast_settings()).await
}

pub async fn serve(desk: Desk, settings: ServerSettings) -> Harness {
    let server = server::start(
        desk.clone(),
        ServerSettings {
            port: 0,
            ..settings
        },
    )
    .await
    .expect("server starts");
    Harness { desk, server }
}

/// An Agent: records the Heartbeats it receives.
#[derive(Clone, Default)]
pub struct Agent {
    pub client_info: Option<Implementation>,
    pub progress: Arc<Mutex<Vec<ProgressNotificationParam>>>,
}

impl ClientHandler for Agent {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(
            ClientCapabilities::default(),
            self.client_info
                .clone()
                .unwrap_or_else(|| Implementation::new("test-agent", "1.0")),
        )
    }

    async fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _: NotificationContext<RoleClient>,
    ) {
        self.progress.lock().await.push(params);
    }
}

impl Harness {
    pub fn url(&self) -> String {
        format!("http://{}/mcp", self.server.addr())
    }

    pub async fn agent(&self) -> RunningService<RoleClient, Agent> {
        self.agent_as(Agent::default()).await
    }

    pub async fn agent_as(&self, agent: Agent) -> RunningService<RoleClient, Agent> {
        Self::agent_at(&self.url(), agent).await
    }

    pub async fn agent_at(url: &str, agent: Agent) -> RunningService<RoleClient, Agent> {
        // Like Claude Code: `server/discover` on protocol 2026-07-28, so the client's
        // identity travels in each request's `_meta`.
        let transport = StreamableHttpClientTransport::from_uri(url.to_owned());
        let lifecycle = ClientLifecycleMode::Discover {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        };
        agent
            .serve_with_lifecycle(transport, lifecycle)
            .await
            .expect("agent connects")
    }

    /// Waits until the desk shows an open round with this Ticket, and selects it.
    pub async fn select(&self, ticket: &str) {
        let round = self.appeared(ticket).await;
        self.desk.select(&round.id);
    }

    /// Waits until no Agent call is attached to the open round with this Ticket any more.
    pub async fn disconnected(&self, ticket: &str) {
        for _ in 0..200 {
            if !self.appeared(ticket).await.connected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("round {ticket} stayed connected");
    }

    /// Waits until the desk shows an open round with this Ticket.
    pub async fn appeared(&self, ticket: &str) -> RoundSummary {
        for _ in 0..200 {
            if let Some(round) = self
                .desk
                .sessions()
                .into_iter()
                .flat_map(|s| s.rounds)
                .find(|r| r.ticket == ticket)
            {
                return round;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("round {ticket} never appeared on the desk");
    }
}

pub async fn call(
    agent: &RunningService<RoleClient, Agent>,
    tool: &str,
    args: Value,
) -> CallToolResult {
    let params = CallToolRequestParams::new(tool.to_owned())
        .with_arguments(args.as_object().expect("arguments are an object").clone());
    tokio::time::timeout(Duration::from_secs(10), agent.call_tool(params))
        .await
        .expect("tool call returns within 10 s")
        .expect("tool call completes")
}

pub fn text(result: &CallToolResult) -> String {
    result.content[0]
        .as_text()
        .expect("text content")
        .text
        .clone()
}

pub fn db_round(ticket: &str) -> Value {
    json!({
        "ticket": ticket,
        "title": "Storage",
        "questions": [
            {
                "question": "Which database?",
                "header": "Database",
                "body": "We need **transactions**.",
                "options": [
                    { "label": "Postgres", "description": "Relational, mature" },
                    { "label": "SQLite", "description": "Embedded" }
                ],
                "recommended": "Postgres"
            },
            {
                "question": "Anything else?",
                "header": "Other"
            }
        ]
    })
}

/// An Agent whose `clientInfo` has this name and optional title.
pub fn agent_named(name: &str, title: Option<&str>) -> Agent {
    let mut info = Implementation::new(name, "1.0");
    if let Some(title) = title {
        info = info.with_title(title);
    }
    Agent {
        client_info: Some(info),
        ..Agent::default()
    }
}

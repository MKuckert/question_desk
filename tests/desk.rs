//! The primary seam: a headless Question Desk. The Agent side is driven by a real rmcp
//! client over HTTP, the user side by the `desk` user intents.

use std::{sync::Arc, time::Duration};

use question_desk::{
    desk::Desk,
    server::{self, RunningServer, ServerSettings},
};
use rmcp::{
    ClientHandler, RoleClient, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, Implementation,
        ProgressNotificationParam,
    },
    service::{NotificationContext, RunningService},
    transport::{
        StreamableHttpClientTransport,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use tokio::sync::Mutex;

struct Harness {
    desk: Desk,
    server: RunningServer,
}

async fn start() -> Harness {
    start_with(ServerSettings {
        heartbeat: Duration::from_millis(50),
        pickup_wait: Duration::from_millis(300),
        ..ServerSettings::default()
    })
    .await
}

async fn start_with(settings: ServerSettings) -> Harness {
    let desk = Desk::default();
    let server = server::start(desk.clone(), ServerSettings { port: 0, ..settings })
        .await
        .expect("server starts");
    Harness { desk, server }
}

/// An Agent: records the Heartbeats it receives.
#[derive(Clone, Default)]
struct Agent {
    client_info: Option<Implementation>,
    progress: Arc<Mutex<Vec<ProgressNotificationParam>>>,
}

impl ClientHandler for Agent {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(
            ClientCapabilities::default(),
            self.client_info.clone().unwrap_or_else(|| Implementation::new("test-agent", "1.0")),
        )
    }

    async fn on_progress(&self, params: ProgressNotificationParam, _: NotificationContext<RoleClient>) {
        self.progress.lock().await.push(params);
    }
}

impl Harness {
    fn url(&self) -> String {
        format!("http://{}/mcp", self.server.addr())
    }

    async fn agent(&self) -> RunningService<RoleClient, Agent> {
        self.agent_as(Agent::default()).await
    }

    async fn agent_as(&self, agent: Agent) -> RunningService<RoleClient, Agent> {
        let transport = StreamableHttpClientTransport::from_uri(self.url());
        agent.serve(transport).await.expect("agent connects")
    }

    /// Waits until the desk shows an open round with this Ticket, and selects it.
    async fn select(&self, ticket: &str) {
        for _ in 0..200 {
            if self.desk.sessions().iter().flat_map(|s| &s.rounds).any(|r| r.ticket == ticket) {
                self.desk.select(ticket);
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("round {ticket} never appeared on the desk");
    }
}

async fn call(agent: &RunningService<RoleClient, Agent>, tool: &str, args: Value) -> CallToolResult {
    let params = CallToolRequestParams::new(tool.to_owned())
        .with_arguments(args.as_object().expect("arguments are an object").clone());
    agent.call_tool(params).await.expect("tool call completes")
}

fn text(result: &CallToolResult) -> String {
    result.content[0].as_text().expect("text content").text.clone()
}

fn db_round(ticket: &str) -> Value {
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

#[tokio::test]
async fn the_user_answers_a_round_and_the_agent_receives_the_answers() {
    let h = start().await;
    let agent = h.agent().await;
    let call = tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });

    h.select("db-1").await;
    h.desk.toggle_option(1);
    h.desk.next();
    h.desk.set_note("Keep it simple");
    h.desk.next();
    h.desk.submit();

    let result = call.await.unwrap();
    assert_eq!(result.is_error, Some(false));
    assert_eq!(
        text(&result),
        r#"Your questions have been answered: "Which database?"="SQLite", "Anything else?"="Keep it simple". You can now continue with these answers in mind."#
    );
    assert_eq!(
        result.structured_content,
        Some(json!({
            "status": "answered",
            "ticket": "db-1",
            "answers": { "Which database?": "SQLite", "Anything else?": "Keep it simple" }
        }))
    );
}

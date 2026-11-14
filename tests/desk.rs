//! The primary seam: a headless Question Desk. The Agent side is driven by a real rmcp
//! client over HTTP, the user side by the `desk` user intents.

use std::{sync::Arc, time::Duration};

use question_desk::{
    desk::Desk,
    server::{self, RunningServer, ServerSettings},
};
use rmcp::{
    ClientHandler, RoleClient,
    model::{
        CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, Implementation,
        ProgressNotificationParam, ProtocolVersion,
    },
    service::{ClientLifecycleMode, ClientServiceExt, NotificationContext, RunningService},
    transport::{
        StreamableHttpClientTransport,
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
        // Like Claude Code: `server/discover` on protocol 2026-07-28, so the client's
        // identity travels in each request's `_meta`.
        let transport = StreamableHttpClientTransport::from_uri(self.url());
        let lifecycle =
            ClientLifecycleMode::Discover { preferred_versions: vec![ProtocolVersion::V_2026_07_28] };
        agent.serve_with_lifecycle(transport, lifecycle).await.expect("agent connects")
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
    tokio::time::timeout(Duration::from_secs(10), agent.call_tool(params))
        .await
        .expect("tool call returns within 10 s")
        .expect("tool call completes")
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

#[tokio::test]
async fn invalid_rounds_are_rejected_with_a_precise_tool_error_and_never_reach_the_user() {
    let h = start().await;
    let agent = h.agent().await;
    let q = |text: &str| json!({ "question": text, "header": "H" });
    let cases = [
        (
            json!({ "questions": [q("A?")] }),
            "`ticket` is required: choose a short unique id",
        ),
        (json!({ "ticket": "t 1", "questions": [q("A?")] }), "`ticket` contains ' '"),
        (json!({ "ticket": "t1", "questions": [] }), "`questions` must contain at least one question"),
        (json!({ "ticket": "t1", "questions": [q("A?"), q("A?")] }), "`questions[1].question` duplicates"),
        (
            json!({ "ticket": "t1", "questions": [{ "question": "A?", "header": "H", "multiSelect": true }] }),
            "`questions[0].multiSelect` is true but the question has no options",
        ),
        (
            json!({ "ticket": "t1", "questions": [{ "question": "A?", "header": "H", "options": [
                { "label": "X", "description": "" }, { "label": "X", "description": "" }
            ] }] }),
            "`questions[0].options[1].label` duplicates",
        ),
        (
            json!({ "ticket": "t1", "questions": [{ "question": "A?" }] }),
            "invalid arguments at `questions[0]`: missing field `header`",
        ),
        (
            json!({ "ticket": "t1", "questions": [{ "question": "A?", "header": "H", "multiselect": true }] }),
            "invalid arguments at `questions[0].multiselect`: unknown field `multiselect`",
        ),
    ];
    for (args, expected) in cases {
        let result = call(&agent, "ask_user_question", args.clone()).await;
        assert_eq!(result.is_error, Some(true), "{args}");
        assert!(text(&result).starts_with(expected), "{args}\n  got: {}\n  expected: {expected}", text(&result));
    }
    assert!(h.desk.sessions().is_empty());
}

#[tokio::test]
async fn multi_select_toggles_like_checkboxes_and_single_select_like_radio_buttons() {
    let h = start().await;
    let agent = h.agent().await;
    let options = json!([
        { "label": "Auth", "description": "" },
        { "label": "Billing", "description": "" },
        { "label": "Search", "description": "" }
    ]);
    let args = json!({ "ticket": "features", "questions": [
        { "question": "Which features?", "header": "Scope", "options": options, "multiSelect": true },
        { "question": "Which first?", "header": "Order", "options": options },
        { "question": "Which last?", "header": "Order", "options": options }
    ] });
    let call = tokio::spawn(async move { call(&agent, "ask_user_question", args).await });

    h.select("features").await;
    for option in [2, 0, 1, 1] {
        h.desk.toggle_option(option);
    }
    h.desk.next();
    h.desk.toggle_option(0);
    h.desk.toggle_option(2);
    h.desk.next();
    h.desk.toggle_option(1);
    h.desk.toggle_option(1);
    h.desk.submit();

    let result = call.await.unwrap();
    assert_eq!(
        result.structured_content,
        Some(json!({
            "status": "answered",
            "ticket": "features",
            "answers": { "Which features?": "Auth, Search", "Which first?": "Search" },
            "unanswered": ["Which last?"]
        }))
    );
}

#[tokio::test]
async fn cancelling_delivers_the_partial_answers_with_their_notes() {
    let h = start().await;
    let agent = h.agent().await;
    let call = tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-2")).await });

    h.select("db-2").await;
    h.desk.toggle_option(0);
    h.desk.set_note("v16 or newer");
    h.desk.cancel();

    let result = call.await.unwrap();
    assert_eq!(
        text(&result),
        r#"The user cancelled the question round. Answers given before cancelling: "Which database?"="Postgres" (note: "v16 or newer"). Unanswered: "Anything else?"."#
    );
    assert_eq!(
        result.structured_content,
        Some(json!({
            "status": "cancelled",
            "ticket": "db-2",
            "answers": { "Which database?": "Postgres" },
            "annotations": { "Which database?": { "notes": "v16 or newer" } },
            "unanswered": ["Anything else?"]
        }))
    );
    assert!(h.desk.sessions().is_empty(), "a cancelled round leaves the sidebar");
}

#[tokio::test]
async fn repeating_the_call_with_the_same_ticket_reattaches_instead_of_asking_twice() {
    let h = start().await;
    let first = h.agent().await;
    let second = h.agent().await;
    let first = tokio::spawn(async move { call(&first, "ask_user_question", db_round("db-3")).await });
    h.select("db-3").await;
    let second = tokio::spawn(async move { call(&second, "ask_user_question", db_round("db-3")).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(h.desk.sessions().iter().flat_map(|s| &s.rounds).count(), 1);

    h.desk.toggle_option(0);
    h.desk.submit();

    let (first, second) = (first.await.unwrap(), second.await.unwrap());
    assert_eq!(first, second);
    assert_eq!(text(&first).contains(r#""Which database?"="Postgres""#), true);

    let late = call(&h.agent().await, "ask_user_question", db_round("db-3")).await;
    assert_eq!(late, first, "a finished round's result is delivered again");
}

#[tokio::test]
async fn reusing_a_ticket_for_different_questions_is_a_tool_error() {
    let h = start().await;
    let agent = h.agent().await;
    let _open = {
        let agent = h.agent().await;
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-4")).await })
    };
    h.select("db-4").await;

    let mut other = db_round("db-4");
    other["questions"][0]["question"] = json!("Which cache?");
    let result = call(&agent, "ask_user_question", other).await;

    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        text(&result),
        "ticket \"db-4\" is already used by a different question round; choose a new unique ticket"
    );
}

#[tokio::test]
async fn after_a_dropped_call_the_agent_picks_up_the_answers_with_wait_for_answers() {
    let h = start().await;
    let dropped = {
        let agent = h.agent().await;
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-5")).await })
    };
    h.select("db-5").await;
    dropped.abort();
    let _ = dropped.await;

    let agent = h.agent().await;
    let pending = call(&agent, "wait_for_answers", json!({ "ticket": "db-5" })).await;
    assert_eq!(pending.is_error, Some(false));
    assert_eq!(
        pending.structured_content,
        Some(json!({
            "status": "pending",
            "ticket": "db-5",
            "title": "Storage",
            "questions": ["Which database?", "Anything else?"]
        }))
    );
    assert_eq!(
        text(&pending),
        r#"Still pending: the user has not finished question round "db-5" ("Storage": "Which database?", "Anything else?") yet. Call wait_for_answers again."#
    );

    h.desk.toggle_option(1);
    h.desk.submit();
    let picked_up = call(&agent, "wait_for_answers", json!({ "ticket": "db-5" })).await;
    assert_eq!(picked_up.structured_content.unwrap()["answers"], json!({ "Which database?": "SQLite" }));
}

#[tokio::test]
async fn wait_for_answers_returns_as_soon_as_the_user_submits() {
    let h = start_with(ServerSettings {
        pickup_wait: Duration::from_secs(5),
        ..ServerSettings::default()
    })
    .await;
    let _open = {
        let agent = h.agent().await;
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-6")).await })
    };
    h.select("db-6").await;
    let agent = h.agent().await;
    let waiting = tokio::spawn(async move { call(&agent, "wait_for_answers", json!({ "ticket": "db-6" })).await });
    tokio::time::sleep(Duration::from_millis(100)).await;

    h.desk.cancel();
    let started = std::time::Instant::now();
    let result = waiting.await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(result.structured_content.unwrap()["status"], json!("cancelled"));
}

#[tokio::test]
async fn an_unknown_ticket_is_a_tool_error() {
    let h = start().await;
    let result = call(&h.agent().await, "wait_for_answers", json!({ "ticket": "nope" })).await;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        text(&result),
        "unknown ticket \"nope\": no question round was asked with it since the Question Desk started"
    );
}

#[tokio::test]
async fn a_blocked_call_sends_heartbeats_until_the_user_answers() {
    let h = start().await;
    let agent = Agent::default();
    let progress = agent.progress.clone();
    let agent = h.agent_as(agent).await;
    let call = tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-7")).await });
    h.select("db-7").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    h.desk.submit();
    call.await.unwrap();

    let progress = progress.lock().await;
    assert!(progress.len() >= 3, "expected heartbeats every 50 ms, got {}", progress.len());
    let message = progress[0].message.as_deref().unwrap_or_default();
    assert!(message.contains("db-7"), "heartbeat names the ticket: {message:?}");
}

#[tokio::test]
async fn the_desk_shows_when_no_agent_is_waiting_for_a_round_any_more() {
    let h = start().await;
    let connected = |h: &Harness| h.desk.sessions()[0].rounds[0].connected;
    let dropped = {
        let agent = h.agent().await;
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-8")).await })
    };
    h.select("db-8").await;
    assert!(connected(&h));

    dropped.abort();
    let _ = dropped.await;
    for _ in 0..100 {
        if !connected(&h) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!connected(&h), "the dropped call no longer counts as connected");

    let agent = h.agent().await;
    let _reattached = tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-8")).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(connected(&h), "reattaching connects the round again");
}

fn agent_named(name: &str, title: Option<&str>) -> Agent {
    let mut info = Implementation::new(name, "1.0");
    if let Some(title) = title {
        info = info.with_title(title);
    }
    Agent { client_info: Some(info), ..Agent::default() }
}

#[tokio::test]
async fn concurrent_rounds_are_grouped_by_agent_session() {
    let h = start().await;
    let ask = |agent: RunningService<RoleClient, Agent>, ticket: &str, source: Option<&str>| {
        let mut args = db_round(ticket);
        if let Some(source) = source {
            args["source"] = json!(source);
        }
        tokio::spawn(async move { call(&agent, "ask_user_question", args).await })
    };
    let _a1 = ask(h.agent_as(agent_named("claude-code", Some("Claude Code"))).await, "a1", Some("shop"));
    h.select("a1").await;
    let _b1 = ask(h.agent_as(agent_named("codex", None)).await, "b1", None);
    h.select("b1").await;
    let _a2 = ask(h.agent_as(agent_named("claude-code", Some("Claude Code"))).await, "a2", Some("shop"));
    h.select("a2").await;

    let sessions: Vec<(String, Vec<String>)> = h
        .desk
        .sessions()
        .into_iter()
        .map(|s| (s.label, s.rounds.into_iter().map(|r| r.ticket).collect()))
        .collect();
    assert_eq!(
        sessions,
        vec![
            ("Claude Code · shop".to_owned(), vec!["a1".to_owned(), "a2".to_owned()]),
            ("codex".to_owned(), vec!["b1".to_owned()]),
        ]
    );
}

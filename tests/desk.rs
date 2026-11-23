//! The primary seam: a headless Question Desk. The Agent side is driven by a real rmcp
//! client over HTTP, the user side by the `desk` user intents.

mod common;

use common::*;
use question_desk::desk::{Desk, QuestionState, Step};
use question_desk::server::ServerSettings;
use rmcp::{
    RoleClient,
    model::{Implementation, ProtocolVersion},
    service::{ClientLifecycleMode, ClientServiceExt, RunningService},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
#[tokio::test]
async fn the_user_answers_a_round_and_the_agent_receives_the_answers() {
    let h = start().await;
    let agent = h.agent().await;
    let call =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });

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
        (
            json!({ "ticket": "t 1", "questions": [q("A?")] }),
            "`ticket` contains ' '",
        ),
        (
            json!({ "ticket": "t1", "questions": [] }),
            "`questions` must contain at least one question",
        ),
        (
            json!({ "ticket": "t1", "questions": [q("A?"), q("A?")] }),
            "`questions[1].question` duplicates",
        ),
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
        assert!(
            text(&result).starts_with(expected),
            "{args}\n  got: {}\n  expected: {expected}",
            text(&result)
        );
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
    let call =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-2")).await });

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
    assert!(
        h.desk.sessions().is_empty(),
        "a cancelled round leaves the sidebar"
    );
}

#[tokio::test]
async fn repeating_the_call_with_the_same_ticket_reattaches_instead_of_asking_twice() {
    let h = start().await;
    let first = h.agent().await;
    let second = h.agent().await;
    let first =
        tokio::spawn(async move { call(&first, "ask_user_question", db_round("db-3")).await });
    h.select("db-3").await;
    let second =
        tokio::spawn(async move { call(&second, "ask_user_question", db_round("db-3")).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(h.desk.sessions().iter().flat_map(|s| &s.rounds).count(), 1);

    h.desk.toggle_option(0);
    h.desk.submit();

    let (first, second) = (first.await.unwrap(), second.await.unwrap());
    assert_eq!(first, second);
    assert!(text(&first).contains(r#""Which database?"="Postgres""#));

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
    assert_eq!(
        picked_up.structured_content.unwrap()["answers"],
        json!({ "Which database?": "SQLite" })
    );
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
    let waiting =
        tokio::spawn(
            async move { call(&agent, "wait_for_answers", json!({ "ticket": "db-6" })).await },
        );
    tokio::time::sleep(Duration::from_millis(100)).await;

    h.desk.cancel();
    let started = std::time::Instant::now();
    let result = waiting.await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(
        result.structured_content.unwrap()["status"],
        json!("cancelled")
    );
}

#[tokio::test]
async fn an_unknown_ticket_is_a_tool_error() {
    let h = start().await;
    let result = call(
        &h.agent().await,
        "wait_for_answers",
        json!({ "ticket": "nope" }),
    )
    .await;
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
    let call =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-7")).await });
    h.select("db-7").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    h.desk.submit();
    call.await.unwrap();

    let progress = progress.lock().await;
    assert!(
        progress.len() >= 3,
        "expected heartbeats every 50 ms, got {}",
        progress.len()
    );
    let message = progress[0].message.as_deref().unwrap_or_default();
    assert!(
        message.contains("db-7"),
        "heartbeat names the ticket: {message:?}"
    );
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
    assert!(
        !connected(&h),
        "the dropped call no longer counts as connected"
    );

    let agent = h.agent().await;
    let _reattached =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-8")).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(connected(&h), "reattaching connects the round again");
}

fn agent_named(name: &str, title: Option<&str>) -> Agent {
    let mut info = Implementation::new(name, "1.0");
    if let Some(title) = title {
        info = info.with_title(title);
    }
    Agent {
        client_info: Some(info),
        ..Agent::default()
    }
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
    let _a1 = ask(
        h.agent_as(agent_named("claude-code", Some("Claude Code")))
            .await,
        "a1",
        Some("shop"),
    );
    h.select("a1").await;
    let _b1 = ask(h.agent_as(agent_named("codex", None)).await, "b1", None);
    h.select("b1").await;
    let _a2 = ask(
        h.agent_as(agent_named("claude-code", Some("Claude Code")))
            .await,
        "a2",
        Some("shop"),
    );
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
            (
                "Claude Code · shop".to_owned(),
                vec!["a1".to_owned(), "a2".to_owned()]
            ),
            ("codex".to_owned(), vec!["b1".to_owned()]),
        ]
    );
}

fn three_questions(ticket: &str) -> Value {
    let options =
        json!([{ "label": "Yes", "description": "" }, { "label": "No", "description": "" }]);
    json!({ "ticket": ticket, "questions": [
        { "question": "First?", "header": "1", "options": options },
        { "question": "Second?", "header": "2", "options": options },
        { "question": "Third?", "header": "3", "options": options }
    ] })
}

fn states(desk: &Desk) -> Vec<QuestionState> {
    desk.current()
        .expect("a round is selected")
        .questions
        .iter()
        .map(|q| q.state)
        .collect()
}

#[tokio::test]
async fn the_user_moves_freely_through_a_round_and_reviews_it() {
    use QuestionState::{Answered, Current, Unanswered};
    let h = start().await;
    let agent = h.agent().await;
    let _call =
        tokio::spawn(
            async move { call(&agent, "ask_user_question", three_questions("nav")).await },
        );
    h.select("nav").await;

    assert_eq!(h.desk.current().unwrap().step, Step::Question(0));
    assert_eq!(states(&h.desk), [Current, Unanswered, Unanswered]);
    h.desk.toggle_option(0);
    h.desk.next();
    assert_eq!(states(&h.desk), [Answered, Current, Unanswered]);
    h.desk.go_to(2);
    h.desk.back();
    assert_eq!(h.desk.current().unwrap().step, Step::Question(1));
    h.desk.back();
    h.desk.back();
    assert_eq!(
        h.desk.current().unwrap().step,
        Step::Question(0),
        "back stops at the first question"
    );

    h.desk.review();
    let review = h.desk.current().unwrap();
    assert_eq!(review.step, Step::Review);
    assert_eq!(states(&h.desk), [Answered, Unanswered, Unanswered]);
    assert_eq!(review.unanswered(), ["Second?", "Third?"]);
    h.desk.next();
    assert_eq!(
        h.desk.current().unwrap().step,
        Step::Review,
        "next stops at the Review Page"
    );
    h.desk.back();
    assert_eq!(h.desk.current().unwrap().step, Step::Question(2));

    let summary = &h.desk.sessions()[0].rounds[0];
    assert_eq!((summary.answered, summary.total), (1, 3));
}

#[tokio::test]
async fn the_recommendation_is_shown_never_preselected_and_accepted_on_request() {
    let h = start().await;
    let agent = h.agent().await;
    let options =
        json!([{ "label": "Yes", "description": "" }, { "label": "No", "description": "" }]);
    let args = json!({ "ticket": "rec", "questions": [
        { "question": "Badge?", "header": "1", "options": options, "recommended": "No" },
        { "question": "Text?", "header": "2", "options": options, "recommended": "Yes, but only on Fridays" },
        { "question": "Mine?", "header": "3", "options": options, "recommended": "Yes" },
        { "question": "None?", "header": "4", "options": options }
    ] });
    let call = tokio::spawn(async move { call(&agent, "ask_user_question", args).await });
    h.select("rec").await;

    let view = h.desk.current().unwrap();
    let badge = &view.questions[0];
    assert_eq!(
        badge
            .options
            .iter()
            .map(|o| o.recommended)
            .collect::<Vec<_>>(),
        [false, true]
    );
    assert!(
        badge.options.iter().all(|o| !o.selected),
        "never pre-selected"
    );
    assert_eq!(badge.recommendation_text, None);
    assert_eq!(
        view.questions[1].recommendation_text.as_deref(),
        Some("Yes, but only on Fridays")
    );
    assert!(view.questions[1].options.iter().all(|o| !o.recommended));

    h.desk.accept_recommendation();
    h.desk.accept_recommendation();
    h.desk.toggle_option(1);
    h.desk.accept_recommendation();
    assert_eq!(
        h.desk.current().unwrap().step,
        Step::Question(3),
        "accepting moves on"
    );
    h.desk.accept_recommendation();
    h.desk.submit();

    let result = call.await.unwrap();
    assert_eq!(
        result.structured_content.unwrap()["answers"],
        json!({ "Badge?": "No", "Text?": "Yes, but only on Fridays", "Mine?": "No" })
    );
}

#[tokio::test]
async fn the_user_switches_between_rounds_without_losing_input() {
    let h = start().await;
    let ask = |ticket: &'static str| {
        let url = h.url();
        tokio::spawn(async move {
            let agent = Harness::agent_at(&url, Agent::default()).await;
            call(&agent, "ask_user_question", db_round(ticket)).await
        })
    };
    let first = ask("first");
    h.appeared("first").await;
    assert_eq!(
        h.desk.current().unwrap().ticket,
        "first",
        "an idle desk shows a new round at once"
    );
    let _second = ask("second");
    let second = h.appeared("second").await;
    assert!(
        second.new,
        "a round the user has not looked at yet is badged"
    );
    assert!(!h.desk.sessions()[0].rounds[0].new);

    h.desk.toggle_option(1);
    h.desk.next();
    h.desk.set_note("Später 🙂");
    h.desk.select("second");
    assert!(!h.appeared("second").await.new);
    h.desk.toggle_option(0);
    h.desk.select("first");

    let view = h.desk.current().unwrap();
    assert_eq!(view.step, Step::Question(1));
    assert_eq!(view.questions[0].answer.as_deref(), Some("SQLite"));
    assert_eq!(view.questions[1].note, "Später 🙂");

    h.desk.submit();
    assert_eq!(
        first.await.unwrap().structured_content.unwrap()["answers"]["Anything else?"],
        json!("Später 🙂")
    );
    let view = h.desk.current().expect("the next open round is shown");
    assert_eq!(view.ticket, "second");
    assert_eq!(view.questions[0].answer.as_deref(), Some("Postgres"));
}

#[tokio::test]
async fn only_clients_with_the_bearer_token_and_without_an_origin_get_in() {
    let h = start_with(ServerSettings {
        bearer_token: Some("s3cret".into()),
        ..ServerSettings::default()
    })
    .await;
    let http = reqwest::Client::new();
    let post = |auth: Option<&str>, origin: Option<&str>| {
        let mut request = http
            .post(h.url())
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);
        if let Some(auth) = auth {
            request = request.header("authorization", auth);
        }
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        async move {
            request
                .send()
                .await
                .expect("request is answered")
                .status()
                .as_u16()
        }
    };
    assert_eq!(post(None, None).await, 401);
    assert_eq!(post(Some("Bearer wrong"), None).await, 401);
    assert_eq!(
        post(Some("Bearer s3cret"), Some("https://evil.example")).await,
        403
    );
    assert_eq!(post(None, Some("http://127.0.0.1")).await, 403);

    let config = StreamableHttpClientTransportConfig::with_uri(h.url()).auth_header("s3cret");
    let lifecycle = ClientLifecycleMode::Discover {
        preferred_versions: vec![ProtocolVersion::V_2026_07_28],
    };
    let agent = Agent::default()
        .serve_with_lifecycle(
            StreamableHttpClientTransport::from_config(config),
            lifecycle,
        )
        .await
        .expect("an agent with the token connects");
    let result = call(&agent, "wait_for_answers", json!({ "ticket": "nope" })).await;
    assert_eq!(result.is_error, Some(true), "and can call tools");
}

#[tokio::test]
async fn the_tools_advertise_their_input_and_output_schemas() {
    let h = start().await;
    let tools = h
        .agent()
        .await
        .list_all_tools()
        .await
        .expect("tools/list works");
    let names: Vec<&str> = tools.iter().map(|t| &*t.name).collect();
    assert_eq!(names, ["ask_user_question", "wait_for_answers"]);

    let ask = &tools[0];
    let required = |schema: &serde_json::Map<String, Value>| schema["required"].clone();
    assert_eq!(required(&ask.input_schema), json!(["ticket", "questions"]));
    let question = &ask.input_schema["$defs"]["Question"];
    assert_eq!(
        question["properties"]["multiSelect"]["type"],
        json!("boolean")
    );
    assert_eq!(question["required"], json!(["question", "header"]));
    for tool in &tools {
        let output = tool
            .output_schema
            .as_ref()
            .expect("structured output is advertised");
        assert_eq!(output["required"], json!(["status", "ticket"]));
        assert_eq!(
            output["$defs"]["Status"]["enum"],
            json!(["answered", "cancelled", "pending"])
        );
    }
    let description = ask.description.as_deref().unwrap_or_default();
    for advice in ["ticket", "wait_for_answers", "body", "recommended"] {
        assert!(
            description.contains(advice),
            "the description explains `{advice}`"
        );
    }
}

#[tokio::test]
async fn closing_the_desk_cancels_every_open_round() {
    let h = start().await;
    let ask = |ticket: &'static str| {
        let url = h.url();
        tokio::spawn(async move {
            let agent = Harness::agent_at(&url, Agent::default()).await;
            call(&agent, "ask_user_question", db_round(ticket)).await
        })
    };
    let (first, second) = (ask("close-1"), ask("close-2"));
    h.appeared("close-1").await;
    h.appeared("close-2").await;
    h.desk.toggle_option(0);

    h.desk.cancel_all();

    let first = first.await.unwrap().structured_content.unwrap();
    let second = second.await.unwrap().structured_content.unwrap();
    assert_eq!(first["status"], json!("cancelled"));
    assert_eq!(first["answers"], json!({ "Which database?": "Postgres" }));
    assert_eq!(second["status"], json!("cancelled"));
    assert!(h.desk.sessions().is_empty());
    assert_eq!(h.desk.current(), None);
}

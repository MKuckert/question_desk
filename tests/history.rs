//! History: finished rounds, newest first, read-only; Awaiting Pickup rounds can be
//! reopened.

mod common;

use common::*;
use question_desk::{desk::HistoryEntry, format::Outcome};
use serde_json::json;

/// Opens `round` with a call that is dropped at once, so its result will be Awaiting
/// Pickup; leaves the round selected.
async fn orphaned(h: &Harness, round: serde_json::Value) {
    let ticket = round["ticket"].as_str().unwrap().to_owned();
    let agent = h.agent().await;
    let ask = tokio::spawn(async move { call(&agent, "ask_user_question", round).await });
    h.select(&ticket).await;
    ask.abort();
    let _ = ask.await;
    h.disconnected(&ticket).await;
}

fn tickets(history: &[HistoryEntry]) -> Vec<&str> {
    history.iter().map(|e| e.ticket.as_str()).collect()
}

#[tokio::test]
async fn finished_rounds_are_listed_newest_first_and_badged_until_delivered() {
    let h = start().await;
    orphaned(&h, db_round("old")).await;
    h.desk.submit();
    tokio::time::sleep(Duration::from_millis(5)).await;

    let agent = h.agent_as(agent_named("codex", None)).await;
    let ask = tokio::spawn(async move { call(&agent, "ask_user_question", db_round("new")).await });
    h.select("new").await;
    h.desk.cancel();
    ask.await.unwrap();

    let history = h.desk.history();
    assert_eq!(tickets(&history), ["new", "old"]);
    assert_eq!(history[0].session, "codex");
    assert_eq!(history[0].outcome, Outcome::Cancelled);
    assert!(!history[0].awaiting_pickup, "its result reached the Agent");
    assert_eq!(history[1].outcome, Outcome::Submitted);
    assert!(history[1].awaiting_pickup, "nobody received it yet");
    assert!(history[0].finished_at >= history[1].finished_at);
    assert!(history[1].finished_at >= history[1].opened_at);
    assert!(
        h.desk.sessions().is_empty(),
        "History is not among the open rounds"
    );
}

#[tokio::test]
async fn a_finished_round_shows_every_option_and_cannot_be_changed() {
    let h = start().await;
    orphaned(&h, db_round("db-1")).await;
    h.desk.toggle_option(1);
    h.desk.next();
    h.desk.set_note("tiny");
    h.desk.submit();

    h.desk.select("db-1");
    let round = h.desk.current().expect("finished rounds can be selected");
    let finished = round.finished.as_ref().expect("marked finished");
    assert_eq!(finished.outcome, Outcome::Submitted);
    assert!(finished.can_reopen);
    let labels: Vec<_> = round.questions[0]
        .options
        .iter()
        .map(|o| (o.label.as_str(), o.selected))
        .collect();
    assert_eq!(labels, [("Postgres", false), ("SQLite", true)]);
    assert_eq!(round.questions[1].answer.as_deref(), Some("tiny"));

    h.desk.toggle_option(0);
    h.desk.go_to(1);
    h.desk.set_note("changed");
    h.desk.submit();
    let after = h.desk.current().unwrap();
    assert_eq!(after.questions, round.questions);
    assert_eq!(after.step, round.step);
}

#[tokio::test]
async fn a_delivered_round_cannot_be_reopened() {
    let h = start().await;
    let agent = h.agent().await;
    let ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    h.select("db-1").await;
    h.desk.submit();
    ask.await.unwrap();

    h.desk.select("db-1");
    assert!(!h.desk.current().unwrap().finished.unwrap().can_reopen);
    h.desk.reopen();
    assert!(h.desk.sessions().is_empty());
    assert!(h.desk.current().unwrap().finished.is_some());
}

#[tokio::test]
async fn a_pickup_of_a_reopened_round_waits_for_the_new_answers() {
    let dir = tempfile::tempdir().unwrap();
    let h = start_in(dir.path()).await;
    orphaned(&h, db_round("db-1")).await;
    h.desk.toggle_option(0);
    h.desk.submit();

    h.desk.select("db-1");
    h.desk.reopen();
    let round = h.desk.current().unwrap();
    assert!(round.finished.is_none(), "open again");
    assert_eq!(h.appeared("db-1").await.answered, 1);
    assert!(h.desk.history().is_empty());

    let agent = h.agent().await;
    let pending = call(&agent, "wait_for_answers", json!({ "ticket": "db-1" })).await;
    assert_eq!(
        pending.structured_content.unwrap()["status"],
        json!("pending")
    );

    let wait =
        tokio::spawn(
            async move { call(&agent, "wait_for_answers", json!({ "ticket": "db-1" })).await },
        );
    tokio::time::sleep(Duration::from_millis(50)).await;
    h.desk.go_to(0);
    h.desk.toggle_option(1);
    h.desk.submit();
    let result = wait.await.unwrap();
    assert_eq!(
        result.structured_content.unwrap()["answers"],
        json!({ "Which database?": "SQLite" })
    );
    let file = std::fs::read_to_string(dir.path().join("rounds/db-1.json")).unwrap();
    assert!(file.contains("\"SQLite\""), "{file}");
}

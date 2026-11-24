//! Delivery releases a Ticket; asking for a Delivered result again is a Re-delivery
//! Request the user decides on (ADR 0007).

mod common;

use common::*;
use question_desk::desk::RedeliveryView;
use serde_json::json;

/// Asks `round`, has the user pick the first Option and submit, and returns the result
/// the Agent received: the round is Delivered.
async fn delivered(h: &Harness, round: serde_json::Value) -> rmcp::model::CallToolResult {
    let ticket = round["ticket"].as_str().unwrap().to_owned();
    let agent = h.agent().await;
    let ask = tokio::spawn(async move { call(&agent, "ask_user_question", round).await });
    h.select(&ticket).await;
    h.desk.toggle_option(0);
    h.desk.submit();
    ask.await.unwrap()
}

async fn requested(h: &Harness) -> RedeliveryView {
    for _ in 0..200 {
        if let Some(request) = h.desk.redeliveries().into_iter().next() {
            return request;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no Re-delivery Request appeared");
}

#[tokio::test]
async fn a_delivered_ticket_with_different_questions_opens_a_new_round() {
    let dir = tempfile::tempdir().unwrap();
    let h = start_in(dir.path()).await;
    delivered(&h, db_round("db-1")).await;
    let old_file = dir.path().join("rounds").join("db-1.json");
    let old = std::fs::read_to_string(&old_file).unwrap();
    assert!(old.contains("\"delivered_at\""), "{old}");

    let mut other = db_round("db-1");
    other["questions"][0]["question"] = json!("Which cache?");
    let agent = h.agent().await;
    let ask = tokio::spawn(async move { call(&agent, "ask_user_question", other).await });
    let round = h.appeared("db-1").await;
    assert_ne!(round.id, "db-1");
    h.desk.select(&round.id);
    assert_eq!(
        h.desk.current().unwrap().questions[0].question,
        "Which cache?"
    );
    h.desk.cancel();
    assert_eq!(
        ask.await.unwrap().structured_content.unwrap()["status"],
        json!("cancelled")
    );
    assert_eq!(std::fs::read_to_string(&old_file).unwrap(), old);
}

#[tokio::test]
async fn the_user_delivers_a_result_again_on_request() {
    let h = start().await;
    let first = delivered(&h, db_round("db-1")).await;

    let agent = h.agent_as(agent_named("codex", None)).await;
    let again =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    let request = requested(&h).await;
    assert_eq!(request.ticket, "db-1");
    assert_eq!(request.title, "Storage");
    assert_eq!(request.requesters, vec!["codex".to_owned()]);
    assert!(h.desk.sessions().is_empty(), "no new round was opened");

    h.desk.deliver_again(&request.id);
    let again = again.await.unwrap();
    assert_eq!(again.structured_content, first.structured_content);
    assert_eq!(text(&again), text(&first));
    assert!(h.desk.redeliveries().is_empty());
}

#[tokio::test]
async fn a_declined_redelivery_tells_the_agent_to_ask_with_a_new_ticket() {
    let h = start().await;
    delivered(&h, db_round("db-1")).await;

    let agent = h.agent().await;
    let again =
        tokio::spawn(
            async move { call(&agent, "wait_for_answers", json!({ "ticket": "db-1" })).await },
        );
    let request = requested(&h).await;
    h.desk.decline_redelivery(&request.id);
    let again = again.await.unwrap();
    assert_eq!(again.is_error, Some(true));
    assert_eq!(
        text(&again),
        "the user declined re-delivery of ticket \"db-1\"; ask again with a new ticket or clarify with the user first."
    );
}

#[tokio::test]
async fn requests_fold_into_one_and_wait_for_answers_stays_pending_until_decided() {
    let h = start().await;
    let first = delivered(&h, db_round("db-1")).await;

    let asking = h
        .agent_as(agent_named("claude-code", Some("Claude Code")))
        .await;
    let ask =
        tokio::spawn(async move { call(&asking, "ask_user_question", db_round("db-1")).await });
    requested(&h).await;
    let waiting = h.agent_as(agent_named("codex", None)).await;
    let pending = call(&waiting, "wait_for_answers", json!({ "ticket": "db-1" })).await;
    let output = pending.structured_content.clone().unwrap();
    assert_eq!(output["status"], json!("pending"));
    assert_eq!(output["title"], json!("Storage"));
    assert!(text(&pending).contains("deliver"), "{}", text(&pending));

    let requests = h.desk.redeliveries();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].requesters, vec!["Claude Code", "codex"]);

    let wait = tokio::spawn(async move {
        call(&waiting, "wait_for_answers", json!({ "ticket": "db-1" })).await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    h.desk.deliver_again(&requests[0].id);
    let (ask, wait) = (ask.await.unwrap(), wait.await.unwrap());
    assert_eq!(ask.structured_content, first.structured_content);
    assert_eq!(wait.structured_content, first.structured_content);
}

#[tokio::test]
async fn a_result_nobody_received_is_picked_up_without_asking() {
    let h = start().await;
    let dropped = {
        let agent = h.agent().await;
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await })
    };
    h.select("db-1").await;
    dropped.abort();
    let _ = dropped.await;
    h.disconnected("db-1").await;
    h.desk.toggle_option(1);
    h.desk.submit();

    let agent = h.agent().await;
    let picked = call(&agent, "ask_user_question", db_round("db-1")).await;
    assert_eq!(
        picked.structured_content.unwrap()["status"],
        json!("answered")
    );
    assert!(h.desk.redeliveries().is_empty());
    // Now it is Delivered: the next Pickup asks the user.
    let agent = h.agent().await;
    let _again =
        tokio::spawn(
            async move { call(&agent, "wait_for_answers", json!({ "ticket": "db-1" })).await },
        );
    requested(&h).await;
}

#[tokio::test]
async fn delivery_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let h = start_in(dir.path()).await;
    delivered(&h, db_round("db-1")).await;
    h.desk.close();
    drop(h);

    let h = start_in(dir.path()).await;
    let agent = h.agent().await;
    let _again =
        tokio::spawn(
            async move { call(&agent, "wait_for_answers", json!({ "ticket": "db-1" })).await },
        );
    requested(&h).await;
}

/// One `wait_for_answers` call that ends `pending` after the short pickup wait.
async fn poll(
    agent: &rmcp::service::RunningService<rmcp::RoleClient, Agent>,
) -> rmcp::model::CallToolResult {
    call(agent, "wait_for_answers", json!({ "ticket": "db-1" })).await
}

#[tokio::test]
async fn a_decision_reaches_a_listed_agent_between_two_polls() {
    let h = start().await;
    let first = delivered(&h, db_round("db-1")).await;
    let codex = h.agent_as(agent_named("codex", None)).await;
    let pending = poll(&codex).await;
    assert_eq!(
        pending.structured_content.unwrap()["status"],
        json!("pending")
    );

    // No call waits while the user decides.
    let request = requested(&h).await;
    h.desk.deliver_again(&request.id);
    assert!(h.desk.redeliveries().is_empty());
    assert_eq!(
        poll(&codex).await.structured_content,
        first.structured_content
    );
}

#[tokio::test]
async fn a_decision_without_a_waiting_call_is_not_given_to_another_agent() {
    let h = start().await;
    delivered(&h, db_round("db-1")).await;
    let codex = h.agent_as(agent_named("codex", None)).await;
    poll(&codex).await;
    h.desk.deliver_again(&requested(&h).await.id);

    let other = h.agent_as(agent_named("other", None)).await;
    let pending = poll(&other).await;
    assert_eq!(
        pending.structured_content.unwrap()["status"],
        json!("pending")
    );
    assert_eq!(requested(&h).await.requesters, vec!["other".to_owned()]);
}

#[tokio::test]
async fn a_decision_while_one_call_waits_also_answers_the_agent_still_polling() {
    let h = start().await;
    delivered(&h, db_round("db-1")).await;
    let codex = h.agent_as(agent_named("codex", None)).await;
    poll(&codex).await;
    let asking = h.agent_as(agent_named("claude", None)).await;
    let ask =
        tokio::spawn(async move { call(&asking, "ask_user_question", db_round("db-1")).await });
    for _ in 0..200 {
        if requested(&h).await.requesters.len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    h.desk.decline_redelivery(&requested(&h).await.id);
    assert_eq!(ask.await.unwrap().is_error, Some(true));
    let again = poll(&codex).await;
    assert_eq!(again.is_error, Some(true), "declined, not a new request");
    assert!(h.desk.redeliveries().is_empty());
}

#[tokio::test]
async fn a_decision_on_an_ask_with_a_source_answers_the_same_agents_poll() {
    let h = start().await;
    delivered(&h, db_round("db-1")).await;
    let codex = std::sync::Arc::new(h.agent_as(agent_named("codex", None)).await);
    let mut round = db_round("db-1");
    round["source"] = json!("webshop");
    let asking = codex.clone();
    let ask = tokio::spawn(async move { call(&asking, "ask_user_question", round).await });
    let request = requested(&h).await;
    assert_eq!(request.requesters, vec!["codex · webshop".to_owned()]);

    h.desk.decline_redelivery(&request.id);
    assert_eq!(ask.await.unwrap().is_error, Some(true));
    let again = poll(&codex).await;
    assert_eq!(again.is_error, Some(true), "declined, not a new request");
    assert!(h.desk.redeliveries().is_empty());
}

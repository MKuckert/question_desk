//! The filter over open rounds and History, on the `desk` read model.

mod common;

use common::*;
use serde_json::json;

fn cache_round() -> serde_json::Value {
    json!({
        "ticket": "cache-7f3",
        "title": "Caching",
        "source": "webshop",
        "questions": [{
            "question": "Which cache?",
            "header": "Cache",
            "body": "Sessions need *eviction*.",
            "options": [
                { "label": "Redis", "description": "In-memory store", "preview": "maxmemory-policy volatile-lru" },
                { "label": "Memcached", "description": "Simple and fast" }
            ]
        }]
    })
}

/// One finished round ("db-1", noted "tiny footprint") in History and one open round
/// ("cache-7f3", drafted "zstd") from another Agent Session.
async fn two_rounds() -> Harness {
    let h = start().await;
    let agent = h.agent().await;
    let ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    h.select("db-1").await;
    h.desk.toggle_option(1);
    h.desk.set_note("tiny footprint");
    h.desk.submit();
    ask.await.unwrap();

    let agent = h.agent_as(agent_named("codex", None)).await;
    let _ask = tokio::spawn(async move { call(&agent, "ask_user_question", cache_round()).await });
    h.select("cache-7f3").await;
    h.desk.set_note("zstd compression");
    h
}

/// Tickets of the rounds the desk shows with this filter: open ones, then History.
fn shown(h: &Harness, filter: &str) -> Vec<String> {
    h.desk.set_filter(filter);
    let open = h
        .desk
        .sessions()
        .into_iter()
        .flat_map(|s| s.rounds)
        .map(|r| r.ticket);
    let history = h.desk.history().into_iter().map(|e| e.ticket);
    open.chain(history).collect()
}

#[tokio::test]
async fn every_searchable_field_matches_case_insensitively() {
    let h = two_rounds().await;
    for (filter, expected) in [
        ("", vec!["cache-7f3", "db-1"]),
        ("storage", vec!["db-1"]),        // title
        ("CODEX", vec!["cache-7f3"]),     // Agent Session
        ("webshop", vec!["cache-7f3"]),   // Agent Session source
        ("7f3", vec!["cache-7f3"]),       // Ticket
        ("which database", vec!["db-1"]), // question text
        ("other", vec!["db-1"]),          // header
        ("eviction", vec!["cache-7f3"]),  // body
        ("memcached", vec!["cache-7f3"]), // Option label
        ("relational", vec!["db-1"]),     // Option description
        ("footprint", vec!["db-1"]),      // Note of a finished round
        ("zstd", vec!["cache-7f3"]),      // draft Note of an open round
        ("nothing like this", vec![]),
    ] {
        assert_eq!(shown(&h, filter), expected, "filter {filter:?}");
    }
}

#[tokio::test]
async fn all_terms_must_match_within_one_round() {
    let h = two_rounds().await;
    assert_eq!(shown(&h, "  redis   eviction "), ["cache-7f3"]);
    assert_eq!(shown(&h, "redis postgres"), Vec::<String>::new());
}

#[tokio::test]
async fn previews_do_not_match() {
    let h = two_rounds().await;
    assert_eq!(shown(&h, "volatile-lru"), Vec::<String>::new());
}

#[tokio::test]
async fn the_opened_round_marks_the_hits() {
    let h = two_rounds().await;
    h.desk.set_filter("REDIS evic");
    assert_eq!(h.desk.filter_terms(), ["redis", "evic"]);
    let hits = question_desk::search::highlights("Use Redis for eviction", &h.desk.filter_terms());
    assert_eq!(hits, [4..9, 14..18]);
    let terms = question_desk::search::terms("ärg");
    assert_eq!(
        question_desk::search::highlights("Viel ÄRGER", &terms),
        vec![5..9]
    );
}

#[tokio::test]
async fn the_filter_hides_rounds_but_not_from_the_close_guard_or_clearing_history() {
    let h = two_rounds().await;
    assert!(shown(&h, "nothing-matches").is_empty());
    assert!(h.desk.has_open(), "closing still asks before cancelling");
    assert_eq!(h.desk.finished_count(), 1, "clearing History counts all");
}

//! The round file format: one JSON file per Question Round (ADR 0007).

use question_desk::{
    schema::{Question, QuestionOption},
    store::{self, DraftAnswer, LoadError, Status, Store, StoredRound},
};
use serde_json::json;

fn round(id: &str) -> StoredRound {
    StoredRound {
        id: id.to_owned(),
        ticket: "db-1".to_owned(),
        session: "Claude Code · repo".to_owned(),
        title: Some("Storage".to_owned()),
        questions: vec![Question {
            question: "Which database?".to_owned(),
            header: "Database".to_owned(),
            body: Some("We need **transactions**.".to_owned()),
            options: vec![
                QuestionOption {
                    label: "Postgres".to_owned(),
                    description: "Relational".to_owned(),
                    preview: None,
                },
                QuestionOption {
                    label: "SQLite".to_owned(),
                    description: "Embedded".to_owned(),
                    preview: Some("CREATE TABLE t;".to_owned()),
                },
            ],
            multi_select: false,
            recommended: Some("Postgres".to_owned()),
        }],
        answers: vec![DraftAnswer {
            selected: vec!["SQLite".to_owned()],
            note: "small data".to_owned(),
        }],
        step: None,
        status: Status::Answered,
        opened_at: "2026-11-23T10:00:00Z".parse().unwrap(),
        finished_at: Some("2026-11-23T10:05:00Z".parse().unwrap()),
        delivered_at: None,
    }
}

#[test]
fn a_round_reads_back_as_written() {
    let round = round("db-1");
    let text = store::to_json(&round);
    assert_eq!(store::from_json(&text).unwrap(), round);
}

#[test]
fn the_file_is_versioned_json_with_readable_fields() {
    let value: serde_json::Value = serde_json::from_str(&store::to_json(&round("db-1"))).unwrap();
    assert_eq!(value["version"], json!(1));
    assert_eq!(value["status"], json!("answered"));
    assert_eq!(value["opened_at"], json!("2026-11-23T10:00:00Z"));
    assert_eq!(value["answers"][0]["selected"], json!(["SQLite"]));
    assert_eq!(value["questions"][0]["multiSelect"], json!(false));
}

#[test]
fn a_version_1_file_is_read_through_the_migration_hook() {
    let text = r#"{
        "version": 1,
        "id": "t",
        "ticket": "t",
        "session": "Agent",
        "questions": [{ "question": "Q?", "header": "H" }],
        "answers": [{ "selected": [], "note": "draft" }],
        "status": "open",
        "opened_at": "2026-11-23T10:00:00Z"
    }"#;
    let round = store::from_json(text).unwrap();
    assert_eq!(round.status, Status::Open);
    assert_eq!(round.answers[0].note, "draft");
    assert_eq!(round.title, None);
    assert_eq!(round.finished_at, None);
}

#[test]
fn newer_and_unparseable_files_are_told_apart() {
    assert!(matches!(
        store::from_json(r#"{ "version": 2, "anything": true }"#),
        Err(LoadError::Newer(2))
    ));
    assert!(matches!(
        store::from_json("{ not json"),
        Err(LoadError::Unreadable(_))
    ));
    assert!(matches!(
        store::from_json(r#"{ "version": 1 }"#),
        Err(LoadError::Unreadable(_))
    ));
}

#[test]
fn the_store_saves_loads_and_skips_bad_files_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    store.save(&round("db-1")).unwrap();
    let rounds = store.rounds_dir();
    std::fs::write(rounds.join("broken.json"), "{ nope").unwrap();
    std::fs::write(rounds.join("future.json"), r#"{"version": 99}"#).unwrap();

    let loaded = store.load();
    assert_eq!(loaded.rounds, vec![round("db-1")]);
    assert_eq!(loaded.skipped.len(), 2);
    assert_eq!(
        std::fs::read_to_string(rounds.join("broken.json")).unwrap(),
        "{ nope"
    );
    assert_eq!(
        std::fs::read_to_string(rounds.join("db-1.json")).unwrap(),
        store::to_json(&round("db-1"))
    );
}

#[cfg(unix)]
#[test]
fn only_the_owner_may_read_the_store() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    store.save(&round("db-1")).unwrap();
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&store.rounds_dir()), 0o700);
    assert_eq!(mode(&store.rounds_dir().join("db-1.json")), 0o600);
}

#[test]
fn a_round_no_agent_could_have_asked_is_unreadable() {
    let mut empty = round("empty");
    empty.questions.clear();
    empty.answers.clear();
    assert!(matches!(
        store::from_json(&store::to_json(&empty)),
        Err(LoadError::Unreadable(e)) if e.contains("at least one question")
    ));
}

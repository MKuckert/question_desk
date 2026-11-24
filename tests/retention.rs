//! The user's control over stored rounds: retention, `data_dir`, the persistence switch,
//! and deleting rounds (ADR 0007).

mod common;

use std::path::Path;

use common::*;
use question_desk::{
    desk::StorageSettings,
    housekeeping,
    schema::Question,
    store::{DraftAnswer, Status, Store, StoredRound},
};
use serde_json::json;

/// A stored round with Ticket `ticket` that finished `finished_days_ago` days ago, or is
/// still open if `None`.
fn stored(ticket: &str, finished_days_ago: Option<i64>) -> StoredRound {
    let days_ago = |d: i64| jiff::Timestamp::now() - jiff::SignedDuration::from_hours(24 * d);
    StoredRound {
        id: ticket.to_owned(),
        ticket: ticket.to_owned(),
        session: "Agent".to_owned(),
        title: None,
        questions: vec![Question {
            question: "Q?".to_owned(),
            header: "H".to_owned(),
            body: None,
            options: vec![],
            multi_select: false,
            recommended: None,
        }],
        answers: vec![DraftAnswer::default()],
        step: Some(0),
        status: if finished_days_ago.is_some() {
            Status::Answered
        } else {
            Status::Open
        },
        opened_at: days_ago(400),
        finished_at: finished_days_ago.map(days_ago),
        delivered_at: finished_days_ago.map(days_ago),
    }
}

/// A data dir with rounds finished 100 and 10 days ago, an open one from long ago and a
/// broken file.
fn aged_store(dir: &Path) {
    let store = Store::new(dir);
    store.save(&stored("old", Some(100))).unwrap();
    store.save(&stored("recent", Some(10))).unwrap();
    store.save(&stored("open", None)).unwrap();
    std::fs::write(store.rounds_dir().join("broken.json"), "{").unwrap();
}

fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.join("rounds"))
        .map(|entries| {
            entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn history(h: &Harness) -> Vec<String> {
    h.desk.history().into_iter().map(|e| e.ticket).collect()
}

fn write_config(path: &Path, data_dir: &Path, extra: &str) {
    std::fs::write(
        path,
        format!("data_dir = {:?}\n{extra}\n", data_dir.display().to_string()),
    )
    .unwrap();
}

#[tokio::test]
async fn finished_rounds_older_than_the_retention_are_pruned() {
    let data = tempfile::tempdir().unwrap();
    aged_store(data.path());
    let config = data.path().join("config.toml");
    write_config(&config, data.path(), "retention_days = 30");
    let h = start_in(data.path()).await;

    housekeeping::run(&h.desk, &config).unwrap();
    assert_eq!(
        files(data.path()),
        ["broken.json", "open.json", "recent.json"]
    );
    assert_eq!(history(&h), ["recent"]);
    assert_eq!(
        h.appeared("open").await.ticket,
        "open",
        "open rounds are never pruned"
    );
}

#[tokio::test]
async fn zero_days_keeps_rounds_forever() {
    let data = tempfile::tempdir().unwrap();
    aged_store(data.path());
    let config = data.path().join("config.toml");
    write_config(&config, data.path(), "retention_days = 0");
    let h = start_in(data.path()).await;

    housekeeping::run(&h.desk, &config).unwrap();
    assert_eq!(files(data.path()).len(), 4);
    assert_eq!(history(&h), ["recent", "old"]);
}

#[tokio::test]
async fn the_config_is_read_again_before_each_prune() {
    let data = tempfile::tempdir().unwrap();
    aged_store(data.path());
    let config = data.path().join("config.toml");
    write_config(&config, data.path(), "retention_days = 365");
    let h = start_in(data.path()).await;
    housekeeping::run(&h.desk, &config).unwrap();
    assert_eq!(history(&h), ["recent", "old"]);

    write_config(&config, data.path(), "retention_days = 30");
    housekeeping::run(&h.desk, &config).unwrap();
    assert_eq!(history(&h), ["recent"]);

    std::fs::write(&config, "retention_days = 'five'").unwrap();
    assert!(
        housekeeping::run(&h.desk, &config).is_err(),
        "a broken config prunes nothing"
    );
    assert_eq!(history(&h), ["recent"]);
}

#[tokio::test]
async fn a_new_data_dir_gets_the_rounds_and_the_old_one_is_left_alone() {
    let old = tempfile::tempdir().unwrap();
    let new = tempfile::tempdir().unwrap();
    let h = start_in(old.path()).await;
    let agent = h.agent().await;
    let ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    h.select("db-1").await;
    let before = std::fs::read_to_string(old.path().join("rounds/db-1.json")).unwrap();

    let config = old.path().join("config.toml");
    write_config(&config, new.path(), "");
    housekeeping::run(&h.desk, &config).unwrap();
    assert_eq!(files(new.path()), ["db-1.json"]);

    h.desk.toggle_option(0);
    h.desk.submit();
    ask.await.unwrap();
    let moved = std::fs::read_to_string(new.path().join("rounds/db-1.json")).unwrap();
    assert!(moved.contains("\"answered\""), "{moved}");
    assert_eq!(
        std::fs::read_to_string(old.path().join("rounds/db-1.json")).unwrap(),
        before
    );
}

#[tokio::test]
async fn switching_persistence_off_stops_writing_and_offers_to_delete() {
    let data = tempfile::tempdir().unwrap();
    aged_store(data.path());
    let h = start_in(data.path()).await;

    h.desk.apply_storage(StorageSettings {
        persistence: false,
        data_dir: Some(data.path().to_owned()),
    });
    let offer = h.desk.take_delete_offer().expect("an offer to delete");
    assert_eq!(
        offer.count, 3,
        "the broken file is not one of the stored rounds"
    );
    assert!(h.desk.take_delete_offer().is_none(), "offered once");

    let agent = h.agent().await;
    let _ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    h.select("db-1").await;
    h.desk.set_note("not written");
    h.desk.flush();
    assert!(!files(data.path()).contains(&"db-1.json".to_owned()));

    h.desk.delete_stored(&offer);
    assert_eq!(files(data.path()), ["broken.json"]);
    assert_eq!(
        history(&h),
        ["recent", "old"],
        "this session still shows them"
    );
    assert_eq!(h.desk.sessions().len(), 2, "open rounds stay too");
}

#[tokio::test]
async fn a_hand_edit_switching_persistence_off_is_picked_up() {
    let data = tempfile::tempdir().unwrap();
    aged_store(data.path());
    let config = data.path().join("config.toml");
    write_config(&config, data.path(), "persistence = false");
    let h = start_in(data.path()).await;
    housekeeping::run(&h.desk, &config).unwrap();
    assert!(!h.desk.is_persistent());
    assert_eq!(h.desk.take_delete_offer().unwrap().count, 3);
    assert_eq!(
        files(data.path()).len(),
        4,
        "nothing pruned or deleted without asking"
    );
}

#[tokio::test]
async fn deleting_an_open_round_cancels_it_first() {
    let data = tempfile::tempdir().unwrap();
    let h = start_in(data.path()).await;
    let agent = h.agent().await;
    let ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    h.select("db-1").await;
    h.desk.toggle_option(0);

    h.desk.delete("db-1");
    let result = ask.await.unwrap();
    assert_eq!(
        result.structured_content.unwrap()["status"],
        json!("cancelled")
    );
    assert!(h.desk.sessions().is_empty());
    assert!(h.desk.history().is_empty());
    assert!(h.desk.current().is_none());
    assert!(files(data.path()).is_empty());
}

#[tokio::test]
async fn a_whole_agent_session_or_all_history_can_be_deleted() {
    let data = tempfile::tempdir().unwrap();
    aged_store(data.path());
    let h = start_in(data.path()).await;
    let agent = h.agent_as(agent_named("codex", None)).await;
    let ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    h.appeared("db-1").await;

    h.desk.delete_session("codex");
    assert_eq!(
        ask.await.unwrap().structured_content.unwrap()["status"],
        json!("cancelled")
    );
    assert_eq!(
        files(data.path()),
        ["broken.json", "old.json", "open.json", "recent.json"]
    );

    h.desk.clear_history();
    assert!(h.desk.history().is_empty());
    assert_eq!(files(data.path()), ["broken.json", "open.json"]);
    assert_eq!(h.appeared("open").await.ticket, "open");
}

#[tokio::test]
async fn a_delete_answers_a_pending_redelivery_request_with_a_decline() {
    let h = start().await;
    let agent = h.agent().await;
    let ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    h.select("db-1").await;
    h.desk.submit();
    ask.await.unwrap();
    let agent = h.agent().await;
    let again =
        tokio::spawn(
            async move { call(&agent, "wait_for_answers", json!({ "ticket": "db-1" })).await },
        );
    for _ in 0..200 {
        if !h.desk.redeliveries().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    h.desk.delete("db-1");
    assert_eq!(again.await.unwrap().is_error, Some(true));
}

fn persistence(h: &Harness, on: bool, data_dir: &Path) {
    h.desk.apply_storage(StorageSettings {
        persistence: on,
        data_dir: Some(data_dir.to_owned()),
    });
}

#[tokio::test]
async fn a_new_data_dir_keeps_its_files_and_takes_rounds_under_free_names() {
    let old = tempfile::tempdir().unwrap();
    let new = tempfile::tempdir().unwrap();
    let theirs = Store::new(new.path());
    theirs.save(&stored("other", Some(1))).unwrap();
    std::fs::write(theirs.path_of("db-1"), "{").unwrap();
    let other = std::fs::read_to_string(theirs.path_of("other")).unwrap();

    let h = start_in(old.path()).await;
    let agent = h.agent().await;
    let ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    h.select("db-1").await;
    persistence(&h, true, new.path());

    assert_eq!(
        files(new.path()),
        ["db-1.json", "db-1@2.json", "other.json"]
    );
    assert_eq!(
        std::fs::read_to_string(theirs.path_of("db-1")).unwrap(),
        "{"
    );
    assert_eq!(
        std::fs::read_to_string(theirs.path_of("other")).unwrap(),
        other
    );
    assert!(history(&h).is_empty(), "rounds stored there are not loaded");

    h.desk.toggle_option(0);
    h.desk.submit();
    assert_eq!(
        ask.await.unwrap().structured_content.unwrap()["status"],
        json!("answered")
    );
    let moved = std::fs::read_to_string(theirs.path_of("db-1@2")).unwrap();
    assert!(moved.contains("\"delivered_at\""), "{moved}");
    assert_eq!(
        std::fs::read_to_string(theirs.path_of("db-1")).unwrap(),
        "{"
    );
}

#[tokio::test]
async fn a_round_opened_while_saving_was_off_never_overwrites_a_skipped_file() {
    let data = tempfile::tempdir().unwrap();
    let store = Store::new(data.path());
    std::fs::create_dir_all(store.rounds_dir()).unwrap();
    std::fs::write(store.path_of("db-1"), "{").unwrap();
    let h = start_in(data.path()).await;
    persistence(&h, false, data.path());

    let agent = h.agent().await;
    let _ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    h.select("db-1").await;
    persistence(&h, true, data.path());

    assert_eq!(files(data.path()), ["db-1.json", "db-1@2.json"]);
    assert_eq!(std::fs::read_to_string(store.path_of("db-1")).unwrap(), "{");
    assert_eq!(h.desk.current().unwrap().id, "db-1@2", "still selected");
}

#[tokio::test]
async fn loading_at_startup_rewrites_no_file() {
    let data = tempfile::tempdir().unwrap();
    let store = Store::new(data.path());
    store.save(&stored("old", Some(1))).unwrap();
    let pretty = std::fs::read_to_string(store.path_of("old")).unwrap();
    let value: serde_json::Value = serde_json::from_str(&pretty).unwrap();
    let compact = serde_json::to_string(&value).unwrap();
    std::fs::write(store.path_of("old"), &compact).unwrap();

    // As at app startup (`main`).
    let desk = question_desk::desk::Desk::new(StorageSettings {
        persistence: true,
        data_dir: Some(data.path().to_owned()),
    });
    let h = serve(desk, fast_settings()).await;
    let config = data.path().join("config.toml");
    write_config(&config, data.path(), "retention_days = 30");
    housekeeping::run(&h.desk, &config).unwrap();
    h.desk.flush();
    assert_eq!(history(&h), ["old"]);
    assert_eq!(
        std::fs::read_to_string(store.path_of("old")).unwrap(),
        compact
    );
}

#[tokio::test]
async fn a_round_deleted_while_saving_is_off_stays_deleted() {
    let data = tempfile::tempdir().unwrap();
    aged_store(data.path());
    let h = start_in(data.path()).await;
    persistence(&h, false, data.path());

    h.desk.delete("recent");
    assert!(!files(data.path()).contains(&"recent.json".to_owned()));
    persistence(&h, true, data.path());
    assert_eq!(history(&h), ["old"]);

    h.desk.close();
    drop(h);
    let h = start_in(data.path()).await;
    assert_eq!(history(&h), ["old"]);
}

#[tokio::test]
async fn a_delete_offer_deletes_nothing_once_saving_is_back_on() {
    let data = tempfile::tempdir().unwrap();
    aged_store(data.path());
    let h = start_in(data.path()).await;
    persistence(&h, false, data.path());
    let offer = h.desk.take_delete_offer().unwrap();
    assert!(h.desk.can_delete_stored(&offer));

    let agent = h.agent().await;
    let _ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    h.select("db-1").await;
    persistence(&h, true, data.path());
    assert!(!h.desk.can_delete_stored(&offer));

    h.desk.delete_stored(&offer);
    assert_eq!(
        files(data.path()),
        [
            "broken.json",
            "db-1.json",
            "old.json",
            "open.json",
            "recent.json"
        ]
    );
}

#[tokio::test]
async fn a_delete_offer_deletes_only_the_files_it_counted() {
    let data = tempfile::tempdir().unwrap();
    aged_store(data.path());
    let h = start_in(data.path()).await;
    persistence(&h, false, data.path());
    let offer = h.desk.take_delete_offer().unwrap();
    // Written by hand (or another build) after the offer was made.
    Store::new(data.path())
        .save(&stored("later", Some(1)))
        .unwrap();

    h.desk.delete_stored(&offer);
    assert_eq!(files(data.path()), ["broken.json", "later.json"]);
}

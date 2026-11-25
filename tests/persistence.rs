//! Persisted Question Rounds through the headless Question Desk on a temp data dir. A
//! "restart" drops desk and server (flushing like a quit) and starts new ones on the
//! same dir.

mod common;

use common::*;
use question_desk::desk::Step;
use serde_json::json;

/// Quits the app: drafts are flushed and the data dir released, then desk and server go
/// away.
fn quit(h: Harness) {
    h.desk.close();
    drop(h);
}

#[tokio::test]
async fn open_rounds_and_their_drafts_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let h = start_in(dir.path()).await;
    let agent = h.agent().await;
    let ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    h.select("db-1").await;
    h.desk.toggle_option(1);
    h.desk.next();
    h.desk.set_note("Keep it simple");
    let opened_at = h.appeared("db-1").await.opened_at;
    quit(h);
    ask.abort();

    let h = start_in(dir.path()).await;
    let summary = h.appeared("db-1").await;
    assert!(!summary.connected, "a restored round has no Agent waiting");
    assert_eq!(summary.answered, 2);
    assert_eq!(
        summary
            .opened_at
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
        opened_at
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    h.desk.select("db-1");
    let round = h.desk.current().unwrap();
    assert_eq!(round.step, Step::Question(1));
    assert_eq!(round.questions[1].note, "Keep it simple");
    assert!(round.questions[0].options[1].selected);
    assert!(!round.connected);
}

#[tokio::test]
async fn drafts_are_written_about_a_second_after_the_last_edit() {
    let dir = tempfile::tempdir().unwrap();
    let h = start_in(dir.path()).await;
    let agent = h.agent().await;
    let _ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    h.select("db-1").await;
    h.desk.set_note("draft");
    let file = dir.path().join("rounds").join("db-1.json");
    assert!(!std::fs::read_to_string(&file).unwrap().contains("draft"));
    tokio::time::sleep(Duration::from_millis(1600)).await;
    assert!(std::fs::read_to_string(&file).unwrap().contains("draft"));
}

#[tokio::test]
async fn a_restored_open_round_is_answered_through_a_reattached_call() {
    let dir = tempfile::tempdir().unwrap();
    let h = start_in(dir.path()).await;
    let agent = h.agent().await;
    let ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    h.appeared("db-1").await;
    quit(h);
    ask.abort();

    let h = start_in(dir.path()).await;
    let agent = h.agent().await;
    let ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    for _ in 0..200 {
        if h.appeared("db-1").await.connected {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    h.desk.select("db-1");
    h.desk.toggle_option(0);
    h.desk.submit();
    let result = ask.await.unwrap();
    assert_eq!(
        result.structured_content.unwrap()["answers"],
        json!({ "Which database?": "Postgres" })
    );
}

#[tokio::test]
async fn the_agent_picks_up_a_result_submitted_before_the_restart() {
    let dir = tempfile::tempdir().unwrap();
    let h = start_in(dir.path()).await;
    let agent = h.agent().await;
    let ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    h.select("db-1").await;
    ask.abort();
    // The call is gone before the user submits: the result is Awaiting Pickup.
    h.disconnected("db-1").await;
    h.desk.toggle_option(1);
    h.desk.submit();
    quit(h);

    let h = start_in(dir.path()).await;
    let agent = h.agent().await;
    let result = call(&agent, "wait_for_answers", json!({ "ticket": "db-1" })).await;
    assert_eq!(result.is_error, Some(false), "{}", text(&result));
    let output = result.structured_content.unwrap();
    assert_eq!(output["status"], json!("answered"));
    assert_eq!(output["answers"], json!({ "Which database?": "SQLite" }));
}

#[tokio::test]
async fn corrupt_and_newer_files_are_skipped_and_left_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let rounds = dir.path().join("rounds");
    std::fs::create_dir_all(&rounds).unwrap();
    let broken = rounds.join("db-1.json");
    let newer = rounds.join("future.json");
    std::fs::write(&broken, "{ half a file").unwrap();
    let newer_text = r#"{ "version": 7, "id": "future", "wings": true }"#;
    std::fs::write(&newer, newer_text).unwrap();

    let h = start_in(dir.path()).await;
    assert!(h.desk.sessions().is_empty());
    // A new round with the broken file's Ticket must not overwrite it.
    let agent = h.agent().await;
    let _ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    h.select("db-1").await;
    h.desk.cancel();
    quit(h);

    assert_eq!(std::fs::read_to_string(&broken).unwrap(), "{ half a file");
    assert_eq!(std::fs::read_to_string(&newer).unwrap(), newer_text);
    let files = std::fs::read_dir(&rounds).unwrap().count();
    assert_eq!(files, 3, "the new round got a file of its own");
}

#[tokio::test]
async fn a_failed_write_is_tried_again_until_it_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let h = start_in(dir.path()).await;
    let agent = h.agent().await;
    let _ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    h.select("db-1").await;
    // A directory in the file's place makes the write fail.
    let file = dir.path().join("rounds").join("db-1.json");
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    std::fs::write(file.join("blocker"), "").unwrap();
    h.desk.set_note("draft");
    assert_eq!(h.desk.flush(), 1, "one round is not written");
    assert!(file.is_dir());

    std::fs::remove_dir_all(&file).unwrap();
    assert_eq!(h.desk.flush(), 0);
    assert!(std::fs::read_to_string(&file).unwrap().contains("draft"));
}

#[tokio::test]
async fn a_second_desk_on_the_same_data_dir_keeps_its_rounds_in_memory() {
    let dir = tempfile::tempdir().unwrap();
    let first = start_in(dir.path()).await;
    let second = start_in(dir.path()).await;
    assert!(!second.desk.is_persistent());
    let unavailable = second.desk.take_storage_unavailable().unwrap();
    assert_eq!(unavailable.data_dir, dir.path());

    let agent = second.agent().await;
    let _ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    second.select("db-1").await;
    second.desk.flush();
    let file = dir.path().join("rounds").join("db-1.json");
    assert!(!file.exists(), "the second desk writes nothing");

    // Once the first one quits, the data dir is free for the next start.
    first.desk.close();
    drop(first);
    let third = start_in(dir.path()).await;
    assert!(third.desk.is_persistent());
}

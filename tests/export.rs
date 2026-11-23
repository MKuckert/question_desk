//! Exporting a single round as Markdown; the JSON export is the stored file.

mod common;

use common::*;
use jiff::tz::TimeZone;
use question_desk::{
    export,
    schema::{Question, QuestionOption},
    store::{self, DraftAnswer, Status, StoredRound},
};

fn option(label: &str, description: &str) -> QuestionOption {
    QuestionOption {
        label: label.to_owned(),
        description: description.to_owned(),
        preview: None,
    }
}

fn question(text: &str, header: &str, options: Vec<QuestionOption>) -> Question {
    Question {
        question: text.to_owned(),
        header: header.to_owned(),
        body: None,
        options,
        multi_select: false,
        recommended: None,
    }
}

fn answer(selected: &[&str], note: &str) -> DraftAnswer {
    DraftAnswer {
        selected: selected.iter().map(|s| s.to_string()).collect(),
        note: note.to_owned(),
    }
}

fn storage_round(status: Status) -> StoredRound {
    let mut database = question(
        "Which database?",
        "Database",
        vec![
            option("Postgres", "Relational"),
            option("SQLite", "Embedded"),
        ],
    );
    database.body = Some("We need **transactions**.".to_owned());
    database.recommended = Some("Postgres".to_owned());
    let mut features = question(
        "Which features?",
        "Features",
        vec![option("Auth", ""), option("Billing", "Stripe")],
    );
    features.multi_select = true;
    StoredRound {
        id: "db-1".to_owned(),
        ticket: "db-1".to_owned(),
        session: "Claude Code · repo".to_owned(),
        title: Some("Storage".to_owned()),
        questions: vec![
            database,
            features,
            question("Anything else?", "Other", vec![]),
        ],
        answers: vec![
            answer(&["SQLite"], "small data"),
            answer(&["Auth", "Billing"], ""),
            answer(&[], "Keep it simple"),
        ],
        step: None,
        status,
        opened_at: "2026-11-23T10:00:00Z".parse().unwrap(),
        finished_at: (status != Status::Open).then(|| "2026-11-23T10:05:00Z".parse().unwrap()),
        delivered_at: None,
    }
}

fn markdown(round: &StoredRound) -> String {
    export::markdown(round, &TimeZone::UTC)
}

#[test]
fn an_answered_round() {
    insta::assert_snapshot!(markdown(&storage_round(Status::Answered)), @r"
    # Storage

    * Agent Session: Claude Code · repo
    * Ticket: db-1
    * Opened: 2026-11-23 10:00 UTC
    * Answered: 2026-11-23 10:05 UTC

    ## 1\. Which database?

    *Database*

     > 
     > We need **transactions**.

    * [ ] Postgres: Relational (recommended)
    * [x] SQLite: Embedded

    **Answer:** SQLite

    **Note:** small data

    ## 2\. Which features?

    *Features*

    * [x] Auth
    * [x] Billing: Stripe

    **Answer:** Auth, Billing

    ## 3\. Anything else?

    *Other*

    **Answer:** Keep it simple
    ");
}

#[test]
fn a_cancelled_round_with_unanswered_questions() {
    let mut round = storage_round(Status::Cancelled);
    round.answers[1] = answer(&[], "");
    round.answers[2] = answer(&[], "  ");
    let text = markdown(&round);
    assert!(
        text.contains("* Cancelled: 2026-11-23 10:05 UTC\n"),
        "{text}"
    );
    insta::assert_snapshot!(text.split("## 2\\.").nth(1).unwrap(), @r"
     Which features?

    *Features*

    * [ ] Auth
    * [ ] Billing: Stripe

    *Unanswered*

    ## 3\. Anything else?

    *Other*

    *Unanswered*
    ");
}

#[test]
fn an_open_round_exports_its_drafts_marked_as_open() {
    let text = markdown(&storage_round(Status::Open));
    insta::assert_snapshot!(text.split("## 1\\.").next().unwrap(), @r"
    # Storage

    * Agent Session: Claude Code · repo
    * Ticket: db-1
    * Opened: 2026-11-23 10:00 UTC
    * Status: open, the answers are drafts

    ");
    assert!(text.contains("**Answer:** SQLite"), "{text}");
}

#[test]
fn agent_and_user_text_cannot_break_the_structure() {
    let mut round = storage_round(Status::Answered);
    round.title = Some("# Not a <b>heading</b>".to_owned());
    round.questions[0].question = "Use `*` or _?".to_owned();
    round.questions[0].body = Some("# Big\n\n- item\n\n```\ncode\n```".to_owned());
    round.answers[0].note = "1. not a list\n\n## nor a heading".to_owned();
    insta::assert_snapshot!(markdown(&round).split("## 2\\.").next().unwrap(), @r"
    # \# Not a \<b>heading\</b>

    * Agent Session: Claude Code · repo
    * Ticket: db-1
    * Opened: 2026-11-23 10:00 UTC
    * Answered: 2026-11-23 10:05 UTC

    ## 1\. Use \`\*\` or \_?

    *Database*

     > 
     > # Big
     > 
     > * item
     > 
     > ````
     > code
     > ````

    * [ ] Postgres: Relational (recommended)
    * [x] SQLite: Embedded

    **Answer:** SQLite

    **Note:** 1. not a list

    \## nor a heading

    ");
}

#[test]
fn emoji_pass_through() {
    let mut round = storage_round(Status::Answered);
    round.title = Some("Launch 🚀".to_owned());
    round.answers[0].note = "👍🏽 fine by me 🇩🇪".to_owned();
    let text = markdown(&round);
    assert!(text.starts_with("# Launch 🚀\n"), "{text}");
    assert!(text.contains("**Note:** 👍🏽 fine by me 🇩🇪\n"), "{text}");
}

#[tokio::test]
async fn the_json_export_is_the_stored_file() {
    let dir = tempfile::tempdir().unwrap();
    let h = start_in(dir.path()).await;
    let agent = h.agent().await;
    let ask =
        tokio::spawn(async move { call(&agent, "ask_user_question", db_round("db-1")).await });
    h.select("db-1").await;
    h.desk.toggle_option(1);
    h.desk.submit();
    ask.await.unwrap();

    let exported = h.desk.export_json("db-1").unwrap();
    let stored = std::fs::read_to_string(dir.path().join("rounds/db-1.json")).unwrap();
    assert_eq!(exported, stored);
    assert_eq!(store::to_json(&h.desk.export("db-1").unwrap()), stored);
}

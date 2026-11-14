//! Snapshot tests for the text the Agent receives (secondary seam).

use question_desk::format::{self, Outcome, QuestionResult, RoundResult};

fn q(question: &str, answer: Option<&str>, note: Option<&str>) -> QuestionResult {
    QuestionResult {
        question: question.into(),
        answer: answer.map(Into::into),
        note: note.map(Into::into),
    }
}

fn round(outcome: Outcome, questions: Vec<QuestionResult>) -> RoundResult {
    RoundResult {
        ticket: "db-choice-7f3".into(),
        outcome,
        questions,
    }
}

#[test]
fn short_answers_use_the_one_line_shape() {
    let r = round(
        Outcome::Submitted,
        vec![
            q("Which database?", Some("Postgres"), None),
            q("Which features?", Some("Auth, Billing"), None),
        ],
    );
    insta::assert_snapshot!(format::text(&r), @r#"Your questions have been answered: "Which database?"="Postgres", "Which features?"="Auth, Billing". You can now continue with these answers in mind."#);
}

#[test]
fn notes_follow_their_answer_and_unanswered_questions_are_listed() {
    let r = round(
        Outcome::Submitted,
        vec![
            q("Which database?", Some("Postgres"), Some("only v16+")),
            q("Which cache?", None, None),
            q("Which queue?", None, None),
        ],
    );
    insta::assert_snapshot!(format::text(&r), @r#"Your questions have been answered: "Which database?"="Postgres" (note: "only v16+"). Unanswered: "Which cache?", "Which queue?". You can now continue with these answers in mind."#);
}

#[test]
fn cancelled_round_reports_partial_answers_and_the_rest_as_unanswered() {
    let r = round(
        Outcome::Cancelled,
        vec![
            q("Which database?", Some("Postgres"), None),
            q("Which cache?", None, None),
        ],
    );
    insta::assert_snapshot!(format::text(&r), @r#"The user cancelled the question round. Answers given before cancelling: "Which database?"="Postgres". Unanswered: "Which cache?"."#);
}

#[test]
fn rounds_without_any_answer_say_so_instead_of_listing_nothing() {
    let qs = || {
        vec![
            q("Which database?", None, None),
            q("Which cache?", None, None),
        ]
    };
    insta::assert_snapshot!(format::text(&round(Outcome::Cancelled, qs())), @r#"The user cancelled the question round without answering any questions. Unanswered: "Which database?", "Which cache?"."#);
    insta::assert_snapshot!(format::text(&round(Outcome::Submitted, qs())), @r#"The user submitted the question round without answering any questions. Unanswered: "Which database?", "Which cache?"."#);
}

#[test]
fn a_multi_line_answer_switches_the_whole_round_to_markdown_blocks() {
    let r = round(
        Outcome::Submitted,
        vec![
            q(
                "Which database?",
                Some("Postgres, because\nwe already run it."),
                Some("only v16+"),
            ),
            q("Which cache?", Some("Redis"), None),
            q("Which queue?", None, None),
        ],
    );
    insta::assert_snapshot!(format::text(&r), @r"
    Your questions have been answered. You can now continue with these answers in mind.

    ## Which database?

    **Answer:** Postgres, because
    we already run it.

    **Note:** only v16+

    ## Which cache?

    **Answer:** Redis

    ## Which queue?

    *Unanswered*
    ");
}

#[test]
fn quotes_inside_one_line_answers_are_escaped() {
    let r = round(
        Outcome::Submitted,
        vec![q(
            r#"Call it "Desk"?"#,
            Some(r#"Yes, "Desk" 👍🏽"#),
            Some(r#"or "Pult""#),
        )],
    );
    insta::assert_snapshot!(format::text(&r), @r#"Your questions have been answered: "Call it \"Desk\"?"="Yes, \"Desk\" 👍🏽" (note: "or \"Pult\""). You can now continue with these answers in mind."#);
}

#[test]
fn markdown_in_user_text_cannot_inject_structure() {
    let r = round(
        Outcome::Cancelled,
        vec![
            q(
                "Ship *today*?",
                Some("# Not a heading\n- not a list\n**not bold** 🇩🇪"),
                None,
            ),
            q("## Fake section", None, None),
        ],
    );
    insta::assert_snapshot!(format::text(&r), @r"
    The user cancelled the question round. Answers given before cancelling:

    ## Ship \*today\*?

    **Answer:** # Not a heading
    \- not a list
    \*\*not bold\*\* 🇩🇪

    ## \#\# Fake section

    *Unanswered*
    ");
}

#[test]
fn a_long_single_line_note_also_switches_to_markdown_blocks() {
    let note = "Fine for now, but revisit once we have more than one tenant, because the schema-per-tenant approach does not scale.";
    let r = round(
        Outcome::Submitted,
        vec![q("Which database?", Some("Postgres"), Some(note))],
    );
    insta::assert_snapshot!(format::text(&r), @r"
    Your questions have been answered. You can now continue with these answers in mind.

    ## Which database?

    **Answer:** Postgres

    **Note:** Fine for now, but revisit once we have more than one tenant, because the schema-per-tenant approach does not scale.
    ");
}

//! What the webview gets: the desk read model, flattened into serialisable structs.
//!
//! `desk.rs` stays free of serde; these mirror its views and add what only the frontend needs
//! (rendered Markdown, the age text, whether Enter accepts the Recommendation).

use std::time::SystemTime;

use serde::Serialize;

use super::markdown;
use crate::desk::{QuestionState, RoundView, SessionView, Step};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeskState {
    pub platform: &'static str,
    /// The empty state's line: the MCP URL, or why the server is not running.
    pub status: String,
    pub sessions: Vec<Session>,
    pub open_count: usize,
    pub round: Option<Round>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub label: String,
    pub rounds: Vec<RoundEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundEntry {
    pub ticket: String,
    /// The title, else the Ticket.
    pub name: String,
    pub answered: usize,
    pub total: usize,
    pub age: String,
    pub connected: bool,
    pub new: bool,
    pub selected: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Round {
    pub ticket: String,
    pub session: String,
    pub title: Option<String>,
    /// The current Question; `None` on the Review Page.
    pub step: Option<usize>,
    pub questions: Vec<Question>,
    pub connected: bool,
    pub unanswered: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub question: String,
    pub header: String,
    /// Sanitised HTML of the Markdown `body`; only for the current Question.
    pub body_html: Option<String>,
    pub multi_select: bool,
    pub options: Vec<QuestionOption>,
    pub recommendation_text: Option<String>,
    pub note: String,
    pub answer: Option<String>,
    pub answer_note: Option<String>,
    /// `current`, `answered` or `unanswered`.
    pub state: &'static str,
    /// Enter outside the Note accepts the Recommendation.
    pub accepts_recommendation: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
    pub preview: Option<String>,
    pub selected: bool,
    pub recommended: bool,
}

pub fn desk_state(
    status: String,
    sessions: Vec<SessionView>,
    round: Option<RoundView>,
) -> DeskState {
    let selected = round.as_ref().map(|r| r.ticket.clone());
    let sessions: Vec<Session> = sessions
        .into_iter()
        .map(|s| Session {
            label: s.label,
            rounds: s
                .rounds
                .into_iter()
                .map(|r| RoundEntry {
                    selected: selected.as_deref() == Some(r.ticket.as_str()),
                    name: r.title.unwrap_or_else(|| r.ticket.clone()),
                    ticket: r.ticket,
                    answered: r.answered,
                    total: r.total,
                    age: age(r.opened_at),
                    connected: r.connected,
                    new: r.new,
                })
                .collect(),
        })
        .collect();
    DeskState {
        platform: std::env::consts::OS,
        status,
        open_count: sessions.iter().map(|s| s.rounds.len()).sum(),
        sessions,
        round: round.map(round_view),
    }
}

fn round_view(round: RoundView) -> Round {
    let unanswered = round.unanswered().len();
    let step = match round.step {
        Step::Question(i) => Some(i),
        Step::Review => None,
    };
    Round {
        questions: round
            .questions
            .into_iter()
            .enumerate()
            .map(|(i, q)| {
                let accepts_recommendation = q.answer.is_none()
                    && (q.recommendation_text.is_some() || q.options.iter().any(|o| o.recommended));
                Question {
                    body_html: q
                        .body
                        .as_deref()
                        .filter(|_| step == Some(i))
                        .map(markdown::to_html),
                    question: q.question,
                    header: q.header,
                    multi_select: q.multi_select,
                    options: q
                        .options
                        .into_iter()
                        .map(|o| QuestionOption {
                            label: o.label,
                            description: o.description,
                            preview: o.preview,
                            selected: o.selected,
                            recommended: o.recommended,
                        })
                        .collect(),
                    recommendation_text: q.recommendation_text,
                    note: q.note,
                    answer: q.answer,
                    answer_note: q.answer_note,
                    state: match q.state {
                        QuestionState::Current => "current",
                        QuestionState::Answered => "answered",
                        QuestionState::Unanswered => "unanswered",
                    },
                    accepts_recommendation,
                }
            })
            .collect(),
        ticket: round.ticket,
        session: round.session,
        title: round.title,
        step,
        connected: round.connected,
        unanswered,
    }
}

/// "just now", "5 min ago", "2 h ago".
fn age(since: SystemTime) -> String {
    let secs = since.elapsed().map(|d| d.as_secs()).unwrap_or(0);
    match secs {
        0..60 => "just now".to_owned(),
        60..3600 => format!("{} min ago", secs / 60),
        _ => format!("{} h ago", secs / 3600),
    }
}

#[cfg(test)]
mod tests {
    //! `QD_DUMP_DIR=… cargo test dump_states -- --ignored` writes sample `state` payloads for
    //! `tools/render-offscreen.swift`, which renders the frontend without a window.

    use serde_json::json;

    use super::desk_state;
    use crate::{desk::Desk, schema};

    fn ask(desk: &Desk, client: &str, args: serde_json::Value) -> crate::desk::Attachment {
        desk.ask(schema::parse_ask(args).expect("valid round"), Some(client.into()))
            .expect("round opens")
    }

    #[test]
    #[ignore = "writes files; run on demand"]
    fn dump_states() {
        let Some(dir) = std::env::var_os("QD_DUMP_DIR") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, desk: &Desk| {
            let state = desk_state(
                "Agents connect to http://127.0.0.1:7462/mcp".into(),
                desk.sessions(),
                desk.current(),
            );
            std::fs::write(dir.join(name), serde_json::to_vec(&state).unwrap()).unwrap();
        };

        let desk = Desk::default();
        write("empty.json", &desk);

        let _a = ask(&desk, "claude-code", json!({
            "ticket": "events-1", "source": "~/code/shop-backend", "title": "Event store design",
            "questions": [
                {"question": "Which database should back the event store?", "header": "Database",
                 "body": "We need **append-only** writes, ordered reads per stream, and ~2k events/s at peak.\n\n| | Postgres | EventStoreDB | DynamoDB |\n|---|---|---|---|\n| Ops effort | low (we run it) | new system | managed |\n| Ordering | per table, via sequence | native | per partition |\n\nWe already run Postgres 16 in production.",
                 "options": [
                    {"label": "Postgres", "description": "Events table with a global sequence; we know how to operate it."},
                    {"label": "EventStoreDB", "description": "Purpose-built, projections included, but a new system to run."},
                    {"label": "DynamoDB", "description": "Managed and scalable; ordering only within a partition."}],
                 "recommended": "Postgres"},
                {"question": "Where should projections run?", "header": "Projections",
                 "options": [
                    {"label": "In-process", "description": "Same service, same deploy; simplest."},
                    {"label": "Separate worker", "description": "Own deploy; can be rebuilt without touching writes."}],
                 "recommended": "Separate worker, but share the code with the service so we can move later"},
                {"question": "Anything we have not covered that worries you?", "header": "Other",
                 "body": "Free text. Leave empty if nothing comes to mind."}
            ]}));
        let _b = ask(&desk, "claude-code", json!({
            "ticket": "button-api", "source": "~/code/design-system", "title": "Button API",
            "questions": [{"question": "Which button API should the design system expose?", "header": "API",
                "body": "Hover an option to see its usage.",
                "options": [
                    {"label": "Variant prop", "description": "One component, a `variant` string.",
                     "preview": "<Button variant=\"primary\" size=\"lg\" onClick={save}>\n  Save\n</Button>\n\n<Button variant=\"danger\">Delete</Button>"},
                    {"label": "Separate components", "description": "One component per variant.",
                     "preview": "<PrimaryButton size=\"lg\" onClick={save}>\n  Save\n</PrimaryButton>"},
                    {"label": "Builder", "description": "Chained configuration.",
                     "preview": "button()\n  .primary()\n  .large()\n  .onClick(save)"}],
                "recommended": "Variant prop"}]}));
        let _c = ask(&desk, "codex", json!({
            "ticket": "offsite", "source": "~/code/team-offsite", "title": "Team-Offsite 🎉",
            "questions": [
                {"question": "Wohin geht das Offsite? 🗺️", "header": "Ort",
                 "body": "Zur Auswahl stehen drei Orte. Bitte antworte gern mit Emoji 😄 (Ctrl+Cmd+Space) und Umlauten: ä ö ü ß.",
                 "options": [
                    {"label": "Ostsee 🌊", "description": "Strand, Fischbrötchen, 3 h mit dem Zug."},
                    {"label": "Alpen 🏔️", "description": "Wandern, Hütte, 6 h mit dem Zug."},
                    {"label": "Berlin 🇩🇪", "description": "Kein Reiseaufwand, Büro um die Ecke."}],
                 "recommended": "Ostsee 🌊"},
                {"question": "Welche Aktivitäten? 🧗", "header": "Programm", "multiSelect": true,
                 "options": [
                    {"label": "Kochkurs 👩‍🍳", "description": "Gemeinsam kochen."},
                    {"label": "Escape Room 🔐", "description": "Teamwork unter Zeitdruck."},
                    {"label": "Wanderung 🥾", "description": "Halber Tag draußen."}]},
                {"question": "Gibt es Essenswünsche? 🥗", "header": "Essen"}
            ]}));
        write("question.json", &desk);

        desk.select("button-api");
        desk.toggle_option(1);
        write("preview.json", &desk);

        desk.select("offsite");
        desk.go_to(1);
        desk.toggle_option(0);
        desk.toggle_option(2);
        desk.set_note("Gern mit Grillabend 🍖 👩‍👩‍👧 🇩🇪 – Grüße!");
        write("multi.json", &desk);

        desk.go_to(0);
        desk.toggle_option(0);
        desk.review();
        write("review.json", &desk);
    }
}

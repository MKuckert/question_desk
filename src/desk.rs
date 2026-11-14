//! The core: owns all Question Rounds and Agent Sessions.
//!
//! The Agent side opens or reattaches to rounds by Ticket and awaits their results; the
//! user side is the set of intents the UI triggers on the selected round. No egui and no
//! HTTP types cross this interface.

use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::watch;

use crate::{
    format::{Outcome, QuestionResult, RoundResult},
    schema::{AskInput, Question},
};

#[derive(Clone, Default)]
pub struct Desk {
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    /// In arrival order.
    rounds: Vec<Round>,
    selected: Option<String>,
}

struct Round {
    ticket: String,
    session: String,
    title: Option<String>,
    questions: Vec<Question>,
    answers: Vec<Draft>,
    step: Step,
    result: watch::Sender<Option<RoundResult>>,
}

/// What the user has entered for one Question so far.
#[derive(Default)]
struct Draft {
    /// Indices of the selected Options, in Option order.
    selected: Vec<usize>,
    note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Question(usize),
    Review,
}

/// One Agent Session in the sidebar, with its open rounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionView {
    pub label: String,
    pub rounds: Vec<RoundSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundSummary {
    pub ticket: String,
    pub title: Option<String>,
}

/// A call waiting for a round's result.
pub struct Attachment {
    result: watch::Receiver<Option<RoundResult>>,
}

impl Attachment {
    /// Resolves when the user Submits or Cancels the round.
    pub async fn finished(&mut self) -> RoundResult {
        let result = self
            .result
            .wait_for(Option::is_some)
            .await
            .expect("rounds live as long as the desk");
        result.clone().expect("waited for a result")
    }
}

impl Desk {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().expect("desk state is never poisoned")
    }

    // ---- Agent side -------------------------------------------------------------------

    /// Opens a Question Round for a validated `ask_user_question` call.
    pub fn ask(&self, input: AskInput, client: Option<String>) -> Result<Attachment, String> {
        let mut state = self.lock();
        let (result, receiver) = watch::channel(None);
        let session = session_label(client.as_deref(), input.source.as_deref());
        state.rounds.push(Round {
            ticket: input.ticket,
            session,
            title: input.title,
            answers: input.questions.iter().map(|_| Draft::default()).collect(),
            questions: input.questions,
            step: Step::Question(0),
            result,
        });
        Ok(Attachment { result: receiver })
    }

    // ---- User side --------------------------------------------------------------------

    /// Open rounds grouped by Agent Session, in order of first arrival.
    pub fn sessions(&self) -> Vec<SessionView> {
        let state = self.lock();
        let mut sessions: Vec<SessionView> = Vec::new();
        for round in state.rounds.iter().filter(|r| r.is_open()) {
            let summary = RoundSummary { ticket: round.ticket.clone(), title: round.title.clone() };
            match sessions.iter_mut().find(|s| s.label == round.session) {
                Some(session) => session.rounds.push(summary),
                None => sessions.push(SessionView { label: round.session.clone(), rounds: vec![summary] }),
            }
        }
        sessions
    }

    pub fn select(&self, ticket: &str) {
        let mut state = self.lock();
        if state.rounds.iter().any(|r| r.ticket == ticket && r.is_open()) {
            state.selected = Some(ticket.to_owned());
        }
    }

    /// Selects Option `index` of the current Question.
    pub fn toggle_option(&self, index: usize) {
        self.with_current_question(|_, draft| {
            draft.selected = vec![index];
        });
    }

    pub fn set_note(&self, note: &str) {
        self.with_current_question(|_, draft| draft.note = note.to_owned());
    }

    pub fn next(&self) {
        self.with_selected(|round| {
            round.step = match round.step {
                Step::Question(i) if i + 1 < round.questions.len() => Step::Question(i + 1),
                _ => Step::Review,
            }
        });
    }

    pub fn submit(&self) {
        self.finish(Outcome::Submitted);
    }

    fn finish(&self, outcome: Outcome) {
        self.with_selected(|round| {
            let result = round.result_for(outcome);
            round.result.send_replace(Some(result));
        });
    }

    fn with_selected(&self, f: impl FnOnce(&mut Round)) {
        let mut state = self.lock();
        let State { rounds, selected } = &mut *state;
        if let Some(round) = rounds.iter_mut().find(|r| Some(&r.ticket) == selected.as_ref() && r.is_open()) {
            f(round);
        }
    }

    fn with_current_question(&self, f: impl FnOnce(&Question, &mut Draft)) {
        self.with_selected(|round| {
            if let Step::Question(i) = round.step {
                f(&round.questions[i], &mut round.answers[i]);
            }
        });
    }
}

impl Round {
    fn is_open(&self) -> bool {
        self.result.borrow().is_none()
    }

    fn result_for(&self, outcome: Outcome) -> RoundResult {
        RoundResult {
            ticket: self.ticket.clone(),
            outcome,
            questions: self
                .questions
                .iter()
                .zip(&self.answers)
                .map(|(question, draft)| answer(question, draft))
                .collect(),
        }
    }
}

/// The Answer is the selected labels; without a selection the Note is the Answer.
fn answer(question: &Question, draft: &Draft) -> QuestionResult {
    let note = draft.note.trim();
    let note = (!note.is_empty()).then(|| note.to_owned());
    let (answer, note) = if draft.selected.is_empty() {
        (note, None)
    } else {
        let labels: Vec<&str> =
            draft.selected.iter().map(|&i| question.options[i].label.as_str()).collect();
        (Some(labels.join(", ")), note)
    };
    QuestionResult { question: question.question.clone(), answer, note }
}

fn session_label(client: Option<&str>, source: Option<&str>) -> String {
    let client = client.unwrap_or("Unknown client");
    match source {
        Some(source) => format!("{client} · {source}"),
        None => client.to_owned(),
    }
}

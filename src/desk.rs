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
    /// One clone per live [`Attachment`]; the count tells whether an Agent is waiting.
    attached: Arc<()>,
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
    /// Whether any Agent call is currently waiting for this round's result.
    pub connected: bool,
}

/// A call waiting for a round's result.
pub struct Attachment {
    ticket: String,
    result: watch::Receiver<Option<RoundResult>>,
    _attached: Arc<()>,
    title: Option<String>,
    questions: Vec<String>,
}

impl Attachment {
    pub fn ticket(&self) -> &str {
        &self.ticket
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The round's question texts, so a pending answer can identify the round.
    pub fn questions(&self) -> &[String] {
        &self.questions
    }

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

    /// Opens a Question Round for a validated `ask_user_question` call, or reattaches to
    /// the round with the same Ticket if its Questions are identical.
    pub fn ask(&self, input: AskInput, client: Option<String>) -> Result<Attachment, String> {
        let mut state = self.lock();
        if let Some(round) = state.rounds.iter().find(|r| r.ticket == input.ticket) {
            if round.questions != input.questions {
                return Err(format!(
                    "ticket {:?} is already used by a different question round; choose a new unique ticket",
                    input.ticket
                ));
            }
            return Ok(round.attach());
        }
        let (result, _) = watch::channel(None);
        let session = session_label(client.as_deref(), input.source.as_deref());
        state.rounds.push(Round {
            ticket: input.ticket,
            session,
            title: input.title,
            answers: input.questions.iter().map(|_| Draft::default()).collect(),
            questions: input.questions,
            step: Step::Question(0),
            result,
            attached: Arc::new(()),
        });
        Ok(state.rounds.last().expect("just pushed").attach())
    }

    /// Attaches to the round with this Ticket, open or finished; `None` if unknown.
    pub fn pickup(&self, ticket: &str) -> Option<Attachment> {
        self.lock().rounds.iter().find(|r| r.ticket == ticket).map(Round::attach)
    }

    // ---- User side --------------------------------------------------------------------

    /// Open rounds grouped by Agent Session, in order of first arrival.
    pub fn sessions(&self) -> Vec<SessionView> {
        let state = self.lock();
        let mut sessions: Vec<SessionView> = Vec::new();
        for round in state.rounds.iter().filter(|r| r.is_open()) {
            let summary = RoundSummary {
                ticket: round.ticket.clone(),
                title: round.title.clone(),
                connected: Arc::strong_count(&round.attached) > 1,
            };
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

    /// Toggles Option `index` of the current Question: like a checkbox for `multiSelect`,
    /// like a radio button (that can be cleared again) otherwise.
    pub fn toggle_option(&self, index: usize) {
        self.with_current_question(|question, draft| {
            if index >= question.options.len() {
                return;
            }
            let was_selected = draft.selected.contains(&index);
            if !question.multi_select {
                draft.selected.clear();
            }
            if was_selected {
                draft.selected.retain(|&i| i != index);
            } else {
                draft.selected.push(index);
                draft.selected.sort_unstable();
            }
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

    pub fn cancel(&self) {
        self.finish(Outcome::Cancelled);
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
    fn attach(&self) -> Attachment {
        Attachment {
            ticket: self.ticket.clone(),
            result: self.result.subscribe(),
            _attached: self.attached.clone(),
            title: self.title.clone(),
            questions: self.questions.iter().map(|q| q.question.clone()).collect(),
        }
    }

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

//! The core: owns all Question Rounds and Agent Sessions.
//!
//! The Agent side opens or reattaches to rounds by Ticket and awaits their results; the
//! user side is the set of intents the UI triggers on the selected round. No egui and no
//! HTTP types cross this interface.

use std::{
    sync::{Arc, Mutex, MutexGuard},
    time::SystemTime,
};

use tokio::sync::watch;

use crate::{
    format::{Outcome, QuestionResult, RoundResult},
    schema::{AskInput, Question},
};

#[derive(Clone, Default)]
pub struct Desk {
    state: Arc<Mutex<State>>,
    listener: Arc<Mutex<Option<Listener>>>,
}

type Listener = Arc<dyn Fn(DeskEvent) + Send + Sync>;

/// What the UI hears about changes it did not trigger itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeskEvent {
    /// A new Question Round arrived.
    RoundOpened,
    /// Anything else changed, e.g. an Agent connected or went away.
    Changed,
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
    opened_at: SystemTime,
    /// Whether the user has looked at the round yet.
    seen: bool,
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
    pub answered: usize,
    pub total: usize,
    pub opened_at: SystemTime,
    /// The user has not looked at this round yet.
    pub new: bool,
}

/// The selected round, as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundView {
    pub ticket: String,
    pub session: String,
    pub title: Option<String>,
    pub step: Step,
    pub questions: Vec<QuestionView>,
    pub connected: bool,
}

impl RoundView {
    /// Texts of the Questions that have no Answer yet.
    pub fn unanswered(&self) -> Vec<&str> {
        self.questions
            .iter()
            .filter(|q| q.answer.is_none())
            .map(|q| q.question.as_str())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionView {
    pub question: String,
    pub header: String,
    pub body: Option<String>,
    pub multi_select: bool,
    pub options: Vec<OptionView>,
    /// A Recommendation that matches no Option label; matching ones badge the Option.
    pub recommendation_text: Option<String>,
    pub note: String,
    /// What the Agent would receive for this Question right now.
    pub answer: Option<String>,
    pub state: QuestionState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionView {
    pub label: String,
    pub description: String,
    pub preview: Option<String>,
    pub selected: bool,
    pub recommended: bool,
}

/// A Question's state in the step bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionState {
    Open,
    Answered,
    Current,
}

/// A call waiting for a round's result.
pub struct Attachment {
    ticket: String,
    result: watch::Receiver<Option<RoundResult>>,
    /// Counts this call as connected; released on drop.
    attached: Option<Arc<()>>,
    desk: Desk,
    title: Option<String>,
    questions: Vec<String>,
}

impl Drop for Attachment {
    fn drop(&mut self) {
        self.attached.take();
        self.desk.notify(DeskEvent::Changed);
    }
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

    /// Registers the one listener (the UI) for changes from the Agent side.
    pub fn on_change(&self, listener: impl Fn(DeskEvent) + Send + Sync + 'static) {
        *self
            .listener
            .lock()
            .expect("listener lock is never poisoned") = Some(Arc::new(listener));
    }

    fn notify(&self, event: DeskEvent) {
        let listener = self
            .listener
            .lock()
            .expect("listener lock is never poisoned")
            .clone();
        if let Some(listener) = listener {
            listener(event);
        }
    }

    // ---- Agent side -------------------------------------------------------------------

    /// Opens a Question Round for a validated `ask_user_question` call, or reattaches to
    /// the round with the same Ticket if its Questions are identical.
    pub fn ask(&self, input: AskInput, client: Option<String>) -> Result<Attachment, String> {
        let (attachment, event) = self.open_or_reattach(input, client)?;
        self.notify(event);
        Ok(attachment)
    }

    fn open_or_reattach(
        &self,
        input: AskInput,
        client: Option<String>,
    ) -> Result<(Attachment, DeskEvent), String> {
        let mut state = self.lock();
        if let Some(round) = state.rounds.iter().find(|r| r.ticket == input.ticket) {
            if round.questions != input.questions {
                return Err(format!(
                    "ticket {:?} is already used by a different question round; choose a new unique ticket",
                    input.ticket
                ));
            }
            return Ok((round.attach(self), DeskEvent::Changed));
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
            opened_at: SystemTime::now(),
            seen: false,
        });
        if state.selected_round().is_none() {
            state.select_first_open();
        }
        let attachment = state.rounds.last().expect("just pushed").attach(self);
        Ok((attachment, DeskEvent::RoundOpened))
    }

    /// Attaches to the round with this Ticket, open or finished; `None` if unknown.
    pub fn pickup(&self, ticket: &str) -> Option<Attachment> {
        self.lock()
            .rounds
            .iter()
            .find(|r| r.ticket == ticket)
            .map(|r| r.attach(self))
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
                connected: round.connected(),
                answered: round.answered_count(),
                total: round.questions.len(),
                opened_at: round.opened_at,
                new: !round.seen,
            };
            match sessions.iter_mut().find(|s| s.label == round.session) {
                Some(session) => session.rounds.push(summary),
                None => sessions.push(SessionView {
                    label: round.session.clone(),
                    rounds: vec![summary],
                }),
            }
        }
        sessions
    }

    /// The selected round, if any.
    pub fn current(&self) -> Option<RoundView> {
        let state = self.lock();
        let round = state.selected_round()?;
        let questions = round
            .questions
            .iter()
            .zip(&round.answers)
            .enumerate()
            .map(|(i, (question, draft))| {
                let answer = answer(question, draft).answer;
                let recommended = question.recommended.as_deref();
                let matches_option = question
                    .options
                    .iter()
                    .any(|o| Some(o.label.as_str()) == recommended);
                QuestionView {
                    question: question.question.clone(),
                    header: question.header.clone(),
                    body: question.body.clone(),
                    multi_select: question.multi_select,
                    options: question
                        .options
                        .iter()
                        .enumerate()
                        .map(|(j, o)| OptionView {
                            label: o.label.clone(),
                            description: o.description.clone(),
                            preview: o.preview.clone(),
                            selected: draft.selected.contains(&j),
                            recommended: Some(o.label.as_str()) == recommended,
                        })
                        .collect(),
                    recommendation_text: recommended.filter(|_| !matches_option).map(str::to_owned),
                    note: draft.note.clone(),
                    state: if round.step == Step::Question(i) {
                        QuestionState::Current
                    } else if answer.is_some() {
                        QuestionState::Answered
                    } else {
                        QuestionState::Open
                    },
                    answer,
                }
            })
            .collect();
        Some(RoundView {
            ticket: round.ticket.clone(),
            session: round.session.clone(),
            title: round.title.clone(),
            step: round.step,
            questions,
            connected: round.connected(),
        })
    }

    pub fn select(&self, ticket: &str) {
        let mut state = self.lock();
        if let Some(round) = state
            .rounds
            .iter_mut()
            .find(|r| r.ticket == ticket && r.is_open())
        {
            round.seen = true;
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

    /// Answers the current Question with its Recommendation if it has no Answer yet,
    /// then moves on.
    pub fn accept_recommendation(&self) {
        self.with_current_question(|question, draft| {
            let Some(recommended) = &question.recommended else {
                return;
            };
            if !draft.selected.is_empty() || !draft.note.trim().is_empty() {
                return;
            }
            match question
                .options
                .iter()
                .position(|o| &o.label == recommended)
            {
                Some(index) => draft.selected = vec![index],
                None => draft.note = recommended.clone(),
            }
        });
        self.next();
    }

    /// Goes to the previous Question; from the Review Page to the last Question.
    pub fn back(&self) {
        self.with_selected(|round| {
            round.step = match round.step {
                Step::Question(i) => Step::Question(i.saturating_sub(1)),
                Step::Review => Step::Question(round.questions.len() - 1),
            }
        });
    }

    /// Jumps to Question `index` (as in the step bar or on the Review Page).
    pub fn go_to(&self, index: usize) {
        self.with_selected(|round| {
            if index < round.questions.len() {
                round.step = Step::Question(index);
            }
        });
    }

    pub fn review(&self) {
        self.with_selected(|round| round.step = Step::Review);
    }

    pub fn submit(&self) {
        self.finish(Outcome::Submitted);
    }

    pub fn cancel(&self) {
        self.finish(Outcome::Cancelled);
    }

    /// Cancels every open round, e.g. when the user closes the window.
    pub fn cancel_all(&self) {
        let mut state = self.lock();
        for round in state.rounds.iter_mut().filter(|r| r.is_open()) {
            let result = round.result_for(Outcome::Cancelled);
            round.result.send_replace(Some(result));
        }
        state.selected = None;
    }

    /// Delivers the selected round's result and shows the next open round.
    fn finish(&self, outcome: Outcome) {
        let mut state = self.lock();
        if let Some(round) = state.selected_round_mut() {
            let result = round.result_for(outcome);
            round.result.send_replace(Some(result));
            state.select_first_open();
        }
    }

    fn with_selected(&self, f: impl FnOnce(&mut Round)) {
        let mut state = self.lock();
        if let Some(round) = state.selected_round_mut() {
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

impl State {
    fn select_first_open(&mut self) {
        let next = self.rounds.iter_mut().find(|r| r.is_open());
        self.selected = next.map(|round| {
            round.seen = true;
            round.ticket.clone()
        });
    }

    fn selected_round(&self) -> Option<&Round> {
        let selected = self.selected.as_ref()?;
        self.rounds
            .iter()
            .find(|r| &r.ticket == selected && r.is_open())
    }

    fn selected_round_mut(&mut self) -> Option<&mut Round> {
        let selected = self.selected.as_ref()?;
        self.rounds
            .iter_mut()
            .find(|r| &r.ticket == selected && r.is_open())
    }
}

impl Round {
    fn connected(&self) -> bool {
        Arc::strong_count(&self.attached) > 1
    }

    fn answered_count(&self) -> usize {
        self.questions
            .iter()
            .zip(&self.answers)
            .filter(|(q, d)| answer(q, d).answer.is_some())
            .count()
    }

    fn attach(&self, desk: &Desk) -> Attachment {
        Attachment {
            ticket: self.ticket.clone(),
            result: self.result.subscribe(),
            attached: Some(self.attached.clone()),
            desk: desk.clone(),
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
        let labels: Vec<&str> = draft
            .selected
            .iter()
            .map(|&i| question.options[i].label.as_str())
            .collect();
        (Some(labels.join(", ")), note)
    };
    QuestionResult {
        question: question.question.clone(),
        answer,
        note,
    }
}

fn session_label(client: Option<&str>, source: Option<&str>) -> String {
    let client = client.unwrap_or("Unknown client");
    match source {
        Some(source) => format!("{client} · {source}"),
        None => client.to_owned(),
    }
}

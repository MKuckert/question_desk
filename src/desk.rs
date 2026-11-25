//! The core: owns all Question Rounds and Agent Sessions.
//!
//! The Agent side opens or reattaches to rounds by Ticket and awaits their results; the
//! user side is the set of intents the UI triggers on the selected round. No egui and no
//! HTTP types cross this interface.

use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
    path::PathBuf,
    sync::{Arc, Condvar, Mutex, MutexGuard, Weak},
    time::{Duration, Instant, SystemTime},
};

use tokio::sync::watch;

use crate::{
    format::{Outcome, QuestionResult, RoundResult},
    schema::{AskInput, Question},
    search,
    store::{self, DraftAnswer, Store, StoreLock, StoredRound},
};

/// How long draft edits rest before they are written.
const DRAFT_DELAY: Duration = Duration::from_secs(1);

/// How long a decided Re-delivery Request still answers the Agents it listed, e.g. one
/// that polls `wait_for_answers` and was between two calls when the user decided.
pub const REDELIVERY_GRACE: Duration = Duration::from_secs(120);

#[derive(Clone, Default)]
pub struct Desk {
    state: Arc<Mutex<State>>,
    disk: Arc<DiskQueue>,
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
    /// The id of the selected round.
    selected: Option<String>,
    /// Where rounds are persisted, fixed at startup; `None` keeps them in memory only.
    store: Option<Store>,
    /// Keeps other Question Desks out of `store` until the quit.
    store_lock: Option<StoreLock>,
    /// The data dir persistence is configured for but cannot use, e.g. because another
    /// Question Desk uses it; until the UI took it.
    unavailable: Option<StorageUnavailable>,
    /// File writes decided under the lock, carried out once it is released (see
    /// [`Locked`]).
    jobs: Vec<Job>,
    /// The sequence number of the last queued [`Job`].
    last_job: u64,
    /// The filter's terms; open rounds and History show only matching rounds.
    filter: Vec<String>,
}

struct Round {
    /// Unique among all rounds, and the file name in the store: the Ticket, plus `@<n>`
    /// when a later round reuses a released Ticket.
    id: String,
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
    finished_at: Option<SystemTime>,
    /// When the result was first Delivered; `None` while open or Awaiting Pickup.
    delivered_at: Option<SystemTime>,
    /// Whether the user has looked at the round yet.
    seen: bool,
    /// Draft edits not written to the store yet, since this instant.
    dirty_since: Option<Instant>,
    /// Agents asking for this Delivered round's result again; never persisted.
    redelivery: Option<Redelivery>,
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
    pub id: String,
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
    pub id: String,
    pub ticket: String,
    pub session: String,
    pub title: Option<String>,
    pub step: Step,
    pub questions: Vec<QuestionView>,
    pub connected: bool,
    pub opened_at: SystemTime,
    /// Set once the round is finished; it is read-only then.
    pub finished: Option<FinishedView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishedView {
    pub outcome: Outcome,
    pub finished_at: SystemTime,
    /// `None` while Awaiting Pickup.
    pub delivered_at: Option<SystemTime>,
    /// Awaiting Pickup rounds can be reopened and changed; Delivered ones cannot.
    pub can_reopen: bool,
}

/// A finished round in the History.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub id: String,
    pub ticket: String,
    pub title: Option<String>,
    /// The Agent Session label.
    pub session: String,
    pub outcome: Outcome,
    pub opened_at: SystemTime,
    pub finished_at: SystemTime,
    /// No Agent has received the result yet.
    pub awaiting_pickup: bool,
    pub answered: usize,
    pub total: usize,
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
    /// The Answer the Agent would receive for this Question right now.
    pub answer: Option<String>,
    /// The Note the Agent would receive next to the Answer; `None` when the Note is
    /// itself the Answer or empty.
    pub answer_note: Option<String>,
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
    Unanswered,
    Answered,
    Current,
}

/// A call waiting for a round's result.
pub struct Attachment {
    ticket: String,
    wait: Wait,
    /// Counts this call as connected and identifies the round; released on drop. `None` for a Re-delivery Request.
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

    /// Whether this call waits for the user's decision on a Re-delivery Request.
    pub fn is_redelivery(&self) -> bool {
        matches!(self.wait, Wait::Redelivery { .. })
    }

    /// Resolves when the user Submits or Cancels the round, or decides a Re-delivery
    /// Request; a declined request is an error message for the Agent.
    ///
    /// A result is handed over only together with marking the round Delivered, so the
    /// round cannot be reopened after this returned it. Call it only when the result
    /// goes into the response right away.
    pub async fn finished(&mut self) -> Result<RoundResult, String> {
        match &mut self.wait {
            Wait::Result(result) => loop {
                // A round is only deleted after it finished, so this sees its result.
                drop(
                    result
                        .wait_for(Option::is_some)
                        .await
                        .map_err(|_| "the user deleted the question round".to_owned())?,
                );
                let attached = self.attached.as_ref().expect("a result call is attached");
                if let Some(result) = self.desk.hand_over(attached, result) {
                    return Ok(result);
                }
                // Reopened in the meantime: wait for the re-submit.
            },
            Wait::Redelivery { decision, result } => {
                // A deleted round's request is declined.
                let deliver = decision
                    .wait_for(Option::is_some)
                    .await
                    .map_or(Some(false), |d| *d);
                if deliver == Some(true) {
                    Ok(result.clone())
                } else {
                    Err(format!(
                        "the user declined re-delivery of ticket {:?}; ask again with a new ticket or clarify with the user first.",
                        self.ticket
                    ))
                }
            }
        }
    }
}

enum Wait {
    /// For the round's result.
    Result(watch::Receiver<Option<RoundResult>>),
    /// For the user's decision on delivering a Delivered round's `result` again.
    Redelivery {
        decision: watch::Receiver<Option<bool>>,
        result: RoundResult,
    },
}

/// Agents asking for a Delivered round's result again, answered by one decision.
struct Redelivery {
    /// The requesting calls, in order, one per Agent.
    requesters: Vec<Requester>,
    /// `Some(true)` to deliver again, `Some(false)` to decline.
    decision: watch::Sender<Option<bool>>,
    /// When the user decided; the decision answers the `requesters` for
    /// [`REDELIVERY_GRACE`] after that, and anyone else gets a new request.
    decided_at: Option<Instant>,
}

/// The MCP client making a call, from its `clientInfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Client {
    /// `clientInfo.name`: identifies the client.
    pub name: String,
    /// `clientInfo.title`: the display name, if it has one.
    pub title: Option<String>,
}

impl Client {
    fn label(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }
}

/// An Agent asking for a re-delivery.
struct Requester {
    /// Who asks, stable across `ask_user_question` and `wait_for_answers`: the client
    /// name, as titles need not be unique and `wait_for_answers` carries no `source`.
    agent: String,
    /// The Agent Session label to show, with the `source` if the call had one.
    label: String,
}

impl Requester {
    fn new(client: Option<&Client>, source: Option<&str>) -> Self {
        Self {
            agent: client.map_or_else(|| UNKNOWN_CLIENT.to_owned(), |c| c.name.clone()),
            label: session_label(client, source),
        }
    }
}

impl Redelivery {
    /// Whether a call from `agent` gets this request's decision.
    fn answers(&self, agent: &str) -> bool {
        self.decided_at.is_none_or(|at| {
            at.elapsed() < REDELIVERY_GRACE && self.requesters.iter().any(|r| r.agent == agent)
        })
    }
}

/// A file operation on one round, decided under the state lock.
struct Job {
    /// Later jobs on the same file win, whichever thread gets to the disk first.
    seq: u64,
    store: Store,
    id: String,
    /// `None` removes the file.
    write: Option<StoredRound>,
}

/// How long a failed write rests before the draft writer tries it again.
const RETRY_DELAY: Duration = Duration::from_secs(5);

/// Carries out the [`Job`]s, and tells when all queued ones are done.
#[derive(Default)]
struct DiskQueue {
    /// Taken only while `state` is not held, so disk I/O never holds up the desk.
    disk: Mutex<Disk>,
    /// Batches taken from the state but not carried out yet; counted up under the state
    /// lock, so whoever takes it later sees them.
    pending: Mutex<usize>,
    idle: Condvar,
}

impl DiskQueue {
    /// Counts a batch as pending; call it while holding the state lock.
    fn begin(&self) {
        *self.pending.lock().expect("pending lock is never poisoned") += 1;
    }

    /// Carries out a batch counted by [`DiskQueue::begin`].
    fn run(&self, jobs: Vec<Job>) {
        self.disk
            .lock()
            .expect("disk lock is never poisoned")
            .run(jobs);
        let mut pending = self.pending.lock().expect("pending lock is never poisoned");
        *pending -= 1;
        if *pending == 0 {
            self.idle.notify_all();
        }
    }

    /// Waits until every batch counted so far is carried out.
    fn wait_idle(&self) {
        let pending = self.pending.lock().expect("pending lock is never poisoned");
        drop(
            self.idle
                .wait_while(pending, |n| *n > 0)
                .expect("pending lock is never poisoned"),
        );
    }

    /// Tries the failed writes again, if they have rested for `delay`; returns how many
    /// still fail.
    fn retry(&self, delay: Duration) -> usize {
        let mut disk = self.disk.lock().expect("disk lock is never poisoned");
        disk.retry(delay);
        disk.failed.len()
    }
}

/// The sequence number of the last job carried out per file, and the jobs that failed.
#[derive(Default)]
struct Disk {
    done: HashMap<PathBuf, u64>,
    /// The latest failed job per file, until it or a later job on the file succeeds.
    failed: HashMap<PathBuf, Job>,
    /// When a job last failed.
    failed_at: Option<Instant>,
}

impl Disk {
    fn run(&mut self, mut jobs: Vec<Job>) {
        // Failed jobs go first: they are older than any new job on the same file.
        let mut retries: Vec<Job> = self.failed.drain().map(|(_, job)| job).collect();
        retries.sort_by_key(|job| job.seq);
        retries.append(&mut jobs);
        for job in retries {
            let path = job.store.path_of(&job.id);
            if self.done.get(&path).is_some_and(|&seq| seq > job.seq)
                || self.failed.get(&path).is_some_and(|f| f.seq > job.seq)
            {
                continue;
            }
            let result = match &job.write {
                Some(round) => job.store.save(round),
                None => job.store.remove(&job.id),
            };
            match result {
                Ok(()) => {
                    self.failed.remove(&path);
                    self.done.insert(path, job.seq);
                }
                Err(e) => {
                    let action = if job.write.is_some() {
                        "save"
                    } else {
                        "delete"
                    };
                    log::error!(
                        "cannot {action} question round {:?}, trying again later: {e}",
                        job.id
                    );
                    self.failed_at = Some(Instant::now());
                    self.failed.insert(path, job);
                }
            }
        }
    }

    fn retry(&mut self, delay: Duration) {
        if !self.failed.is_empty() && self.failed_at.is_none_or(|at| at.elapsed() >= delay) {
            self.run(Vec::new());
        }
    }
}

/// The locked desk state. The file writes queued meanwhile run when it is released, so
/// the UI, the draft writer and Agent calls never wait for the disk while holding it.
struct Locked<'a> {
    state: Option<MutexGuard<'a, State>>,
    disk: &'a DiskQueue,
}

impl<'a> Locked<'a> {
    fn new(state: &'a Mutex<State>, disk: &'a DiskQueue) -> Self {
        Self {
            state: Some(state.lock().expect("desk state is never poisoned")),
            disk,
        }
    }
}

impl Deref for Locked<'_> {
    type Target = State;

    fn deref(&self) -> &State {
        self.state.as_ref().expect("held until dropped")
    }
}

impl DerefMut for Locked<'_> {
    fn deref_mut(&mut self) -> &mut State {
        self.state.as_mut().expect("held until dropped")
    }
}

impl Drop for Locked<'_> {
    fn drop(&mut self) {
        let mut state = self.state.take().expect("held until dropped");
        let jobs = std::mem::take(&mut state.jobs);
        if jobs.is_empty() {
            return;
        }
        self.disk.begin();
        drop(state);
        self.disk.run(jobs);
    }
}

/// Where and whether rounds are stored (see `config.toml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageSettings {
    pub persistence: bool,
    /// `None` when there is no data dir (no home directory): rounds stay in memory.
    pub data_dir: Option<PathBuf>,
}

/// Why rounds are kept in memory only although persistence is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageUnavailable {
    pub data_dir: PathBuf,
    /// E.g. "another Question Desk uses it".
    pub reason: String,
}

/// A pending Re-delivery Request, as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedeliveryView {
    /// The Delivered round's id.
    pub id: String,
    pub ticket: String,
    /// The round's title, else its first question.
    pub title: String,
    pub requesters: Vec<String>,
    pub delivered_at: SystemTime,
}

impl Desk {
    /// The desk at startup: with persistence on, it holds the rounds stored in the data
    /// dir and writes to it.
    pub fn new(settings: StorageSettings) -> Self {
        match settings.data_dir.filter(|_| settings.persistence) {
            Some(dir) => Self::with_store(Store::new(dir)),
            None => Self::default(),
        }
    }

    /// A desk that persists its rounds in `store`, starting with the rounds stored there.
    /// Loading writes nothing. If another Question Desk uses `store`, the desk starts
    /// empty and keeps its rounds in memory (see [`Desk::take_storage_unavailable`]).
    pub fn with_store(store: Store) -> Self {
        let desk = Desk::default();
        let lock = match store.lock() {
            Ok(lock) => lock,
            Err(e) => {
                log::warn!(
                    "keeping question rounds in memory only: cannot use {}: {e}",
                    store.data_dir().display()
                );
                desk.lock().unavailable = Some(StorageUnavailable {
                    data_dir: store.data_dir().to_owned(),
                    reason: e.to_string(),
                });
                return desk;
            }
        };
        let loaded = store.load();
        {
            let mut state = desk.lock();
            state.rounds = loaded.rounds.into_iter().map(Round::restore).collect();
            state.rounds.sort_by_key(|r| r.opened_at);
            state.store = Some(store);
            state.store_lock = Some(lock);
            state.select_first_open();
        }
        desk.start_draft_writer();
        desk
    }

    fn start_draft_writer(&self) {
        let weak = Arc::downgrade(&self.state);
        let disk = self.disk.clone();
        std::thread::Builder::new()
            .name("draft-writer".into())
            .spawn(move || write_drafts_until_dropped(weak, disk))
            .expect("draft writer thread starts");
    }

    /// Why rounds are kept in memory although persistence is on; told once.
    pub fn take_storage_unavailable(&self) -> Option<StorageUnavailable> {
        self.lock().unavailable.take()
    }

    /// Deletes finished rounds that finished more than `retention_days` ago, from memory
    /// and store; `0` keeps them forever. Open rounds are never pruned, and nothing is
    /// pruned without a store.
    pub fn prune(&self, retention_days: u32) {
        if retention_days == 0 || !self.is_persistent() {
            return;
        }
        let max_age = Duration::from_secs(u64::from(retention_days) * 24 * 60 * 60);
        // Decided under the deleting lock: a round reopened meanwhile is not pruned.
        let pruned = self.delete_where(|r| {
            !r.is_open()
                && r.finished_at
                    .is_some_and(|at| at.elapsed().is_ok_and(|age| age > max_age))
        });
        if pruned > 0 {
            log::info!("pruned {pruned} question round(s) finished over {retention_days} days ago");
        }
    }

    /// Deletes a round from memory and its file, even while persistence is off; an open
    /// one is cancelled first, so its Agent receives `cancelled`.
    pub fn delete(&self, id: &str) {
        self.delete_where(|r| r.id == id);
    }

    /// Deletes all rounds, open and finished, of an Agent Session.
    pub fn delete_session(&self, label: &str) {
        self.delete_where(|r| r.session == label);
    }

    /// Deletes all finished rounds.
    pub fn clear_history(&self) {
        self.delete_where(|r| !r.is_open());
    }

    /// Deletes the rounds `doomed` picks, all under one lock; returns how many.
    fn delete_where(&self, doomed: impl Fn(&Round) -> bool) -> usize {
        let mut state = self.lock();
        let mut i = 0;
        let mut deleted = 0;
        while i < state.rounds.len() {
            if !doomed(&state.rounds[i]) {
                i += 1;
                continue;
            }
            let round = &mut state.rounds[i];
            if round.is_open() {
                round.finish(Outcome::Cancelled);
            }
            if let Some(request) = round.redelivery.take()
                && request.decided_at.is_none()
            {
                request.decision.send_replace(Some(false));
            }
            let round = state.rounds.remove(i);
            if let Some(store) = state.store.clone() {
                state.queue(&store, &round.id, None);
            }
            if state.selected.as_ref() == Some(&round.id) {
                state.selected = None;
            }
            deleted += 1;
        }
        if deleted > 0 {
            if state.selected.is_none() {
                state.select_first_open();
            }
            drop(state);
            self.notify(DeskEvent::Changed);
        }
        deleted
    }

    /// Whether rounds are written to a store and so survive a quit.
    pub fn is_persistent(&self) -> bool {
        self.lock().store.is_some()
    }

    /// Writes all unsaved drafts, e.g. on quit, and returns once every write queued so
    /// far is done, failed ones tried once more. Returns how many rounds' latest state
    /// could still not be written (or removed): it is lost on quit.
    pub fn flush(&self) -> usize {
        self.lock().write_drafts(Duration::ZERO);
        self.disk.wait_idle();
        self.disk.retry(Duration::ZERO)
    }

    /// Writes everything like [`Desk::flush`], then stops storing and lets other Question
    /// Desks use the data dir: the quit.
    pub fn close(&self) {
        self.flush();
        let mut state = self.lock();
        state.store = None;
        state.store_lock = None;
    }

    fn lock(&self) -> Locked<'_> {
        Locked::new(&self.state, &self.disk)
    }

    /// Takes the result of the finished round `attached` belongs to and marks the round
    /// Delivered, in one step: the user cannot reopen it in between. `None` if it was
    /// reopened, so the call waits for the re-submit.
    fn hand_over(
        &self,
        attached: &Arc<()>,
        receiver: &watch::Receiver<Option<RoundResult>>,
    ) -> Option<RoundResult> {
        let mut state = self.lock();
        let Some(i) = state
            .rounds
            .iter()
            .position(|r| Arc::ptr_eq(&r.attached, attached))
        else {
            // Deleted after it finished (an open round is cancelled first).
            return receiver.borrow().clone();
        };
        let result = state.rounds[i].result.borrow().clone()?;
        if state.rounds[i].delivered_at.is_none() {
            state.rounds[i].delivered_at = Some(SystemTime::now());
            state.save(i);
        }
        Some(result)
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
    pub fn ask(&self, input: AskInput, client: Option<Client>) -> Result<Attachment, String> {
        let (attachment, event) = self.open_or_reattach(input, client)?;
        self.notify(event);
        Ok(attachment)
    }

    fn open_or_reattach(
        &self,
        input: AskInput,
        client: Option<Client>,
    ) -> Result<(Attachment, DeskEvent), String> {
        let mut state = self.lock();
        let session = session_label(client.as_ref(), input.source.as_deref());
        if let Some(i) = state.holder(&input.ticket) {
            let round = &state.rounds[i];
            if round.delivered_at.is_none() {
                if round.questions != input.questions {
                    return Err(format!(
                        "ticket {:?} is already used by a different question round; choose a new unique ticket",
                        input.ticket
                    ));
                }
                return Ok((round.attach(self), DeskEvent::Changed));
            }
            if round.questions == input.questions {
                let requester = Requester::new(client.as_ref(), input.source.as_deref());
                return Ok(state.request_redelivery(i, requester, self));
            }
            // Delivered with other Questions: the Ticket is free for a new round.
        }
        let (result, _) = watch::channel(None);
        let id = state.new_id(&input.ticket);
        state.rounds.push(Round {
            id,
            ticket: input.ticket,
            session,
            title: input.title,
            answers: input.questions.iter().map(|_| Draft::default()).collect(),
            questions: input.questions,
            step: Step::Question(0),
            result,
            attached: Arc::new(()),
            opened_at: SystemTime::now(),
            finished_at: None,
            delivered_at: None,
            seen: false,
            dirty_since: None,
            redelivery: None,
        });
        let last = state.rounds.len() - 1;
        state.save(last);
        if state.selected_round().is_none() {
            state.select_first_open();
        }
        let attachment = state.rounds.last().expect("just pushed").attach(self);
        Ok((attachment, DeskEvent::RoundOpened))
    }

    /// Attaches to the round with this Ticket, open or finished; `None` if unknown.
    /// A Delivered round raises a Re-delivery Request from `client` instead.
    pub fn pickup(&self, ticket: &str, client: Option<Client>) -> Option<Attachment> {
        let mut state = self.lock();
        let i = state.holder(ticket)?;
        if state.rounds[i].delivered_at.is_none() {
            return Some(state.rounds[i].attach(self));
        }
        let requester = Requester::new(client.as_ref(), None);
        let (attachment, event) = state.request_redelivery(i, requester, self);
        drop(state);
        self.notify(event);
        Some(attachment)
    }

    /// Pending Re-delivery Requests, oldest round first.
    pub fn redeliveries(&self) -> Vec<RedeliveryView> {
        let state = self.lock();
        state
            .rounds
            .iter()
            .filter_map(|round| {
                let request = round.redelivery.as_ref()?;
                if request.decided_at.is_some() {
                    return None;
                }
                Some(RedeliveryView {
                    id: round.id.clone(),
                    ticket: round.ticket.clone(),
                    title: round
                        .title
                        .clone()
                        .unwrap_or_else(|| round.questions[0].question.clone()),
                    requesters: request.requesters.iter().map(|r| r.label.clone()).collect(),
                    delivered_at: round.delivered_at.unwrap_or(round.opened_at),
                })
            })
            .collect()
    }

    /// Answers the round's Re-delivery Request: its waiting calls get the old result.
    pub fn deliver_again(&self, id: &str) {
        self.decide_redelivery(id, true);
    }

    /// Answers the round's Re-delivery Request: its waiting calls get an error.
    pub fn decline_redelivery(&self, id: &str) {
        self.decide_redelivery(id, false);
    }

    /// Answers the calls waiting now, and for [`REDELIVERY_GRACE`] the next calls of the
    /// listed Agents (one polling `wait_for_answers` may be between two calls).
    fn decide_redelivery(&self, id: &str, deliver: bool) {
        let mut state = self.lock();
        let Some(round) = state.rounds.iter_mut().find(|r| r.id == id) else {
            return;
        };
        let Some(request) = &mut round.redelivery else {
            return;
        };
        if request.decided_at.is_none() {
            request.decision.send_replace(Some(deliver));
            request.decided_at = Some(Instant::now());
        }
    }

    // ---- User side --------------------------------------------------------------------

    /// Open rounds grouped by Agent Session, in order of first arrival.
    pub fn sessions(&self) -> Vec<SessionView> {
        let state = self.lock();
        let mut sessions: Vec<SessionView> = Vec::new();
        for round in state
            .rounds
            .iter()
            .filter(|r| r.is_open() && r.matches(&state.filter))
        {
            let summary = RoundSummary {
                id: round.id.clone(),
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

    /// Whether any round is open, filtered out or not.
    pub fn has_open(&self) -> bool {
        self.lock().rounds.iter().any(Round::is_open)
    }

    /// How many rounds are finished, filtered out or not: all that
    /// [`Desk::clear_history`] deletes.
    pub fn finished_count(&self) -> usize {
        self.lock().rounds.iter().filter(|r| !r.is_open()).count()
    }

    /// Finished rounds, most recently finished first.
    pub fn history(&self) -> Vec<HistoryEntry> {
        let state = self.lock();
        let mut history: Vec<HistoryEntry> = state
            .rounds
            .iter()
            .filter(|r| r.matches(&state.filter))
            .filter_map(|round| {
                Some(HistoryEntry {
                    id: round.id.clone(),
                    ticket: round.ticket.clone(),
                    title: round.title.clone(),
                    session: round.session.clone(),
                    outcome: round.outcome()?,
                    opened_at: round.opened_at,
                    finished_at: round.finished_at.unwrap_or(round.opened_at),
                    awaiting_pickup: round.delivered_at.is_none(),
                    answered: round.answered_count(),
                    total: round.questions.len(),
                })
            })
            .collect();
        history.sort_by_key(|e| std::cmp::Reverse(e.finished_at));
        history
    }

    /// Filters open rounds and History to the rounds matching all whitespace-separated
    /// terms of `query`, case-insensitively; an empty query shows all.
    pub fn set_filter(&self, query: &str) {
        self.lock().filter = search::terms(query);
    }

    /// The filter's terms, for highlighting the hits (see [`search::highlights`]).
    pub fn filter_terms(&self) -> Vec<String> {
        self.lock().filter.clone()
    }

    /// The round with this id as it is (or would be) stored, drafts included.
    pub fn export(&self, id: &str) -> Option<StoredRound> {
        let state = self.lock();
        state
            .rounds
            .iter()
            .find(|r| r.id == id)
            .map(Round::to_stored)
    }

    /// The JSON export of a round: the contents of its file in the store.
    pub fn export_json(&self, id: &str) -> Option<String> {
        self.export(id).map(|round| store::to_json(&round))
    }

    /// The selected round, open or finished, if any.
    pub fn current(&self) -> Option<RoundView> {
        let state = self.lock();
        let round = &state.rounds[state.selected_any()?];
        let questions = round
            .questions
            .iter()
            .zip(&round.answers)
            .enumerate()
            .map(|(i, (question, draft))| {
                let QuestionResult {
                    answer,
                    note: answer_note,
                    ..
                } = question_result(question, draft);
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
                        QuestionState::Unanswered
                    },
                    answer,
                    answer_note,
                }
            })
            .collect();
        Some(RoundView {
            id: round.id.clone(),
            ticket: round.ticket.clone(),
            session: round.session.clone(),
            title: round.title.clone(),
            step: round.step,
            questions,
            connected: round.connected(),
            opened_at: round.opened_at,
            finished: round.outcome().map(|outcome| FinishedView {
                outcome,
                finished_at: round.finished_at.unwrap_or(round.opened_at),
                delivered_at: round.delivered_at,
                can_reopen: round.delivered_at.is_none(),
            }),
        })
    }

    /// Selects the round with this id (see [`RoundSummary::id`]).
    pub fn select(&self, id: &str) {
        let mut state = self.lock();
        if let Some(round) = state.rounds.iter_mut().find(|r| r.id == id) {
            round.seen = true;
            state.selected = Some(id.to_owned());
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

    /// Makes the selected Awaiting Pickup round open again, on its Review Page; a Pickup
    /// then waits for the re-submit. Delivered rounds stay as they are.
    pub fn reopen(&self) {
        let mut state = self.lock();
        let Some(i) = state.selected_any() else {
            return;
        };
        let round = &mut state.rounds[i];
        if round.is_open() || round.delivered_at.is_some() {
            return;
        }
        round.result.send_replace(None);
        round.finished_at = None;
        round.step = Step::Review;
        state.save(i);
        drop(state);
        self.notify(DeskEvent::Changed);
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
        let open: Vec<usize> = (0..state.rounds.len())
            .filter(|&i| state.rounds[i].is_open())
            .collect();
        for i in open {
            state.rounds[i].finish(Outcome::Cancelled);
            state.save(i);
        }
        state.selected = None;
    }

    /// Delivers the selected round's result and shows the next open round.
    fn finish(&self, outcome: Outcome) {
        let mut state = self.lock();
        if let Some(i) = state.selected_index() {
            state.rounds[i].finish(outcome);
            state.save(i);
            state.select_first_open();
        }
    }

    /// Runs a draft edit on the selected open round; the store gets it debounced.
    fn with_selected(&self, f: impl FnOnce(&mut Round)) {
        let mut state = self.lock();
        if let Some(round) = state.selected_round_mut() {
            f(round);
            round.dirty_since.get_or_insert_with(Instant::now);
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
            round.id.clone()
        });
    }

    /// The selected round, open or finished.
    fn selected_any(&self) -> Option<usize> {
        let selected = self.selected.as_ref()?;
        self.rounds.iter().position(|r| &r.id == selected)
    }

    /// The selected round if it is open: only open rounds take user input.
    fn selected_index(&self) -> Option<usize> {
        let selected = self.selected.as_ref()?;
        self.rounds
            .iter()
            .position(|r| &r.id == selected && r.is_open())
    }

    fn selected_round(&self) -> Option<&Round> {
        self.selected_index().map(|i| &self.rounds[i])
    }

    fn selected_round_mut(&mut self) -> Option<&mut Round> {
        self.selected_index().map(|i| &mut self.rounds[i])
    }

    /// The round that answers to `ticket`: the latest one asked with it.
    fn holder(&self, ticket: &str) -> Option<usize> {
        self.rounds.iter().rposition(|r| r.ticket == ticket)
    }

    /// Adds `requester` to the Delivered round's Re-delivery Request, raising one if
    /// there is none, and attaches the call to its decision.
    fn request_redelivery(
        &mut self,
        i: usize,
        requester: Requester,
        desk: &Desk,
    ) -> (Attachment, DeskEvent) {
        let round = &mut self.rounds[i];
        if round
            .redelivery
            .as_ref()
            .is_some_and(|r| !r.answers(&requester.agent))
        {
            // Decided too long ago, or without this Agent: the user is asked again.
            round.redelivery = None;
        }
        let event = if round.redelivery.is_some() {
            DeskEvent::Changed
        } else {
            DeskEvent::RoundOpened
        };
        let request = round.redelivery.get_or_insert_with(|| Redelivery {
            requesters: Vec::new(),
            decision: watch::channel(None).0,
            decided_at: None,
        });
        if request.decided_at.is_none()
            && !request
                .requesters
                .iter()
                .any(|r| r.agent == requester.agent)
        {
            request.requesters.push(requester);
        }
        let decision = request.decision.subscribe();
        let result = round
            .result
            .borrow()
            .clone()
            .expect("a Delivered round is finished");
        let attachment = Attachment {
            ticket: round.ticket.clone(),
            wait: Wait::Redelivery { decision, result },
            attached: None,
            desk: desk.clone(),
            title: round.title.clone(),
            questions: round.questions.iter().map(|q| q.question.clone()).collect(),
        };
        (attachment, event)
    }

    /// An id for a new round with `ticket`, used by no round in memory and no file.
    fn new_id(&self, ticket: &str) -> String {
        let taken = |id: &str| {
            self.rounds.iter().any(|r| r.id == id)
                || self.store.as_ref().is_some_and(|s| s.exists(id))
        };
        if !taken(ticket) {
            return ticket.to_owned();
        }
        (2..)
            .map(|n| format!("{ticket}@{n}"))
            .find(|id| !taken(id))
            .expect("some suffix is free")
    }

    /// Queues writing round `i` to the store, if there is one.
    fn save(&mut self, i: usize) {
        let round = &mut self.rounds[i];
        round.dirty_since = None;
        let Some(store) = self.store.clone() else {
            return;
        };
        let (id, stored) = (round.id.clone(), round.to_stored());
        self.queue(&store, &id, Some(stored));
    }

    /// Queues a write (`None`: removal) of the file of round `id` in `store`.
    fn queue(&mut self, store: &Store, id: &str, write: Option<StoredRound>) {
        self.last_job += 1;
        self.jobs.push(Job {
            seq: self.last_job,
            store: store.clone(),
            id: id.to_owned(),
            write,
        });
    }

    /// Writes the drafts that have rested for at least `delay`.
    fn write_drafts(&mut self, delay: Duration) {
        let due: Vec<usize> = (0..self.rounds.len())
            .filter(|&i| {
                self.rounds[i]
                    .dirty_since
                    .is_some_and(|since| since.elapsed() >= delay)
            })
            .collect();
        for i in due {
            self.save(i);
        }
    }
}

/// Writes rested drafts every quarter second, and tries failed writes again, until the
/// desk is gone.
fn write_drafts_until_dropped(state: Weak<Mutex<State>>, disk: Arc<DiskQueue>) {
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let Some(state) = state.upgrade() else { return };
        Locked::new(&state, &disk).write_drafts(DRAFT_DELAY);
        disk.retry(RETRY_DELAY);
    }
}

impl Round {
    /// Whether all `terms` occur in the round: its title, Agent Session and Ticket, its
    /// Questions and Options, and the Notes. Previews are not searched.
    fn matches(&self, terms: &[String]) -> bool {
        if terms.is_empty() {
            return true;
        }
        let meta = self
            .title
            .as_deref()
            .into_iter()
            .chain([self.session.as_str(), self.ticket.as_str()]);
        let questions = self.questions.iter().flat_map(|q| {
            [q.question.as_str(), q.header.as_str()]
                .into_iter()
                .chain(q.body.as_deref())
                .chain(
                    q.options
                        .iter()
                        .flat_map(|o| [o.label.as_str(), o.description.as_str()]),
                )
        });
        let notes = self.answers.iter().map(|d| d.note.as_str());
        search::matches(meta.chain(questions).chain(notes), terms)
    }

    fn finish(&mut self, outcome: Outcome) {
        let result = self.result_for(outcome);
        self.result.send_replace(Some(result));
        self.finished_at = Some(SystemTime::now());
    }

    fn outcome(&self) -> Option<Outcome> {
        self.result.borrow().as_ref().map(|r| r.outcome)
    }

    fn to_stored(&self) -> StoredRound {
        StoredRound {
            id: self.id.clone(),
            ticket: self.ticket.clone(),
            session: self.session.clone(),
            title: self.title.clone(),
            questions: self.questions.clone(),
            answers: self
                .questions
                .iter()
                .zip(&self.answers)
                .map(|(question, draft)| DraftAnswer {
                    selected: draft
                        .selected
                        .iter()
                        .map(|&i| question.options[i].label.clone())
                        .collect(),
                    note: draft.note.clone(),
                })
                .collect(),
            step: match self.step {
                Step::Question(i) => Some(i),
                Step::Review => None,
            },
            status: match self.outcome() {
                None => store::Status::Open,
                Some(Outcome::Submitted) => store::Status::Answered,
                Some(Outcome::Cancelled) => store::Status::Cancelled,
            },
            opened_at: timestamp(self.opened_at),
            finished_at: self.finished_at.map(timestamp),
            delivered_at: self.delivered_at.map(timestamp),
        }
    }

    /// A round as it was stored; nothing is attached to it yet.
    fn restore(stored: StoredRound) -> Self {
        let answers = stored
            .questions
            .iter()
            .enumerate()
            .map(|(i, question)| {
                let saved = stored.answers.get(i).cloned().unwrap_or_default();
                let mut selected: Vec<usize> = question
                    .options
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| saved.selected.contains(&o.label))
                    .map(|(j, _)| j)
                    .collect();
                if !question.multi_select {
                    selected.truncate(1);
                }
                Draft {
                    selected,
                    note: saved.note,
                }
            })
            .collect();
        let step = match stored.step {
            Some(i) if i < stored.questions.len() => Step::Question(i),
            _ => Step::Review,
        };
        let mut round = Round {
            id: stored.id,
            ticket: stored.ticket,
            session: stored.session,
            title: stored.title,
            questions: stored.questions,
            answers,
            step,
            result: watch::channel(None).0,
            attached: Arc::new(()),
            opened_at: system_time(stored.opened_at),
            finished_at: stored.finished_at.map(system_time),
            delivered_at: stored.delivered_at.map(system_time),
            seen: true,
            dirty_since: None,
            redelivery: None,
        };
        let outcome = match stored.status {
            store::Status::Open => None,
            store::Status::Answered => Some(Outcome::Submitted),
            store::Status::Cancelled => Some(Outcome::Cancelled),
        };
        if let Some(outcome) = outcome {
            let result = round.result_for(outcome);
            round.result.send_replace(Some(result));
            round.finished_at.get_or_insert(round.opened_at);
        }
        round
    }

    fn connected(&self) -> bool {
        Arc::strong_count(&self.attached) > 1
    }

    fn answered_count(&self) -> usize {
        self.questions
            .iter()
            .zip(&self.answers)
            .filter(|(q, d)| question_result(q, d).answer.is_some())
            .count()
    }

    fn attach(&self, desk: &Desk) -> Attachment {
        Attachment {
            ticket: self.ticket.clone(),
            wait: Wait::Result(self.result.subscribe()),
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
                .map(|(question, draft)| question_result(question, draft))
                .collect(),
        }
    }
}

/// The Answer is the selected labels; without a selection the Note is the Answer.
fn question_result(question: &Question, draft: &Draft) -> QuestionResult {
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

const UNKNOWN_CLIENT: &str = "Unknown client";

/// The Agent Session label: the client's display name, plus the call's `source`.
fn session_label(client: Option<&Client>, source: Option<&str>) -> String {
    let client = client.map_or(UNKNOWN_CLIENT, Client::label);
    match source {
        Some(source) => format!("{client} · {source}"),
        None => client.to_owned(),
    }
}

fn timestamp(time: SystemTime) -> jiff::Timestamp {
    jiff::Timestamp::try_from(time).unwrap_or(jiff::Timestamp::UNIX_EPOCH)
}

fn system_time(timestamp: jiff::Timestamp) -> SystemTime {
    let nanos = u64::try_from(timestamp.as_nanosecond()).unwrap_or(0);
    SystemTime::UNIX_EPOCH + Duration::from_nanos(nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(name: &str) -> Client {
        Client {
            name: name.into(),
            title: None,
        }
    }

    #[tokio::test]
    async fn a_decision_expires_for_listed_agents_after_the_grace() {
        let desk = Desk::default();
        let input = crate::schema::parse_ask(serde_json::json!({
            "ticket": "db-1",
            "questions": [{ "question": "Which database?", "header": "Database" }]
        }))
        .unwrap();
        let mut asked = desk.ask(input, Some(client("claude"))).unwrap();
        desk.submit();
        asked.finished().await.unwrap();

        drop(desk.pickup("db-1", Some(client("codex"))).unwrap());
        let id = desk.redeliveries()[0].id.clone();
        desk.deliver_again(&id);
        desk.lock().rounds[0]
            .redelivery
            .as_mut()
            .unwrap()
            .decided_at = Some(Instant::now() - REDELIVERY_GRACE);

        let late = desk.pickup("db-1", Some(client("codex"))).unwrap();
        assert_eq!(desk.redeliveries().len(), 1, "asked again");
        assert!(late.is_redelivery());
    }
}

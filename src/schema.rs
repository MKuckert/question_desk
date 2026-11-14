//! Tool input and output types, and strict validation of the input.
//!
//! The input is a superset of the built-in AskUserQuestion schema (ADR 0004): same field
//! names, no count limits, plus `body`, `recommended`, `title`, `source` and the required
//! Agent-chosen `ticket` (ADR 0003).

use std::collections::{BTreeMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Longest Ticket we accept. Tickets are short ids, not text.
pub const MAX_TICKET_LEN: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AskInput {
    /// Your own id for this Question Round: short, unique, only `A-Z a-z 0-9 . _ -`,
    /// e.g. `db-choice-7f3`. Reuse it to pick up the answers if this call is dropped.
    pub ticket: String,
    /// The Questions of this round, in the order the user should see them.
    pub questions: Vec<Question>,
    /// Short title of the round, shown to the user.
    #[serde(default)]
    pub title: Option<String>,
    /// Where you are asking from (project name or working directory), so the user can
    /// tell your rounds apart from other Agents'.
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Question {
    /// The question text. Unique within the round; it keys the answer.
    pub question: String,
    /// Very short label shown as a chip, e.g. "Database".
    pub header: String,
    /// Optional Markdown with context, trade-offs and consequences.
    #[serde(default)]
    pub body: Option<String>,
    /// The choices. May be empty for open questions that only need free text.
    #[serde(default)]
    pub options: Vec<QuestionOption>,
    /// Allow selecting several Options.
    #[serde(default, rename = "multiSelect")]
    pub multi_select: bool,
    /// Your suggested answer: an Option label, or free text for a nuanced recommendation.
    #[serde(default)]
    pub recommended: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionOption {
    /// The choice as returned in the answer. Unique within the question.
    pub label: String,
    /// What this choice means and implies.
    pub description: String,
    /// Optional artifact to compare (code snippet, mockup), shown in monospace.
    #[serde(default)]
    pub preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WaitInput {
    /// The Ticket you chose in `ask_user_question`.
    pub ticket: String,
}

/// Parses tool arguments into `T`, with the JSON path of the offending field in the error.
pub fn parse<T: serde::de::DeserializeOwned>(arguments: serde_json::Value) -> Result<T, String> {
    serde_path_to_error::deserialize(arguments).map_err(|e| {
        let path = e.path().to_string();
        if path == "." {
            format!("invalid arguments: {}", e.inner())
        } else {
            format!("invalid arguments at `{path}`: {}", e.inner())
        }
    })
}

/// Parses and validates `ask_user_question` arguments. A call written exactly like the
/// built-in AskUserQuestion lacks `ticket`; the error says how to add one.
pub fn parse_ask(arguments: serde_json::Value) -> Result<AskInput, String> {
    if arguments.get("ticket").is_none() {
        return Err(MISSING_TICKET.into());
    }
    let input: AskInput = parse(arguments)?;
    input.validate()?;
    Ok(input)
}

const MISSING_TICKET: &str = "`ticket` is required: choose a short unique id for this round, \
     e.g. `db-choice-7f3`, and reuse it to pick up the answers if the call is dropped";

impl AskInput {
    /// Checks everything serde cannot; the message tells the Agent what to fix.
    pub fn validate(&self) -> Result<(), String> {
        validate_ticket(&self.ticket)?;
        if self.questions.is_empty() {
            return Err("`questions` must contain at least one question".into());
        }
        let mut texts = HashSet::new();
        for (i, q) in self.questions.iter().enumerate() {
            let at = format!("questions[{i}]");
            if q.question.trim().is_empty() {
                return Err(format!("`{at}.question` must not be empty"));
            }
            if !texts.insert(q.question.as_str()) {
                return Err(format!(
                    "`{at}.question` duplicates an earlier question ({:?}); question texts must be unique within a round because they key the answers",
                    q.question
                ));
            }
            if q.multi_select && q.options.is_empty() {
                return Err(format!(
                    "`{at}.multiSelect` is true but the question has no options; add options or drop `multiSelect`"
                ));
            }
            if q.recommended
                .as_deref()
                .is_some_and(|r| r.trim().is_empty())
            {
                return Err(format!(
                    "`{at}.recommended` must not be empty; omit it instead"
                ));
            }
            let mut labels = HashSet::new();
            for (j, o) in q.options.iter().enumerate() {
                if o.label.trim().is_empty() {
                    return Err(format!("`{at}.options[{j}].label` must not be empty"));
                }
                if !labels.insert(o.label.as_str()) {
                    return Err(format!(
                        "`{at}.options[{j}].label` duplicates an earlier option ({:?}); option labels must be unique within a question",
                        o.label
                    ));
                }
            }
        }
        Ok(())
    }
}

pub fn validate_ticket(ticket: &str) -> Result<(), String> {
    if ticket.is_empty() {
        return Err(MISSING_TICKET.into());
    }
    if ticket.len() > MAX_TICKET_LEN {
        return Err(format!(
            "`ticket` must be at most {MAX_TICKET_LEN} characters"
        ));
    }
    if let Some(c) = ticket
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')))
    {
        return Err(format!(
            "`ticket` contains {c:?}; only A-Z, a-z, 0-9, `.`, `_` and `-` are allowed"
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Answered,
    Cancelled,
    Pending,
}

/// `structuredContent` of both tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Output {
    /// `answered` or `cancelled` when the round is finished; `pending` from
    /// `wait_for_answers` while the user is still answering.
    pub status: Status,
    pub ticket: String,
    /// Question text → Answer. Multiple selections are comma-joined.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub answers: BTreeMap<String, String>,
    /// Question text → the user's Note on it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, Annotation>,
    /// Question texts the user left Unanswered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unanswered: Vec<String>,
    /// While pending: the round's title, to confirm it is yours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// While pending: the round's question texts, to confirm it is yours.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Annotation {
    pub notes: String,
}

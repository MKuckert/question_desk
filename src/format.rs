//! Turns a finished Question Round into what the Agent receives.

use pulldown_cmark::{Event, HeadingLevel, Tag, TagEnd};

use crate::schema::{Annotation, Output, Status};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundResult {
    pub ticket: String,
    pub outcome: Outcome,
    pub questions: Vec<QuestionResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Submitted,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionResult {
    pub question: String,
    pub answer: Option<String>,
    pub note: Option<String>,
}

/// The `structuredContent` of a finished round.
pub fn structured(result: &RoundResult) -> Output {
    let questions = &result.questions;
    Output {
        status: match result.outcome {
            Outcome::Submitted => Status::Answered,
            Outcome::Cancelled => Status::Cancelled,
        },
        ticket: result.ticket.clone(),
        answers: questions
            .iter()
            .filter_map(|q| Some((q.question.clone(), q.answer.clone()?)))
            .collect(),
        annotations: questions
            .iter()
            .filter_map(|q| {
                Some((
                    q.question.clone(),
                    Annotation {
                        notes: q.note.clone()?,
                    },
                ))
            })
            .collect(),
        unanswered: unanswered(result).map(str::to_owned).collect(),
        title: None,
        questions: Vec::new(),
    }
}

/// What `wait_for_answers` returns while the user is still answering: text and
/// `structuredContent`, both naming the round so the Agent can confirm it is its own.
pub fn pending(ticket: &str, title: Option<&str>, questions: &[String]) -> (String, Output) {
    let questions_list = quoted_list(questions.iter().map(String::as_str));
    let round = match title {
        Some(title) => format!("{}: {questions_list}", quoted(title)),
        None => questions_list,
    };
    let text = format!(
        "Still pending: the user has not finished question round {} ({round}) yet. Call wait_for_answers again.",
        quoted(ticket)
    );
    let output = Output {
        status: Status::Pending,
        ticket: ticket.to_owned(),
        answers: Default::default(),
        annotations: Default::default(),
        unanswered: Vec::new(),
        title: title.map(str::to_owned),
        questions: questions.to_vec(),
    };
    (text, output)
}

/// What `wait_for_answers` returns while the user has not decided a Re-delivery Request.
pub fn pending_redelivery(
    ticket: &str,
    title: Option<&str>,
    questions: &[String],
) -> (String, Output) {
    let (_, output) = pending(ticket, title, questions);
    let text = format!(
        "Still pending: the answers of question round {} were already delivered once, and the user has not decided yet whether to deliver them again. Call wait_for_answers again.",
        quoted(ticket)
    );
    (text, output)
}

/// The text content of a finished round: one line for short Answers, Markdown blocks
/// once any Answer or Note is long or spans several lines.
pub fn text(result: &RoundResult) -> String {
    if needs_blocks(result) {
        blocks(result)
    } else {
        one_line(result)
    }
}

/// Answers and Notes longer than this switch the round to the block shape.
const MAX_ONE_LINE_LEN: usize = 100;

fn needs_blocks(result: &RoundResult) -> bool {
    result
        .questions
        .iter()
        .flat_map(|q| [&q.answer, &q.note])
        .flatten()
        .any(|text| text.contains('\n') || text.chars().count() > MAX_ONE_LINE_LEN)
}

/// One `## question` section per Question with its Answer and Note.
fn blocks(result: &RoundResult) -> String {
    let intro = match result.outcome {
        Outcome::Submitted => {
            "Your questions have been answered. You can now continue with these answers in mind."
        }
        Outcome::Cancelled => {
            "The user cancelled the question round. Answers given before cancelling:"
        }
    };
    let mut events = paragraph(intro);
    for q in &result.questions {
        events.push(Event::Start(Tag::Heading {
            level: HeadingLevel::H2,
            id: None,
            classes: vec![],
            attrs: vec![],
        }));
        events.push(Event::Text(q.question.clone().into()));
        events.push(Event::End(TagEnd::Heading(HeadingLevel::H2)));
        match &q.answer {
            Some(answer) => events.extend(labelled("Answer:", answer)),
            None => events.extend([
                Event::Start(Tag::Paragraph),
                Event::Start(Tag::Emphasis),
                Event::Text("Unanswered".into()),
                Event::End(TagEnd::Emphasis),
                Event::End(TagEnd::Paragraph),
            ]),
        }
        if let Some(note) = &q.note {
            events.extend(labelled("Note:", note));
        }
    }
    let mut out = String::new();
    pulldown_cmark_to_cmark::cmark(events.iter(), &mut out, pulldown_cmark::Options::empty())
        .expect("events built here are always valid Markdown");
    out
}

fn paragraph(text: &str) -> Vec<Event<'static>> {
    vec![
        Event::Start(Tag::Paragraph),
        Event::Text(text.to_owned().into()),
        Event::End(TagEnd::Paragraph),
    ]
}

/// `**label** text`, keeping the user's line breaks; the text is escaped by the writer.
/// Blank lines start further paragraphs, so the text stays inside the section.
fn labelled(label: &'static str, text: &str) -> Vec<Event<'static>> {
    let mut paragraphs: Vec<Vec<&str>> = vec![Vec::new()];
    for line in text.lines() {
        match paragraphs.last_mut() {
            Some(current) if line.trim().is_empty() => {
                if !current.is_empty() {
                    paragraphs.push(Vec::new());
                }
            }
            Some(current) => current.push(line),
            None => unreachable!("starts with one paragraph"),
        }
    }
    paragraphs.retain(|p| !p.is_empty());

    let mut events = Vec::new();
    for (i, lines) in paragraphs.iter().enumerate() {
        events.push(Event::Start(Tag::Paragraph));
        if i == 0 {
            events.extend([
                Event::Start(Tag::Strong),
                Event::Text(label.into()),
                Event::End(TagEnd::Strong),
                Event::Text(" ".into()),
            ]);
        }
        for (j, line) in lines.iter().enumerate() {
            if j > 0 {
                events.push(Event::SoftBreak);
            }
            events.push(Event::Text((*line).to_owned().into()));
        }
        events.push(Event::End(TagEnd::Paragraph));
    }
    events
}

fn one_line(result: &RoundResult) -> String {
    let answered: Vec<String> = result
        .questions
        .iter()
        .filter_map(|q| {
            let answer = q.answer.as_ref()?;
            let pair = format!("{}={}", quoted(&q.question), quoted(answer));
            Some(match &q.note {
                Some(note) => format!("{pair} (note: {})", quoted(note)),
                None => pair,
            })
        })
        .collect();
    let unanswered = quoted_list(unanswered(result));
    let mut out = match result.outcome {
        _ if answered.is_empty() => format!(
            "The user {} the question round without answering any questions.",
            match result.outcome {
                Outcome::Submitted => "submitted",
                Outcome::Cancelled => "cancelled",
            }
        ),
        Outcome::Submitted => format!(
            "Your questions have been answered: {}.",
            answered.join(", ")
        ),
        Outcome::Cancelled => format!(
            "The user cancelled the question round. Answers given before cancelling: {}.",
            answered.join(", ")
        ),
    };
    if !unanswered.is_empty() {
        out += &format!(" Unanswered: {unanswered}.");
    }
    if let Outcome::Submitted = result.outcome
        && !answered.is_empty()
    {
        out += " You can now continue with these answers in mind.";
    }
    out
}

fn unanswered(result: &RoundResult) -> impl Iterator<Item = &str> {
    result
        .questions
        .iter()
        .filter(|q| q.answer.is_none())
        .map(|q| q.question.as_str())
}

fn quoted_list<'a>(items: impl Iterator<Item = &'a str>) -> String {
    items.map(quoted).collect::<Vec<_>>().join(", ")
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

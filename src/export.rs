//! Exporting one Question Round, open or finished: Markdown for reading, JSON (the stored
//! file) for tools.

use jiff::tz::TimeZone;
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::{
    format::labelled,
    store::{Status, StoredRound},
};

/// The round as Markdown: title, Agent Session, Ticket and times, then per Question its
/// text, body, all Options with the chosen ones checked, the Answer and the Note. Times
/// are shown in `tz`. Text from the Agent and the user is escaped, except the Markdown
/// bodies, which are quoted.
pub fn markdown(round: &StoredRound, tz: &TimeZone) -> String {
    let mut events = Vec::new();
    heading(
        &mut events,
        HeadingLevel::H1,
        round.title.as_deref().unwrap_or(&round.ticket),
    );
    let time = |t: jiff::Timestamp| {
        t.to_zoned_datetime(tz.clone())
            .strftime("%Y-%m-%d %H:%M %Z")
            .to_string()
    };
    let mut meta = vec![
        format!("Agent Session: {}", round.session),
        format!("Ticket: {}", round.ticket),
        format!("Opened: {}", time(round.opened_at)),
    ];
    let finished = round.finished_at.map(time).unwrap_or_default();
    meta.push(match round.status {
        Status::Open => "Status: open, the answers are drafts".to_owned(),
        Status::Answered => format!("Answered: {finished}"),
        Status::Cancelled => format!("Cancelled: {finished}"),
    });
    list(&mut events, meta.iter().map(|m| (None, m.clone())));

    for (i, question) in round.questions.iter().enumerate() {
        let draft = round.answers.get(i).cloned().unwrap_or_default();
        heading(
            &mut events,
            HeadingLevel::H2,
            &format!("{}. {}", i + 1, question.question),
        );
        events.extend(emphasis(&question.header));
        if let Some(body) = &question.body {
            events.push(Event::Start(Tag::BlockQuote(None)));
            events.extend(Parser::new(body).map(Event::into_static));
            events.push(Event::End(TagEnd::BlockQuote(None)));
        }
        list(
            &mut events,
            question.options.iter().map(|o| {
                let mut text = o.label.clone();
                if !o.description.is_empty() {
                    text += &format!(": {}", o.description);
                }
                if question.recommended.as_ref() == Some(&o.label) {
                    text += " (recommended)";
                }
                (Some(draft.selected.contains(&o.label)), text)
            }),
        );
        let selected: Vec<&str> = question
            .options
            .iter()
            .filter(|o| draft.selected.contains(&o.label))
            .map(|o| o.label.as_str())
            .collect();
        let note = draft.note.trim();
        let (answer, note) = match (selected.is_empty(), note.is_empty()) {
            (true, true) => (None, None),
            (true, false) => (Some(note.to_owned()), None),
            (false, true) => (Some(selected.join(", ")), None),
            (false, false) => (Some(selected.join(", ")), Some(note)),
        };
        match answer {
            Some(answer) => events.extend(labelled("Answer:", &answer)),
            None => events.extend(emphasis("Unanswered")),
        }
        if let Some(note) = note {
            events.extend(labelled("Note:", note));
        }
    }
    let mut out = String::new();
    pulldown_cmark_to_cmark::cmark(events.iter(), &mut out, pulldown_cmark::Options::empty())
        .expect("events built here are always valid Markdown");
    out.push('\n');
    out
}

fn heading(events: &mut Vec<Event<'static>>, level: HeadingLevel, text: &str) {
    events.push(Event::Start(Tag::Heading {
        level,
        id: None,
        classes: vec![],
        attrs: vec![],
    }));
    events.push(Event::Text(text.to_owned().into()));
    events.push(Event::End(TagEnd::Heading(level)));
}

fn emphasis(text: &str) -> Vec<Event<'static>> {
    vec![
        Event::Start(Tag::Paragraph),
        Event::Start(Tag::Emphasis),
        Event::Text(text.to_owned().into()),
        Event::End(TagEnd::Emphasis),
        Event::End(TagEnd::Paragraph),
    ]
}

/// A bullet list; items with `Some(checked)` get a task list marker.
fn list(events: &mut Vec<Event<'static>>, items: impl Iterator<Item = (Option<bool>, String)>) {
    let items: Vec<_> = items.collect();
    if items.is_empty() {
        return;
    }
    events.push(Event::Start(Tag::List(None)));
    for (checked, text) in items {
        events.push(Event::Start(Tag::Item));
        if let Some(checked) = checked {
            events.push(Event::TaskListMarker(checked));
        }
        events.push(Event::Text(text.into()));
        events.push(Event::End(TagEnd::Item));
    }
    events.push(Event::End(TagEnd::List(false)));
}

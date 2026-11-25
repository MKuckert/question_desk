//! Markdown `body` → blocks for the Slint `MarkdownView`.
//!
//! Slint 1.18's `StyledText` renders a CommonMark subset: paragraphs, emphasis, strong,
//! strikethrough, inline code, links and (nested) lists. It rejects headings, code blocks,
//! tables, block quotes, rules and images. So the body is split into top-level blocks here:
//! paragraphs and lists go to `StyledText` as they are; headings, code blocks, tables, rules and
//! quotes become their own block kinds, with their inline content again as `StyledText`
//! (table cells excepted, see [`table`]).

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use slint::{ModelRc, SharedString, StyledText, VecModel};

use super::MdBlock;

const PARAGRAPH: i32 = 0;
const HEADING: i32 = 1;
const CODE: i32 = 2;
const TABLE_ROW: i32 = 3;
const RULE: i32 = 4;
const QUOTE: i32 = 5;

pub fn blocks(markdown: &str) -> Vec<MdBlock> {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let events: Vec<Event> = Parser::new_ext(markdown, options).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < events.len() {
        let end = block_end(&events, i);
        let block = &events[i..end];
        match &events[i] {
            Event::Start(Tag::Heading { level, .. }) => out.push(MdBlock {
                kind: HEADING,
                level: *level as i32,
                text: inline(&block[1..block.len() - 1], true),
                ..Default::default()
            }),
            Event::Start(Tag::CodeBlock(_)) => out.push(MdBlock {
                kind: CODE,
                code: SharedString::from(text_of(block).trim_end_matches('\n')),
                ..Default::default()
            }),
            Event::Start(Tag::Table(_)) => table(block, &mut out),
            Event::Start(Tag::BlockQuote(_)) => out.push(MdBlock {
                kind: QUOTE,
                text: styled(&block[1..block.len() - 1]),
                ..Default::default()
            }),
            Event::Rule => out.push(MdBlock {
                kind: RULE,
                ..Default::default()
            }),
            _ => out.push(MdBlock {
                kind: PARAGRAPH,
                text: styled(block),
                ..Default::default()
            }),
        }
        i = end;
    }
    out
}

/// Index after the top-level block starting at `start`.
fn block_end(events: &[Event], start: usize) -> usize {
    let mut depth = 0usize;
    for (i, event) in events.iter().enumerate().skip(start) {
        match event {
            Event::Start(_) => depth += 1,
            Event::End(_) => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return i + 1;
        }
    }
    events.len()
}

/// Table cells are plain `Text`: `StyledText` reserves an extra line of height, which makes
/// every row twice as tall. Inline styling inside cells is dropped.
fn table(block: &[Event], out: &mut Vec<MdBlock>) {
    let mut row: Vec<SharedString> = Vec::new();
    let mut cell_start = None;
    let mut header = false;
    for (i, event) in block.iter().enumerate() {
        match event {
            Event::Start(Tag::TableHead) => header = true,
            Event::Start(Tag::TableCell) => cell_start = Some(i + 1),
            Event::End(TagEnd::TableCell) => {
                if let Some(start) = cell_start.take() {
                    row.push(text_of(&block[start..i]).trim().into());
                }
            }
            Event::End(TagEnd::TableHead | TagEnd::TableRow) => {
                out.push(MdBlock {
                    kind: TABLE_ROW,
                    cells: ModelRc::new(VecModel::from(std::mem::take(&mut row))),
                    header,
                    ..Default::default()
                });
                header = false;
            }
            _ => {}
        }
    }
}

/// Inline events (no surrounding block) as one paragraph, optionally bold.
fn inline(events: &[Event], strong: bool) -> StyledText {
    let mut wrapped = vec![Event::Start(Tag::Paragraph)];
    if strong {
        wrapped.push(Event::Start(Tag::Strong));
    }
    wrapped.extend(events.iter().cloned());
    if strong {
        wrapped.push(Event::End(TagEnd::Strong));
    }
    wrapped.push(Event::End(TagEnd::Paragraph));
    styled(&wrapped)
}

/// Block events as `StyledText`; falls back to the plain text if Slint rejects the Markdown.
fn styled(events: &[Event]) -> StyledText {
    // StyledText starts a new line at every soft break; CommonMark renders it as a space.
    let spaced = events.iter().map(|e| match e {
        Event::SoftBreak => Event::Text(" ".into()),
        other => other.clone(),
    });
    let mut markdown = String::new();
    if pulldown_cmark_to_cmark::cmark(spaced, &mut markdown, Options::empty()).is_ok() {
        match StyledText::from_markdown(markdown.trim_end()) {
            Ok(text) => return text,
            Err(e) => log::debug!("StyledText rejected {markdown:?}: {e}"),
        }
    }
    StyledText::from_plain_text(text_of(events).trim_end())
}

/// The plain text of `events`, with line breaks between blocks.
fn text_of(events: &[Event]) -> String {
    let mut text = String::new();
    for event in events {
        match event {
            Event::Text(t) | Event::Code(t) | Event::Html(t) | Event::InlineHtml(t) => {
                text.push_str(t)
            }
            Event::SoftBreak => text.push(' '),
            Event::HardBreak | Event::End(TagEnd::Paragraph | TagEnd::Item) => text.push('\n'),
            _ => {}
        }
    }
    text
}

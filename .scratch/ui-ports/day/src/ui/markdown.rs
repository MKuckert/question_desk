//! A Question's Markdown `body` as native text.
//!
//! Day's `markdown` piece draws headings, paragraphs with inline styles and links, lists,
//! quotes, code blocks and rules as native labels. It has no tables, so GitHub tables are cut
//! out here (pulldown-cmark finds them) and drawn as a `grid` of labels, each cell with its own
//! inline Markdown.

use day::prelude::*;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// The `body` as a column of native Markdown blocks and tables.
pub(crate) fn body(text: &str) -> impl Piece + use<> {
    let mut parts: Vec<AnyPiece> = Vec::new();
    let flush = |parts: &mut Vec<AnyPiece>, source: &str| {
        if !source.trim().is_empty() {
            parts.push(
                markdown(source.trim().to_owned())
                    .selectable_text()
                    .any(),
            );
        }
    };
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut last = 0;
    let mut table: Option<Vec<Vec<String>>> = None;
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(Tag::Table(_)) => {
                flush(&mut parts, &text[last..range.start]);
                table = Some(Vec::new());
            }
            Event::Start(Tag::TableHead | Tag::TableRow) => {
                if let Some(rows) = &mut table {
                    rows.push(Vec::new());
                }
            }
            Event::Start(Tag::TableCell) => {
                if let Some(row) = table.as_mut().and_then(|rows| rows.last_mut()) {
                    let cell = text[range].trim().trim_matches('|').trim();
                    row.push(cell.to_owned());
                }
            }
            Event::End(TagEnd::Table) => {
                if let Some(rows) = table.take() {
                    parts.push(table_piece(rows).any());
                }
                last = range.end;
            }
            _ => {}
        }
    }
    flush(&mut parts, &text[last..]);
    column(parts).spacing(Space::M)
}

/// A table: the header row in bold, a rule under it, then the body rows.
fn table_piece(rows: Vec<Vec<String>>) -> impl Piece {
    let mut children: Vec<AnyPiece> = Vec::new();
    for (i, row) in rows.into_iter().enumerate() {
        let cells: Vec<AnyPiece> = row
            .into_iter()
            .map(|cell| {
                let text = label(cell).markdown().selectable();
                if i == 0 { text.bold().any() } else { text.any() }
            })
            .collect();
        children.push(grid_row(cells).any());
        if i == 0 {
            children.push(divider().any());
        }
    }
    grid(children)
        .column_spacing(Space::L)
        .row_spacing(Space::XS)
        .align(Alignment::TopLeading)
}

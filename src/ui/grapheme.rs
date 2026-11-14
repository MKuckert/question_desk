//! Grapheme-aware editing for egui's `TextEdit`.
//!
//! egui 0.37 deletes and moves by `char` (Unicode scalar), so Backspace takes a ZWJ family
//! apart person by person and a flag falls back to its regional indicator letters.
//! [`GraphemeText`] deletes whole grapheme clusters, and [`snap_cursor`] keeps the cursor
//! off positions inside a cluster (arrow keys, clicks).

use eframe::egui::{
    self, TextBuffer,
    text::{CCursor, CCursorRange, CharIndex},
};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation as _;

/// A `String` that egui's `TextEdit` edits grapheme by grapheme on Backspace/Delete.
pub struct GraphemeText<'a>(pub &'a mut String);

impl TextBuffer for GraphemeText<'_> {
    fn is_mutable(&self) -> bool {
        true
    }

    fn as_str(&self) -> &str {
        self.0
    }

    fn insert_text(&mut self, text: &str, char_index: CharIndex) -> usize {
        self.0.insert_text(text, char_index)
    }

    fn delete_char_range(&mut self, char_range: Range<CharIndex>) {
        self.0.delete_char_range(char_range);
    }

    fn type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<GraphemeText<'static>>()
    }

    fn delete_previous_char(&mut self, ccursor: CCursor) -> CCursor {
        let start = previous_boundary(self.0, ccursor.index.0);
        self.delete_selected_ccursor_range([CCursor::new(CharIndex(start)), ccursor])
    }

    fn delete_next_char(&mut self, ccursor: CCursor) -> CCursor {
        let end = next_boundary(self.0, ccursor.index.0);
        self.delete_selected_ccursor_range([ccursor, CCursor::new(CharIndex(end))])
    }
}

/// Show `edit` (built over a [`GraphemeText`]) and move a cursor that ended up inside a
/// grapheme cluster to the cluster's edge: backwards after ArrowLeft, forwards otherwise.
pub fn show(ui: &mut egui::Ui, edit: egui::TextEdit<'_>) -> egui::Response {
    let moving_left = ui.input(|i| i.key_pressed(egui::Key::ArrowLeft));
    let mut output = edit.show(ui);
    if let Some(range) = output.cursor_range {
        let text = output.galley.text();
        let snapped = snap_cursor(text, range, moving_left);
        if snapped != range {
            output.state.cursor.set_char_range(Some(snapped));
            output.state.store(ui.ctx(), output.response.response.id);
        }
    }
    output.response.response
}

pub fn snap_cursor(text: &str, range: CCursorRange, backwards: bool) -> CCursorRange {
    let snap = |c: CCursor| {
        let index = c.index.0;
        if is_boundary(text, index) {
            c
        } else if backwards {
            CCursor {
                index: CharIndex(previous_boundary(text, index)),
                ..c
            }
        } else {
            CCursor {
                index: CharIndex(next_boundary(text, index)),
                ..c
            }
        }
    };
    CCursorRange {
        primary: snap(range.primary),
        secondary: snap(range.secondary),
        ..range
    }
}

/// Char indices where grapheme clusters start, plus the end of the text.
fn boundaries(text: &str) -> impl Iterator<Item = usize> + '_ {
    let mut chars = 0;
    text.graphemes(true)
        .map(move |g| {
            let start = chars;
            chars += g.chars().count();
            start
        })
        .chain(std::iter::once(text.chars().count()))
}

fn is_boundary(text: &str, index: usize) -> bool {
    boundaries(text).any(|b| b == index)
}

fn previous_boundary(text: &str, index: usize) -> usize {
    boundaries(text)
        .take_while(|&b| b < index)
        .last()
        .unwrap_or(0)
}

fn next_boundary(text: &str, index: usize) -> usize {
    boundaries(text).find(|&b| b > index).unwrap_or(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backspace_at_end(text: &str) -> String {
        let mut s = text.to_owned();
        let end = CCursor::new(CharIndex(s.chars().count()));
        GraphemeText(&mut s).delete_previous_char(end);
        s
    }

    fn delete_at_start(text: &str) -> String {
        let mut s = text.to_owned();
        GraphemeText(&mut s).delete_next_char(CCursor::new(CharIndex(0)));
        s
    }

    #[test]
    fn backspace_removes_whole_clusters() {
        for cluster in ["👨‍👩‍👧", "🇩🇪", "👍🏽", "❤️", "é" /* e + U+0301 */, "ü", "a"]
        {
            assert_eq!(backspace_at_end(&format!("x{cluster}")), "x", "{cluster:?}");
        }
    }

    #[test]
    fn delete_removes_whole_clusters() {
        for cluster in ["👨‍👩‍👧", "🇩🇪", "👍🏽", "❤️", "é", "ü", "a"] {
            assert_eq!(delete_at_start(&format!("{cluster}x")), "x", "{cluster:?}");
        }
    }

    #[test]
    fn backspace_at_start_and_delete_at_end_do_nothing() {
        let mut s = "🇩🇪".to_owned();
        GraphemeText(&mut s).delete_previous_char(CCursor::new(CharIndex(0)));
        GraphemeText(&mut s).delete_next_char(CCursor::new(CharIndex(2)));
        assert_eq!(s, "🇩🇪");
    }

    #[test]
    fn cursor_inside_cluster_snaps_to_its_edges() {
        let text = "a👨‍👩‍👧b"; // family = chars 1..6
        let inside = CCursorRange::one(CCursor::new(CharIndex(3)));
        assert_eq!(snap_cursor(text, inside, false).primary.index, CharIndex(6));
        assert_eq!(snap_cursor(text, inside, true).primary.index, CharIndex(1));
        let on_edge = CCursorRange::one(CCursor::new(CharIndex(6)));
        assert_eq!(snap_cursor(text, on_edge, true), on_edge);
    }
}

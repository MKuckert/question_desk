//! Backspace over emoji.
//!
//! Slint's `TextInput` moves the cursor and forward-deletes by grapheme cluster, but Backspace
//! deliberately removes a single code point (so that e.g. Devanagari can be corrected letter by
//! letter). For emoji that leaves debris: 👩‍🍳 becomes "👩‍", 🇩🇪 becomes "🇩". When an edit
//! removed just the last code point of an emoji cluster, [`repair_backspace`] removes the rest
//! of that cluster too, as AppKit's own text fields do.

use unicode_segmentation::UnicodeSegmentation as _;

/// If `new` is `old` minus the last code point of an emoji grapheme cluster, returns the text
/// without the whole cluster and the byte offset for the cursor.
pub fn repair_backspace(old: &str, new: &str) -> Option<(String, usize)> {
    if new.len() >= old.len() {
        return None;
    }
    // The single removed code point: common prefix, then exactly one char, then common suffix.
    let prefix = old
        .char_indices()
        .zip(new.chars())
        .take_while(|((_, a), b)| a == b)
        .count();
    let at = old.char_indices().nth(prefix)?.0;
    let removed = old[at..].chars().next()?;
    if old[at + removed.len_utf8()..] != new[at..] {
        return None;
    }
    let (start, cluster) = old
        .grapheme_indices(true)
        .find(|(start, g)| (*start..start + g.len()).contains(&at))?;
    let was_last = at + removed.len_utf8() == start + cluster.len();
    if !was_last || cluster.chars().count() < 2 || !is_emoji_cluster(cluster) {
        return None;
    }
    let repaired = format!("{}{}", &old[..start], &old[start + cluster.len()..]);
    Some((repaired, start))
}

/// ZWJ sequences, flags, keycaps, skin tones and emoji presentation selectors.
fn is_emoji_cluster(cluster: &str) -> bool {
    cluster.chars().any(|c| {
        matches!(c,
            '\u{200D}' | '\u{FE0F}' | '\u{20E3}'
            | '\u{1F1E6}'..='\u{1F1FF}'
            | '\u{1F3FB}'..='\u{1F3FF}'
            | '\u{E0020}'..='\u{E007F}')
    })
}

#[cfg(test)]
mod tests {
    use super::repair_backspace;

    #[test]
    fn removes_rest_of_zwj_sequence() {
        let old = "a👩‍🍳";
        let new = "a👩\u{200D}";
        assert_eq!(repair_backspace(old, new), Some(("a".into(), 1)));
    }

    #[test]
    fn removes_rest_of_flag() {
        assert_eq!(repair_backspace("x🇩🇪y", "x🇩y"), Some(("xy".into(), 1)));
    }

    #[test]
    fn leaves_plain_text_alone() {
        assert_eq!(repair_backspace("abc", "ab"), None);
        assert_eq!(repair_backspace("e\u{301}", "e"), None);
        assert_eq!(repair_backspace("ab", "abc"), None);
    }
}

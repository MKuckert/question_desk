//! The filter over Question Rounds: case-insensitive substring terms that must all match
//! within one round.

use std::ops::Range;

/// The lowercase, whitespace-separated terms of a filter query.
pub fn terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

/// Whether every term occurs in at least one of `texts`.
pub fn matches<'a>(texts: impl Iterator<Item = &'a str> + Clone, terms: &[String]) -> bool {
    terms.iter().all(|term| {
        texts
            .clone()
            .any(|text| text.to_lowercase().contains(term.as_str()))
    })
}

/// Byte ranges of `text` where any of `terms` occurs, sorted and merged.
pub fn highlights(text: &str, terms: &[String]) -> Vec<Range<usize>> {
    // Lowercasing can change byte lengths, so map each lowercase byte to its source char.
    let mut lower = String::with_capacity(text.len());
    let mut origin: Vec<Range<usize>> = Vec::with_capacity(text.len());
    for (start, c) in text.char_indices() {
        let source = start..start + c.len_utf8();
        for l in c.to_lowercase() {
            lower.push(l);
            origin.extend(std::iter::repeat_n(source.clone(), l.len_utf8()));
        }
    }
    let mut ranges: Vec<Range<usize>> = terms
        .iter()
        .filter(|t| !t.is_empty())
        .flat_map(|term| {
            lower
                .match_indices(term.as_str())
                .map(|(at, m)| origin[at].start..origin[at + m.len() - 1].end)
                .collect::<Vec<_>>()
        })
        .collect();
    ranges.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

# 04 — Search filter over open rounds and History

Status: ready-for-agent
Blocked by: 03
Spec: `../spec.md` (story 12)

## What

- A filter field at the top of the sidebar (shortcut e.g. `⌘F` or `/`) filters open rounds and History in place.
- Case-insensitive substring match; all whitespace-separated terms must match within one round.
- Fields matched: title, Agent Session label, Ticket; question text, header, body; Option labels and descriptions; Answers and Notes, drafts included. Previews are not matched.
- The opened round highlights the hits.

## Acceptance

- [ ] Read-model tests for each field and the multi-term AND.
- [ ] Previews don't match.
- [ ] Highlighting checked via `tools/screenshot.sh`.

## Comments

- Implemented on branch `persistence`: `src/search.rs` (terms, matching, highlight ranges), `Desk::set_filter` / `filter_terms`. ⌘F (Ctrl+F) focuses the field and Esc clears it. Hits are highlighted in question texts, Option labels and descriptions, and in the Answers and Notes on the Review and finished pages. Markdown bodies and the Note text field are matched but not highlighted.
- Re-delivery banners are not filtered.
- The highlight screenshot check could not run here: window capture is not permitted in this environment.

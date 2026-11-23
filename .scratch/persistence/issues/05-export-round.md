# 05 — Export a round as Markdown or JSON

Status: ready-for-agent
Blocked by: 03
Spec: `../spec.md` (story 13)

## What

- "Export round…" on any round, open or finished (round header and context menu), with a save dialog offering Markdown or JSON.
- JSON = the persisted file contents.
- Markdown = title, Agent Session, Ticket, times, then per Question its text, body, all Options (chosen ones marked), the Answer and the Note. Open rounds export with their drafts and are marked as open.

## Acceptance

- [ ] `insta` snapshots of the Markdown export: answered, cancelled, open with drafts, Unanswered, hostile Markdown, emoji.
- [ ] JSON export equals the stored file.

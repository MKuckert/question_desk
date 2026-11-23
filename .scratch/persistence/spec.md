# Spec: Persisting Question Rounds and History

Status: ready-for-agent

Related: ADR 0003, ADR 0007, `GLOSSARY.md` (History, Delivered, Awaiting Pickup, Re-delivery Request).

## Problem Statement

Question Rounds live only in memory. If the Question Desk quits or crashes during a 20-minute grilling, I lose my draft Answers, the open rounds, and any result my Agent has not picked up yet. Once a round is finished it disappears from view, so I can't look back at what I answered, search it or export it.

## Solution

Every Question Round, open or finished, is persisted as one JSON file per round in my data dir. After a restart the open rounds, drafts and undelivered results are back, and Agents can Pick Up by Ticket as before. Finished rounds form a History below the open rounds: flat, newest first, collapsible, searchable, exportable per round, deletable. A round's Ticket is released when its result is Delivered. If an Agent asks for a Delivered result again, I confirm or decline the re-delivery in the app.

## User Stories

### Restart safety
1. As a user, I want open rounds and my draft Answers and Notes to survive a quit or crash, so that I never re-answer a long grilling.
2. As an Agent, I want to Pick Up the result of a round after the desk restarted, so that a restart doesn't lose answers I haven't received.
3. As a user, I want restored open rounds to show as disconnected, with their opened time in the header, so that I can tell stale rounds apart.

### Delivery and Tickets
4. As an Agent, I want a Ticket to be released once its result was Delivered, so that reusing it later with different Questions opens a new round instead of failing.
5. As a user, I want a Re-delivery Request when an Agent asks for a Delivered result again (same Questions or `wait_for_answers`), so that I decide whether an old result is sent again.
   - The round surfaces at the top of the open section with a banner: *"Agent Y wants to receive answers for 'ABC', already delivered yesterday at 14:50."* [Deliver again] [Decline].
   - It counts towards the open-rounds chip.
6. As an Agent, I want my call to block with Heartbeat (and `wait_for_answers` to return `pending`) while the Re-delivery Request is undecided, and to get the old result or a clear decline error, so that I know whether to ask again with a new Ticket.
7. As a user, I want undelivered finished rounds badged *Awaiting Pickup*, so that I know which results no Agent has seen yet.
8. As a user, I want to reopen an Awaiting Pickup round and change my Answers, so that I can correct them before any Agent sees them. A Pickup meanwhile waits for my re-submit.

### History
9. As a user, I want finished rounds listed flat, newest first, below the open rounds, in a section I can collapse to the bottom and expand again with one navigation.
10. As a user, I want history round headers to show opened and finished time.
11. As a user, I want finished rounds read-only, but showing every Option (not just the selected ones), so that I can see what I chose from.
12. As a user, I want a filter field that filters open rounds and History in place, highlighting hits in the opened round, so that I find past decisions. It matches round meta (title, Agent Session, Ticket), Questions (text, header, body), Options (labels, descriptions), and Answers and Notes, drafts included. It doesn't match previews.
13. As a user, I want to export a single round (open or finished) as Markdown or JSON through a save dialog.

### Control and privacy
14. As a user, I want to delete a round or a whole Agent Session, and clear all History from Settings, each confirmed. Deleting an open round cancels it first, so its Agent receives `cancelled`.
15. As a user, I want `retention_days` (default 90, `0` = forever) to prune finished rounds automatically.
16. As a user, I want `data_dir` to move future storage to another folder without moving existing files.
17. As a user, I want to switch persistence off live, with an offer to delete the stored rounds.
18. As a user, I want hand edits to `config.toml` picked up when the window gains focus and before each prune.

## Implementation Decisions

See ADR 0007 for storage, format evolution, retention and Ticket release. Additional details:

- File per round: `<data_dir>/rounds/<ticket>.json`, `version` field. Atomic temp + rename; `0600` files, `0700` dir.
- Written on open, Submit/Cancel and Delivery; drafts debounced ~1 s and flushed on quit. Startup load is timed and logged.
- Unparseable or newer-version files are skipped in place with a warning, and never pruned or modified.
- Retention counts from finish time. Pruning runs at startup and daily, re-reading `config.toml` first. Open rounds are never pruned.
- Delivered = the result was written to an HTTP response. Re-delivery Requests are in memory only. Several requests for one Ticket fold into one banner, and one decision answers all of them. "Agent Y" is the Agent Session label of the requesting call.
- Decline error text: *"the user declined re-delivery of ticket X; ask again with a new ticket or clarify with the user first."*
- Persistence off applies live: in-memory rounds stay for the running session, and nothing more is written or pruned.
- Changing `data_dir` switches folders: in-memory rounds are written to the new folder, and the old folder is left untouched.
- Server settings (host/port/token) still need a restart.

## Testing Decisions

- Same primary seam as the MVP: a headless Question Desk (real server on an ephemeral port plus `desk`), now pointed at a temp `data_dir`. "Restart" = drop the desk and server, start new ones on the same dir.
- Covered through the seam: restore of open rounds and drafts; Pickup after restart; Delivery releasing the Ticket; new round on reuse with different Questions; Re-delivery Request accept, decline, pending and folding; reopen + Pickup waiting; retention pruning; skipped corrupt/newer files; delete-cancels-open; persistence off.
- Unit/snapshot tests for the file format (serialisation round-trip, migration from older versions) and Markdown export (`insta`).
- Search filtering is tested on the `desk` read model.

## Out of Scope

- Sync across machines, multi-user/per-token stores, encryption at rest.
- Export of Agent Sessions or of the filtered list; clipboard export.
- A file watcher for `config.toml`; live server-setting changes.
- Editing Delivered rounds.

## Comments

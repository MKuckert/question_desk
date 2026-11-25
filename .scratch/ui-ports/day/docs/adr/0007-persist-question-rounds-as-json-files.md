# 7. Persist Question Rounds as one JSON file per round, with Delivery-based Ticket release

Status: accepted (2026-11-23). Revises the consequences of ADR 0003.

## Context

ADR 0003 kept Question Rounds and their results in memory until the app quits, and named persistence across restarts and history/export as post-MVP requirements. Grilling rounds take 10–30 minutes; a crash or quit loses the round, the user's draft Answers, and any result the Agent has not picked up yet. The user also wants to look back at past rounds.

Persisting rounds for months breaks an ADR 0003 assumption: Tickets were unique for the app's lifetime. With long-lived rounds, an Agent reusing a Ticket weeks later would hit a mismatch error, or silently get an old result.

The server cannot know whether an Agent *saw* a result: Claude Code backgrounds long calls, and the background task dies with its session (spike 2).

## Decision

**Storage.** Every Question Round, open or finished, is one JSON file `<data_dir>/rounds/<ticket>.json`. It holds the Questions, the Agent Session label, timestamps, the Answers and Notes (including drafts of open rounds), the status and the Delivery state.
- The default `data_dir` is the XDG data dir (`$XDG_DATA_HOME` or `~/.local/share/question-desk`; Windows `ProjectDirs::data_dir()`). It matches `config.toml`'s XDG-style location.
- Writes are atomic (temp file + rename). Files are `0600`, the directory `0700`.
- Files are written on round open, on Submit/Cancel and on Delivery. Drafts are written debounced (~1 s) and on quit.
- All files are loaded at startup, and the load time is logged.
- One store per OS user. There is no file lock; a second instance already fails to bind the port.

**Format evolution.** Each file carries a `version`; older versions are migrated in memory on load. Unparseable files and files from a newer version are skipped in place with a warning. They are never modified or pruned.

**Retention.** Finished rounds are pruned `retention_days` (default 90, `0` = forever) after they finished. Pruning runs at startup and then daily, and re-reads `config.toml` first. Open rounds are never pruned. `data_dir` and a persistence on/off switch are also settings.

**Ticket release on Delivery.** A finished round is *Delivered* once its result was written to an HTTP response; this releases its Ticket. After that:
- `ask_user_question` with the Ticket and **different** Questions opens a new round. The old one stays in History.
- `ask_user_question` with the **same** Questions, or `wait_for_answers`, raises a **Re-delivery Request**: the user decides in the app whether to deliver the old result again or decline. While it is pending, the call blocks with Heartbeat, and `wait_for_answers` returns `pending`. Declining is a tool error telling the Agent to ask with a new Ticket or clarify with the user first. Re-delivery Requests are not persisted.

A finished round that was never delivered (*Awaiting Pickup*) keeps answering to its Ticket as before. The user can reopen it, which makes it open again: a Pickup then waits for the re-submit.

After a restart, restored open rounds have no attached call and show as disconnected, like a dropped call today.

## Considered alternatives

- **SQLite** or an **append-only JSONL log**: SQLite would make search queryable and the log would make writes cheap. But round data is small (KBs, hundreds of rounds). A file per round is human-readable, easy to export and grep, and has no migrations engine or compaction.
- **An explicit `commit` tool** the Agent calls after reading the result: more precise about "seen", but Agents forget extra steps, and uncommitted rounds would hold their Ticket indefinitely.
- **Supersede on reuse** (no release; a mismatched reuse replaces a finished round): no new UI, but re-delivery would happen silently. The confirmation keeps the user in control when an old Agent session resumes, or an Agent behaves oddly.
- **A pickup window** (Tickets valid ~24 h after finishing): an arbitrary cut-off that fails resumed sessions.

## Consequences

- Rounds, drafts and undelivered results survive quit and crash; History becomes possible (search, per-round export, delete).
- Answers now rest on disk. Owner-only permissions, retention, delete/clear-all and the persistence switch are the privacy controls; there is no encryption at rest.
- Results are delivered at most once without asking. A resumed Agent session that lost its result needs one confirmation click.
- The glossary gains *History*, *Delivered*, *Awaiting Pickup*, *Re-delivery Request*. Ticket and Pickup are revised.
- `config.toml` is re-read when the window gains focus and before pruning, so persistence settings apply live without a file watcher. Server settings still need a restart.

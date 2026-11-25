# 01 — Round store: persist and restore Question Rounds

Status: ready-for-agent
Spec: `../spec.md` (stories 1–3) · ADR 0007

## What

Persist every Question Round as `<data_dir>/rounds/<ticket>.json` and restore all rounds at startup.

- Default `data_dir`: `$XDG_DATA_HOME/question-desk` or `~/.local/share/question-desk`; Windows `ProjectDirs::data_dir()`. Hard-coded default for now; the setting comes in 06.
- The file holds the Questions, Agent Session label, `opened_at`, `finished_at`, status, Answers, Notes, draft state and a Delivery flag (only written as `false` until 02). `version: 1`, with a migration hook on load.
- Atomic temp + rename; file `0600`, dir `0700`.
- Written on open and Submit/Cancel. Drafts are written debounced ~1 s after the last edit, and flushed on quit.
- Startup load is timed and logged (count + duration).
- Unparseable or newer-version files: log a warning, skip, never touch.
- Restored open rounds show as disconnected. Every round header shows its opened time.
- After a restart, `wait_for_answers` and reattach work for restored rounds, as today.

## Acceptance

- [ ] Headless test: open a round, set draft answers, "restart" on the same dir → round and drafts are back.
- [ ] Headless test: submit, restart, `wait_for_answers(ticket)` → result returned.
- [ ] Corrupt and future-version files are skipped and left byte-identical.
- [ ] Serialisation round-trip and v1 migration hook covered by unit tests.
- [ ] Opened time visible in the header (check with `tools/screenshot.sh`).

## Comments

- Implemented on branch `persistence`: `src/store.rs` (format v1, migration hook, atomic owner-only writes), `Desk::with_store`, debounced draft writer, flush on quit. Quitting with a store keeps open rounds instead of cancelling them. Round ids are the Ticket, plus `@<n>` when a file or round already uses it.
- The screenshot check could not run here: `screencapture` cannot capture the window from this agent's environment. `tools/screenshot.sh` now starts the app on temp config and data dirs (`-d` to choose one).

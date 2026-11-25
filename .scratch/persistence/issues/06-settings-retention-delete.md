# 06 — Retention, data_dir, persistence switch, delete and clear all

Status: ready-for-agent
Blocked by: 01, 03
Spec: `../spec.md` (stories 14–18) · ADR 0007

## What

- New `config.toml` keys, also editable in the Settings view:
  - `retention_days` (default 90, `0` = forever)
  - `data_dir` (default XDG data dir)
  - `persistence` (default on)
- `config.toml` is re-read when the window gains focus and before each prune. Server settings still need a restart.
- Pruning: startup + daily. It re-reads the config first, prunes finished rounds older than `retention_days` since finish, and never touches open or unreadable files.
- `data_dir` change: write in-memory rounds to the new folder and use it from then on. The old folder is left untouched.
- Persistence off, live: stop writing and pruning, keep in-memory rounds, and offer *"Also delete N stored rounds?"* [Keep] [Delete]. This also applies when the change comes from a hand edit picked up on focus.
- Delete one round, delete an Agent Session, and clear all (Settings), each confirmed. Deleting an open round cancels it first, so its Agent gets `cancelled`.

## Acceptance

- [ ] Headless tests: pruning by age, `0` = forever, config re-read before prune.
- [ ] Headless tests: data_dir switch, persistence off (no writes, delete offer).
- [ ] Headless tests: delete cancels an open round.
- [ ] Config tests for the new keys and defaults (`tests/config.rs`).

## Comments

- Implemented on branch `persistence`. New config keys `retention_days`, `data_dir` (absolute or `~/…`) and `persistence` can also be set in Settings, together with "Clear all History…". `src/housekeeping.rs` re-reads the config, applies the storage settings and prunes, at startup and then daily. On window focus the storage settings are re-read and applied, without pruning. Deleting a round or an Agent Session is in the context menu of a sidebar entry. Deleting a round with a pending Re-delivery Request declines the request.
- Switching persistence on (or to a new `data_dir`) writes the rounds in memory to the folder; rounds already stored there are not loaded until the next start. A file there is never overwritten: a round whose file name is taken gets the next free `<ticket>@<n>` id. Deleting a round removes its file even while persistence is off. The delete offer only removes the files it counted, and nothing once its folder is in use again.
- Changed after review: `persistence` and `data_dir` now apply at the next start only (ADR 0007); Settings warns that a restart is needed. Removed with it: re-reading the config on window focus, moving rounds to a new `data_dir`, and the *"Also delete N stored rounds?"* offer. With persistence off, nothing is read or written; delete stored rounds by hand or before switching off. A second instance on the same `data_dir` keeps its rounds in memory only.
- A Re-delivery decision answers the calls waiting at that moment, plus the Agents listed in the banner for 2 minutes (one polling `wait_for_answers` may be between calls). Anyone else, or later, raises a new request.
- Handing a result to a call marks the round Delivered in the same locked step, so it cannot be reopened in between. File writes are queued under the desk lock and carried out after it is released.

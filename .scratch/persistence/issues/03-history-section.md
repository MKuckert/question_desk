# 03 — History section

Status: ready-for-agent
Blocked by: 01, 02
Spec: `../spec.md` (stories 7–11) · ADR 0007

## What

- A History section below the open rounds lists finished rounds flat, newest first, each tagged with its Agent Session label.
- It collapses to the bottom of the sidebar and expands back with a single navigation (click or shortcut).
- History round headers show opened and finished time ("answered 14:50" / "cancelled …").
- Finished rounds open read-only: every Question with **all** Options visible, the chosen ones marked, plus Notes.
- An **Awaiting Pickup** badge marks undelivered finished rounds.
- An Awaiting Pickup round can be reopened. It becomes open again, and a Pickup/reattach waits for the re-submit. Delivered rounds can't be reopened.

## Acceptance

- [ ] Read-model tests: History ordering, badge state, reopen allowed only when undelivered.
- [ ] Headless test: reopen, then `wait_for_answers` → pending until re-submit, then the new answers.
- [ ] Collapsed and expanded states checked via `tools/screenshot.sh`.

## Comments

- Implemented on branch `persistence`. History is collapsed by default; clicking its header or pressing ⌘Y (Ctrl+Y) toggles it. Reopening puts the round on its Review Page.
- The collapsed/expanded screenshot check could not run here: window capture is not permitted in this environment.

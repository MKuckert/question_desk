# 02 — Delivery, Ticket release and Re-delivery Request

Status: ready-for-agent
Blocked by: 01
Spec: `../spec.md` (stories 4–6) · ADR 0007

## What

- Mark a finished round **Delivered** once its result was written to an HTTP response, and persist that.
- Delivered releases the Ticket: `ask_user_question` with the Ticket and **different** Questions opens a new round. The old round's file must not be overwritten, so the file name needs a disambiguator (e.g. `<ticket>-<n>.json`) or an id.
- The same Questions, or `wait_for_answers` on a Delivered round, create a **Re-delivery Request**:
  - The round surfaces at the top of the open section with a banner: *"<requesting Agent Session> wants to receive answers for '<title or first question>', already delivered <relative time>."* [Deliver again] [Decline].
  - It counts towards the open-rounds chip.
  - The call blocks with Heartbeat, and `wait_for_answers` returns `pending`.
  - Several requests for one Ticket fold into one banner, and one decision answers all.
  - In memory only; not persisted.
  - Deliver again sends the original result with its original status. Decline returns the tool error *"the user declined re-delivery of ticket X; ask again with a new ticket or clarify with the user first."*
- Update the tool descriptions (`src/server.rs`) to explain Delivery and Re-delivery.

## Acceptance

- [ ] Headless tests: delivery releases the Ticket; reuse with different Questions → new round, old one intact.
- [ ] Headless tests: same Questions → request appears, accept delivers the old result, decline gives the error.
- [ ] Headless tests: pending `wait_for_answers` during a request, two requests folded.
- [ ] Banner checked via `tools/screenshot.sh`.

## Comments

- Implemented on branch `persistence`. Later rounds that reuse a released Ticket are stored as `<ticket>@<n>.json` (`@` cannot occur in a Ticket). A decision made while no call is waiting (between two `wait_for_answers` calls) is kept for the next call with that Ticket.
- The banner check could not be done with `tools/screenshot.sh` here: window capture is not permitted in this environment.

# 3. Blocking call with Heartbeat, plus Ticket-based Pickup as fallback

Status: accepted (2026-11-12), revised after spike 2 (2026-11-13); consequences on persistence and Ticket lifetime revised by ADR 0007

## Context

Grilling-style Question Rounds can take 10–30 minutes to answer. MCP clients impose timeouts, and other clients may be stricter than Claude Code.

Spike 2 (`spikes/02-mcp-heartbeat/`, Claude Code 2.1.311) measured Claude Code's behaviour over stateless Streamable HTTP:

- Without progress notifications, a blocking call fails client-side with "The operation timed out" after about 3 minutes. SSE keep-alive comments do not count as activity.
- With a progress notification every 15 s, a 7-minute call stayed open and delivered its result.
- After 2 minutes Claude Code moves the call to a background task. The request stays open; the result arrives as a task-completion notification the Agent acts on. The task dies with the Claude Code session.
- Esc or a client timeout cancels the server-side request within the same second.
- The model sees neither progress messages nor anything from the server in the timeout error. So a Ticket the server assigns and only returns with the result never reaches the Agent when the call is dropped; Pickup did not work in the spike without the Agent reading the server's source.

## Decision

`ask_user_question` blocks until the user Submits or Cancels, sending a Heartbeat progress notification roughly every 15 s when the client supplied a progress token.

**The Agent chooses the Ticket.** `ask_user_question` requires a `ticket` string the Agent picks (short, unique, e.g. `db-choice-7f3`). Because it is part of the Agent's own tool call, the Agent still knows it after any drop.

- Calling `ask_user_question` again with a known Ticket and the same Questions reattaches to that Question Round instead of opening a new one: it blocks on the open round, or returns the result immediately if the round is already finished. With different Questions it is a tool error.
- `wait_for_answers(ticket)` blocks up to ~50 s and returns either the result or `pending`. A pending result includes the round's `title` and question texts so the Agent can confirm it is its own round.
- Unknown Tickets are a tool error.

A lost connection never removes a Question Round from the UI. Rounds and their results stay in memory until the app quits, so a result can be delivered again on reattach or Pickup.

## Consequences

- Natural one-call usage for Agents in the common case; a dropped call is recovered by repeating the same call or by `wait_for_answers`, without asking the user again.
- One required field beyond AskUserQuestion's schema (see ADR 0004). Agents that omit it get a validation error telling them to add one.
- Two Agents choosing the same Ticket collide: with identical Questions they share a round, otherwise the second gets an error. The tool description asks for a random suffix.
- In the MVP, rounds and results are lost if the app quits. Persisting them across restarts is a post-MVP requirement; history/export is planned after the MVP too.
- The README setup must deny the built-in `AskUserQuestion`; Claude Code prefers it otherwise. `--disallowedTools AskUserQuestion` and `permissions.deny: ["AskUserQuestion"]` both work.

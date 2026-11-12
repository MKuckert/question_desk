# 3. Blocking call with Heartbeat, plus Ticket-based Pickup as fallback

Status: accepted (2026-11-12)

## Context

Grilling-style Question Rounds can take 10–30 minutes to answer. MCP clients impose timeouts: Claude Code has a ~28 h wall-clock default but an idle timeout of 5 min for HTTP servers (progress notifications count as activity per its docs), and moves MCP calls running longer than 2 min to a background task. Other clients may be stricter.

## Decision

`ask_user_question` blocks until the user Submits or Cancels, sending a Heartbeat progress notification roughly every 15 s when the client supplied a progress token. Every Question Round has a Ticket, which the tool description and any timeout path expose to the Agent. A second tool, `wait_for_answers(ticket)`, blocks up to ~50 s and returns either the result or "still pending".

A lost connection never removes a Question Round from the UI. Submitted results that were not delivered stay in memory until picked up (or the app quits).

## Consequences

- Natural one-call usage for Agents in the common case, no frustration when a client drops the call.
- Results are lost if the app quits before Pickup; history/export is planned after the MVP.
- Real-world client behaviour (idle timeout, auto-backgrounding) must be verified in a spike and documented per client.

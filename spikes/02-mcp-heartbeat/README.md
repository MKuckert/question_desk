# Spike 2: multi-minute blocking call with Heartbeat in Claude Code over HTTP

Gate for ADR 0003. Throwaway crate; not part of the Question Desk build.

Stack: `rmcp` 3.9 (`transport-streamable-http-server`, stateless: `legacy_session_mode = false`) mounted in `axum` 0.8, `tokio`, logging via `tracing` → `env_logger`.

## Questions this spike answers

1. **Idle timeout.** Do Heartbeat progress notifications (every ~15 s) keep `CLAUDE_CODE_MCP_TOOL_IDLE_TIMEOUT` (documented default 5 min for HTTP) from aborting a blocking call? Without them, does the call really die after ~5 min? Do SSE keep-alive comments count as activity?
2. **Auto-backgrounding.** What happens after `CLAUDE_CODE_MCP_AUTO_BACKGROUND_MS` (documented default 2 min)? Does the HTTP request stay open? Does the Agent still get the result, and when?
3. **Disabling the built-in tool.** Does `--disallowedTools AskUserQuestion` or `permissions.deny: ["AskUserQuestion"]` remove the built-in tool, so the Agent uses `ask_user_question` from this server?
4. **Disconnect indicator.** When Claude Code drops a call (Esc, timeout), does the server see it (stream closed, request cancellation token, `notifications/cancelled`)? How fast?
5. **Pickup.** After a drop, can the Agent recover the Answers with `wait_for_answers(ticket)`? Does it know the Ticket (from the tool description, the progress messages, or the error)?
6. **Client identity.** What does Claude Code send in `initialize` (`clientInfo`, protocol version, capabilities)? Does it send `progressToken` on `tools/call`?

## What the server does

- `POST /mcp` on `127.0.0.1:<port>`; optional bearer token (`--token` or `SPIKE_TOKEN`) → 401; any `Origin` header → 403.
- `ask_user_question` takes `questions[]` (`question`, `header`, `options`, `multiSelect`), opens a round with a Ticket (`t1`, `t2`, …) and **blocks until you answer in the server's terminal**. While it waits it sends a Heartbeat `notifications/progress` every `--heartbeat` seconds, with a message that contains the Ticket, if the client sent a `progressToken`.
- `wait_for_answers(ticket)` blocks up to `--pickup-wait` seconds (default 50) and returns the result or `status: pending`. Unknown Ticket → tool error.
- The result text has the AskUserQuestion shape (`Your questions have been answered: "Q"="A". …`) plus `structuredContent` (`status`, `ticket`, `answers`) and an `outputSchema`. The one answer you type is used for every question in the round.
- The log shows every HTTP request (JSON-RPC method, id, `MCP-Protocol-Version`, session header, user agent), when each response stream closes and how long it was open, `initialize` details, `progressToken` present or missing, each Heartbeat, call cancellations and `notifications/cancelled`.

Console commands (type into the server terminal):

```text
ls                    list rounds: age, state, blocked calls, delivered?
a <ticket> <answer>   Submit (answer defaults to "yes")
c <ticket>            Cancel
```

## How to run

The server must run outside the nono sandbox (Claude Code has to reach it, and you need its terminal). Build once:

```sh
cd spikes/02-mcp-heartbeat
cargo build --release
./target/release/spike-mcp-heartbeat --token spike        # defaults: --port 7457 --heartbeat 15 --sse-keep-alive 15
./target/release/spike-mcp-heartbeat --help
RUST_LOG=info,spike_mcp_heartbeat=debug ./target/release/spike-mcp-heartbeat --token spike   # also logs request bodies
```

Register it in Claude Code for this project only (`local` scope: private, not written to the repo):

```sh
claude mcp add --transport http --scope local question-desk-spike http://127.0.0.1:7457/mcp \
  --header "Authorization: Bearer spike"
claude mcp remove --scope local question-desk-spike    # afterwards
```

Smoke-tested with curl (2026-11-13): initialize, tools/list, 401 without token, 403 with `Origin`, Heartbeats on a blocking call, result + `structuredContent`, client drop → `call CANCELLED` logged immediately when the stream closed, Answer submitted afterwards picked up via `wait_for_answers`, Cancel, unknown Ticket.

## Test protocol (Claude Code)

Start a fresh `claude` session for each experiment unless noted. Keep the server log; note wall-clock times from the Claude Code side too (what the UI shows at 2 min, 5 min).

**E1, baseline (~1 min).** Server with defaults. Prompt: *"Use the ask_user_question tool from question-desk-spike to ask me whether to ship today."* Answer after ~60 s with `a t1 yes`. Expect: result arrives, Agent quotes it. Check in the log: `initialize` clientInfo + protocol version, `progressToken` present, Heartbeats sent.

**E2, long call with Heartbeat (~7 min).** Server with defaults. Same prompt, answer after ~7 min. Watch: what the UI does at 2 min (auto-background?), whether the stream stays open past 5 min (no `response stream closed` before the answer), whether and when the Agent receives the result, and in what form (inline vs. background task notification).

**E3, no Heartbeat, with SSE keep-alive (~7 min).** `--heartbeat 0`. Same as E2. Does the idle timeout fire at ~5 min (`call CANCELLED` + `response stream closed` around 5m00s)? If not, SSE keep-alive comments count as activity.

**E4, no Heartbeat, no keep-alive (~7 min).** `--heartbeat 0 --sse-keep-alive 0`. Same as E2. Expect the idle timeout at ~5 min. Then answer with `a t1 …` and tell the Agent *"the call failed, try to recover my answer"*. Does it find the Ticket and call `wait_for_answers`?

**E5, Esc interrupt and Pickup.** Server with defaults. Start a round, press Esc after ~30 s. Log: `notifications/cancelled` and/or stream closed, how fast? Answer with `a t1 …`, then prompt *"I answered in the desk, pick it up."* Does the Agent know the Ticket without being told?

**E6, short idle timeout (optional, faster E3/E4).** Start Claude Code with `CLAUDE_CODE_MCP_TOOL_IDLE_TIMEOUT=60000` (if the variable name/unit is right) and repeat E2/E4 at minute scale.

**E7, disabling the built-in tool.**
- `claude --disallowedTools AskUserQuestion`, prompt: *"Ask me which database to use, with three options."* Which tool does it use? Then: *"List the tools you have whose names contain 'question'."*
- Same with `.claude/settings.local.json` → `{"permissions": {"deny": ["AskUserQuestion"]}}` and plain `claude`.
- Control: plain `claude` without either. Does it prefer the built-in tool?

**E8, two concurrent calls (optional).** Two Claude Code sessions ask at the same time; answer in reverse order. Both results arrive at the right session.

## Results

2.1.311 (Claude Code)

E1: I answered free text form after ~1 minute, the agent confirmed the right answer and that there was no backgrounding involved.

E2: After around 2 minutes the agent wrote:
I asked "Should we ship today?" through question-desk-spike, with two options: "Yes, ship today" and "No, wait". You haven't answered yet. After 120 seconds the call moved to the background as task kimb4wrpm, and it's still running.

Answer in the question-desk UI and I'll get a notification with your choice. The call won't survive if this session exits. If it gets cut off, I can use wait_for_answers with the round's ticket to collect your answer without asking again.

Nothing happened after 5 minutes.

After around 7 minutes i answered "yes", the agent got the answer:

⏺ MCP task kimb4wrpm (question-desk-spike/ask_user_question) completed.

You answered "yes" to "Should we ship today?" (ticket t2). The background task finished, so the heartbeat path delivered the answer.

The spike returned the raw value "yes", not the option label I offered ("Yes, ship today"). The built-in AskUserQuestion returns the selected label, so you may want the spike to match that.

I haven't taken any shipping action, since you only asked me to ask the question. Tell me if you want me to start the release.


E3: After 3 minutes the agent got a time out.

The call timed out without returning a Ticket, so I'm trying to resume from t1, which is how the spike's source numbers them.

  Called question-desk-spike 2 times

I asked whether to ship today, but I don't have your answer yet.

- The call timed out. ask_user_question failed with "The operation timed out." Neither the error nor any progress message I received included a Ticket.
- I recovered the Ticket by guessing it from the code. src/main.rs:133 numbers Tickets t1, t2, … in order, so I tried t1 with wait_for_answers rather than asking again. It exists, but I can't confirm it's my round: the reply doesn't include the question text. If the server already had a round before mine, my question is a later Ticket.
- Pickup: I called wait_for_answers("t1") twice, about 50 seconds each, and both returned pending.

For the README's Pickup question: I didn't learn the Ticket from the error or progress messages. I only found it by reading the server source.

To finish, answer in the question-desk server terminal with a t1 yes (or a t1 no), and tell me when you have. I'll pick it up with wait_for_answers. The server's round list will also show whether t1 is the "Should we ship today?" round.


I answered the question in the shell, but the agent wasn't listening anymore. It picked the answer up after instructing to do so.
So seems like the heartbeat is necessary.

E4: Skipped as I expect a similar pattern as my observations of E3. Say so if we should test this too.

E5: Pressed ESC after ~30s. Log pretty instantly:
[2026-11-13T19:59:12Z INFO  rmcp::service] task cancelled
[2026-11-13T19:59:12Z WARN  spike_mcp_heartbeat] call CANCELLED (client disconnected or sent notifications/cancelled); round stays open for pickup tool="ask_user_question" ticket="t1" elapsed=0m24s
[2026-11-13T19:59:12Z ERROR rmcp::service] failed to send pending response during drain error=channel closed
[2026-11-13T19:59:12Z INFO  rmcp::service] serve finished quit_reason=Cancelled

The agent got no ticket and can't wait for an answer.

E6: skipped

E7: The agent choose our spike tool.

Two tools have "question" in their names, and both come from your spike's MCP server:

1. mcp__question-desk-spike__ask_user_question: asks one or more questions and blocks until you answer.
2. mcp__question-desk-spike__wait_for_answers: picks up a round's answers by its ticket. It blocks for about 50 seconds and returns pending if you haven't answered yet.

The second one matches only because the server name, question-desk-spike, contains "question"; the tool name itself doesn't.

The built-in AskUserQuestion tool isn't available in this session, so your spike's server is the only way I can ask you structured questions here.

--disallowedTools and the settings.local.json file behave the same.

claude without limitations used AskUserQuestion tool.

E8: Yes, the calls arrive at the correct session.


## Conclusions (2026-11-13, Claude Code 2.1.311)

1. **Heartbeat is required, and it works.** With progress notifications every 15 s, a call survived 7 minutes and delivered the result (E2). Without them the call failed with a client-side "The operation timed out" after about 3 minutes (E3), even though SSE keep-alive comments were sent every 15 s. Keep-alive comments do **not** count as activity; only progress notifications do. E4 (no keep-alive either) was skipped because it can only time out sooner. The observed ~3 min is shorter than the documented 5 min idle default; the exact value doesn't matter as long as the Heartbeat interval stays well below it.
2. **Auto-backgrounding is harmless.** After 120 s Claude Code moves the call to a background task. The HTTP request stays open, the Heartbeat keeps it alive, and the result arrives as a task-completion notification that the Agent acts on. The background task dies with the Claude Code session.
3. **Disabling the built-in tool works.** `--disallowedTools AskUserQuestion` and `permissions.deny: ["AskUserQuestion"]` both remove it, and the Agent uses `ask_user_question` (E7). Without either, Claude Code prefers the built-in tool, so the README setup must include the deny rule.
4. **Disconnects are detected immediately.** Esc cancelled the server-side call within the same second (E5), and so did a client timeout (curl). The round stays open, so the disconnect indicator can rely on the request's cancellation token.
5. **Pickup is broken as designed: the Agent never learns the Ticket.** Progress messages are not shown to the model, and the timeout error is generated by the client, so it contains nothing from the server (E3, E5). The Agent only found `t1` by reading this spike's source. Once it was told the answer was in, `wait_for_answers` worked. A server-assigned Ticket that is only returned with the result cannot support Pickup; ADR 0003 needs a different way for the Agent to know the Ticket up front.
6. **`wait_for_answers` results should identify the round.** The Agent could not confirm `t1` was its own round because "pending" did not include the question text or title.
7. **Concurrent sessions work** (E8): each result reached the session that asked.
8. **Client identity.** Claude Code 2.1.311 speaks protocol `2026-07-28`. It opens with `server/discover` (no `initialize`, so the spike's `initialize` log line never fired) and sends its identity in the request `_meta`:

   ```json
   "io.modelcontextprotocol/protocolVersion": "2026-07-28",
   "io.modelcontextprotocol/clientInfo": {"name": "claude-code", "title": "Claude Code", "version": "2.1.311", ...}
   ```

   In `2026-07-28` per-request `clientInfo` is optional. rmcp reads it with `RequestContext::client_info()`; `context.peer.peer_info()` is a placeholder (`rmcp 3.9.0`) in stateless mode and must not be used. Verified on `tools/call` too: its `_meta` carries the same `protocolVersion`, `clientInfo` and `clientCapabilities`, plus `progressToken` (a number) and a Claude Code-specific `claudecode/toolUseId` (`toolu_…`). The tool-use ID is new on every call, so it cannot replace the Agent-chosen Ticket for reattach.

Side note from E2: the Agent remarked that the raw answer "yes" was returned instead of an option label. That's an artefact of the spike's console (free text for every question), not a format issue.

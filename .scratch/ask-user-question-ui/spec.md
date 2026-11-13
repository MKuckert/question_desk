# Spec: Question Desk — a GUI replacement for AskUserQuestion

Status: ready-for-agent

Related: `GLOSSARY.md`, ADR 0001–0005 in `docs/adr/`.

## Problem Statement

When an Agent asks me questions through the built-in AskUserQuestion tool, I am limited to 1–4 short questions with 2–4 options each, rendered in a cramped terminal widget. That works for quick clarifications, but not for grilling sessions where an Agent wants to walk me through many substantial decisions, each with context, trade-offs, and a recommendation. I end up answering long questions in plain chat, losing structure, and the Agent loses the clean, keyed answers it gets from AskUserQuestion. When several Agents run in parallel, I have no single place to see and answer what each of them is waiting on. My Agents also run in a nono sandbox, so anything they spawn cannot reliably open windows.

## Solution

The Question Desk: a desktop app I start myself, outside the sandbox, which hosts an MCP server on localhost. Any MCP-capable Agent connects to it over HTTP and calls `ask_user_question` with the same shape it already knows from AskUserQuestion, but without the size limits and with room for Markdown context and a recommendation per Question.

The app shows each Question Round in a large, focused window: one Question at a time, the Agent's Options as big cards, a Note field that is always available, Back/Next navigation, a step bar to jump around, and a Review Page before I Submit. When I Submit (or Cancel), the Agent receives a result in the same format as AskUserQuestion, so it understands my Answers immediately. Several Agents can wait at the same time; a sidebar lets me switch between their Question Rounds in any order. If a client drops the long-running call, nothing is lost: the Agent can pick up the result later using its Ticket.

## User Stories

### Asking (Agent side)

1. As an Agent, I want to call `ask_user_question` with the same field names as AskUserQuestion, so that I can use it without learning a new schema.
2. As an Agent, I want to ask any number of Questions in one Question Round, so that a grilling round is not split into artificial chunks of four.
3. As an Agent, I want to give a Question zero Options, so that I can ask open questions that only need free text.
4. As an Agent, I want to give a Question more than four Options, so that I can lay out all realistic alternatives.
5. As an Agent, I want to attach a Markdown `body` to a Question, so that I can explain context, trade-offs, and consequences in detail.
6. As an Agent, I want to set a Recommendation per Question, so that the user sees my suggested answer and can accept it quickly.
7. As an Agent, I want to set a Recommendation that is not one of my Option labels, so that I can recommend a nuanced combination.
8. As an Agent, I want to give each Option a `description` and an optional `preview`, so that the user can compare concrete artifacts like code snippets or mockups.
9. As an Agent, I want to mark a Question as `multiSelect`, so that the user can pick several Options.
10. As an Agent, I want to pass an optional `title` for the Question Round, so that the user knows what the round is about.
11. As an Agent, I want to pass an optional `source` (e.g. project name or working directory), so that the user can tell my rounds apart from other Agents'.
12. As an Agent, I want invalid input (duplicate question texts, duplicate Option labels, empty question list, `multiSelect` without Options, missing required fields) to be rejected immediately with a precise error, so that I can correct my call instead of confusing the user.
13. As an Agent, I want the tool description to explain how to use the tool well (bundle related Questions, put context in `body`, always give a Recommendation, choose a unique Ticket and reuse it after a dropped call), so that I produce good grilling rounds without extra prompting.

### Receiving answers (Agent side)

14. As an Agent, I want `ask_user_question` to block until the user Submits or Cancels, so that I can treat it like AskUserQuestion.
15. As an Agent, I want to receive the result as text in the familiar `Your questions have been answered: "Q"="A". You can now continue with these answers in mind.` shape, so that I interpret it like AskUserQuestion.
16. As an Agent, I want Notes to appear next to their Answer in the text, so that I see the user's caveats and reasoning.
17. As an Agent, I want long or multi-line Answers rendered as a per-question Markdown block instead of one line, so that they stay readable and unambiguous.
18. As an Agent, I want the result also as `structuredContent` with `answers`, `annotations` (Notes), `status` and `ticket`, plus an `outputSchema`, so that I can process it reliably.
19. As an Agent, I want Unanswered Questions to be explicitly marked as unanswered, so that I can follow up on them.
20. As an Agent, I want a Cancelled Question Round to return status `cancelled` with the partial Answers, so that I can react sensibly and not lose what the user already answered.
21. As an Agent, I want Heartbeat progress notifications while I wait, so that my client's idle timeout does not abort the call.
22. As an Agent, I want to choose the Ticket of a Question Round myself, and have a repeated `ask_user_question` with the same Ticket reattach to that round, so that I can recover from a dropped call without asking the user twice.
23. As an Agent, I want to call `wait_for_answers(ticket)` and get either the result or "still pending" (with the round's title and question texts) within about 50 seconds, so that I can poll robustly when long blocking calls are not possible and confirm the round is mine.
24. As an Agent, I want an unknown Ticket to produce a clear error, so that I don't loop forever.

### Connecting and security

25. As a user, I want the app to listen only on `127.0.0.1` on a configurable port (default 7457), so that nothing outside my machine can reach it.
26. As a user, I want to configure an optional bearer token in the app's config file, so that only clients I set up can ask me questions.
27. As a user, I want requests without the right token to be rejected with 401, so that misconfigured or foreign clients fail clearly.
28. As a user, I want every request carrying an `Origin` header to be rejected, so that web pages cannot talk to the local port.
29. As a user, I want to connect Agents from inside my nono sandbox over HTTP, so that only the Agent is sandboxed and the GUI is not.
30. As a user, I want the app's settings in a TOML config file in my platform's config directory, so that I can version and edit them.
31. As a user, I want a README that shows how to wire the Question Desk into Claude Code (MCP config with URL and token header, disabling the built-in AskUserQuestion, relevant timeout environment variables), so that setup takes minutes.

### Answering (user side)

32. As a user, I want each Question shown on its own, large and focused, so that I can think about one decision at a time.
33. As a user, I want the header to show the Agent Session, the Question's `header` chip, and progress like "3 / 7", so that I always know where I am.
34. As a user, I want the Question's `body` rendered as Markdown in its own scroll area, so that long context is readable.
35. As a user, I want Options shown as large cards with bold label and description, so that I can compare them at a glance.
36. As a user, I want the Recommended Option badged but not pre-selected, so that I consciously decide instead of clicking through.
37. As a user, I want a Recommendation that matches no Option shown as recommendation text above the Options, so that I still see it.
38. As a user, I want single-select Questions to behave like radio buttons and `multiSelect` Questions like checkboxes, so that the interaction matches the Question.
39. As a user, I want the `preview` of the focused or selected Option shown in a side panel in monospace, so that I can compare artifacts.
40. As a user, I want the preview panel to move below the Options when the window is narrow, so that the layout stays usable.
41. As a user, I want a Note field on every Question that is always visible and grows with its content, so that I can say "yes, but…".
42. As a user, I want my Note to count as the Answer when I select no Option, so that I can answer open questions in free text.
43. As a user, I want Back and Next buttons, so that I can move through the round sequentially.
44. As a user, I want a clickable step bar showing each Question's state (open, answered, current), so that I can jump to any Question.
45. As a user, I want my Answers and Notes preserved when I navigate or switch rounds, so that I never lose input.
46. As a user, I want a Review Page after the last Question listing all Questions with their Answers and Notes, so that I can check everything before Submitting.
47. As a user, I want to click a Question on the Review Page to jump back to it, so that I can fix Answers quickly.
48. As a user, I want Unanswered Questions highlighted on the Review Page, so that I notice gaps.
49. As a user, I want to be allowed to Submit with Unanswered Questions after a warning, so that "no opinion yet" is a valid outcome.
50. As a user, I want Cancel (button or `Esc`) to ask for confirmation, so that I don't lose a round by accident.
51. As a user, I want closing the window with open rounds to ask for confirmation too, so that I don't cancel rounds unintentionally.

### Keyboard

52. As a user, I want `1`–`9` to toggle Options when the Note field is not focused, so that I can answer without the mouse.
53. As a user, I want `Tab` to move focus into the Note field, so that I can type a Note quickly.
54. As a user, I want `Cmd/Ctrl+Enter` to go to the next Question (and Submit on the Review Page), so that I can work fast.
55. As a user, I want `Cmd/Ctrl+Shift+Enter` to go back, so that I can correct an Answer.
56. As a user, I want `Enter` with no selection and an empty Note to accept the Recommendation and move on, so that agreeing is one keystroke.
57. As a user, I want subtle shortcut hints in the footer, so that I discover the shortcuts.

### Multiple Agents

58. As a user, I want a collapsible sidebar listing open Question Rounds grouped by Agent Session, with progress like "3/7 answered", so that I see everything that waits for me.
59. As a user, I want to switch between Question Rounds in any order, not FIFO, so that I can answer what matters first.
60. As a user, I want Agent Sessions labelled from the client's `clientInfo`, refined by `source` and `title`, so that I recognise who is asking; clients that provide nothing show as "Unknown client".
61. As a user, I want new rounds to appear with a badge and timestamp, so that I notice them.
62. As a user, I want a simple indicator when the Agent that asked a round is no longer connected, so that I know my Answers will need to be picked up.
63. As a user, I want rounds whose call was dropped to stay answerable, so that a client timeout never throws away my work.

### Window, look and feel

64. As a user, I want the window to come to the front and request attention (dock bounce / taskbar flash) on a new round without stealing keyboard focus, so that I don't type into it by accident.
65. As a user, I want an empty state "No open questions" when nothing is pending, so that I know the app is alive.
66. As a user, I want the app to follow the system's dark/light theme, so that it fits my desktop.
67. As a user, I want a larger base font and `Cmd +/-` zoom that is remembered with the window size, so that long text is comfortable to read.
68. As a user, I want to enter emoji via the native OS input (e.g. the macOS emoji picker) and see them rendered in color, so that I can express myself naturally.
69. As a user, I want umlauts and IME input to work in all text fields, so that I can answer in German.
70. As a user, I want the app to run on macOS first and on Linux and Windows later without a rewrite, so that I can use it on all my machines.

### Developer experience

71. As a developer, I want an interactive demo client that connects to my running Question Desk and sends realistic sample Question Rounds (grilling-sized, multiSelect, previews, emoji, open questions), so that I can see and answer them in the real GUI without mocking the protocol.
72. As a developer, I want the demo client to print the result exactly as the Agent receives it, so that I can verify the output format by eye.
73. As a developer, I want the demo client to be able to send several rounds concurrently under different `source` values, so that I can try the sidebar and switching.

## Implementation Decisions

Architecture and technology are recorded in ADRs; this section does not repeat their rationale.

- **ADR 0001**: long-running GUI app outside the sandbox, embedded stateless Streamable HTTP MCP server on `127.0.0.1`, optional bearer token, reject any `Origin`.
- **ADR 0002**: `rmcp` mounted in `axum`; no hand-rolled MCP.
- **ADR 0003** (revised after spike 2): blocking `ask_user_question` with Heartbeat (~15 s), Agent-chosen Ticket per round, reattach on repeated calls, `wait_for_answers(ticket)` (~50 s) for Pickup.
- **ADR 0004**: input schema is a superset of AskUserQuestion; output keeps its shape; strict validation; Markdown output generated with `pulldown-cmark-to-cmark`.
- **ADR 0005**: `eframe`/`egui` 0.37 with `egui_commonmark` and `egui_system_font_provider`; color emoji is a hard gate verified by a spike first.

### Modules

One binary crate, with a library target so tests and the demo client can use it.

- **schema** — serde types for the tool input and output (`JsonSchema` derived for MCP schemas) and strict validation. JSON is parsed only via `serde_json`/rmcp, never by hand.
- **desk** (the core) — owns all Question Rounds and Agent Sessions. Deep module with a small interface:
  - Agent side: open or reattach to a Question Round by Ticket (returns a future resolving to the result; error if the Ticket is known with different Questions), await a result by Ticket with a deadline, mark a round's Agent as disconnected.
  - User side (the same intents the UI triggers): list rounds grouped by Agent Session with progress; select a round; go to Question n / next / back / Review Page; toggle Option; set Note; accept Recommendation; Submit; Cancel.
  - Read model for the UI: current round, current Question, per-Question state (open / answered / current), Unanswered list, connection indicator.
  - Thread-safe; notifies the UI to repaint on changes. No egui and no HTTP types in its interface.
- **format** — turns a finished round into the text content (short one-line shape vs. Markdown block shape, cancelled shape with partial Answers and Unanswered list) and the `structuredContent`.
- **server** — rmcp tool handlers (`ask_user_question`, `wait_for_answers`), Heartbeat, cancellation-to-disconnect mapping, bearer middleware, Origin rejection, listening on the configured port. Started on a tokio runtime in a background thread.
- **config** — loads the TOML config (`port`, optional `bearer_token`) from the platform config directory via `serde` + `toml` + `directories`; defaults when absent.
- **ui** — egui rendering of the `desk` read model and dispatch of user intents; theme, fonts, emoji, keyboard shortcuts, attention request.
- **demo client** — a separate executable target using rmcp's client to send sample rounds to a running Question Desk and print results.

### Tool contract (summary)

- `ask_user_question` input: required `ticket` (Agent-chosen; short, `[A-Za-z0-9._-]`), `questions[]` (each: `question`, `header`, optional `body`, `options[]` with `label`, `description`, optional `preview`; `multiSelect`; optional `recommended`), optional `title`, optional `source`.
- Output `structuredContent`: `status` (`answered` | `cancelled`), `ticket`, `answers` (question text → answer string; multiple selections comma-joined like the original), `annotations` (question text → `{ notes }`), `unanswered` (list of question texts).
- `wait_for_answers` input: `ticket`. Output: the same result, or `status: pending` with `title` and `questions` (question texts).
- A repeated `ask_user_question` with a known Ticket and identical Questions reattaches (blocks, or returns the finished result); with different Questions it is a tool error.
- Validation failures and unknown Tickets: tool error (`isError: true`) with a precise message.
- Agent Session label: `clientInfo.title`, else `clientInfo.name` (from the request `_meta`, when available), + `source`; fallback "Unknown client". Claude Code sends `{"name": "claude-code", "title": "Claude Code", ...}`.

### Behaviour details

- Recommendation is never pre-selected. If it equals an Option label, that Option is badged; otherwise it is shown as text.
- An Answer is the selected Option label(s); if none selected and the Note is non-empty, the Note text is the Answer; otherwise the Question is Unanswered.
- Submit with Unanswered Questions shows a warning first. Cancel and window close with open rounds ask for confirmation.
- A dropped call leaves the round visible and answerable; rounds and results are kept in memory until the app quits.
- On a new round: bring window to front and request user attention; do not take keyboard focus.

### Delivery order

1. Spike: color emoji, emoji picker input, umlauts/IME in egui 0.37 on macOS (gate for ADR 0005).
2. Spike: multi-minute blocking call with Heartbeat in Claude Code over HTTP; check idle timeout, auto-backgrounding after 2 min, and whether denying `AskUserQuestion` disables the built-in tool; simple disconnect indicator. Done (2026-11-13): see `spikes/02-mcp-heartbeat/README.md` and the revised ADR 0003.
3. MVP: schema → format → desk → server → config → ui → demo client → README.

## Testing Decisions

- **Good tests** exercise external behaviour only: what an Agent receives over MCP and what the user can see and do in the `desk` read model. They don't assert internal data structures, so modules can be reshaped freely.
- **Primary seam — the headless Question Desk**: a test starts the real server on an ephemeral port together with a `desk`, without any window. It drives:
  - the Agent side through a real rmcp client over HTTP (`ask_user_question`, `wait_for_answers`), and
  - the user side through the `desk` user intents (select round, toggle Option, set Note, navigate, jump, Submit, Cancel).

  Through this seam we cover: the full answer flow; reattach with a repeated call; Ticket reuse with different Questions; multiSelect; Note-as-Answer; Recommendation acceptance; Unanswered on Submit; Cancel with partial Answers; validation errors as tool errors; unknown Ticket; Pickup after the blocking call is dropped; Heartbeat notifications being sent; disconnect indicator; concurrent rounds from different Agent Sessions and switching between them; bearer token (401) and Origin rejection (403).
- **Secondary seam — `format` snapshot tests** (`insta`): text output edge cases that would be clumsy through the primary seam: long and multi-line Answers, Markdown escaping of hostile text, emoji, cancelled rounds, Unanswered lists.
- **Interactive acceptance via the demo client**: run the Question Desk, then the demo client; it sends realistic rounds that appear in the real GUI, I answer them, and the client prints what the Agent receives. This is how the GUI itself is tested in the MVP; no protocol mocking needed.
- **Not automated in the MVP**: egui rendering, layout, emoji, IME, focus behaviour — covered by the spike and the demo client. `egui_kittest` UI tests come after the MVP.
- **Prior art**: none yet; this is a greenfield repository. The `rmcp` prototype (stateless server + curl) showed the primary seam is feasible.
- Built test-first for `schema` validation, `format`, and `desk` behaviour, through the seams above.

## Out of Scope

- Persisting Question Rounds and results across app restarts (required after the MVP; until then they are lost when the app quits).
- History of past Question Rounds and export of Answers (planned after MVP).
- Tray / menu bar icon, hiding the window when idle, login item / autostart.
- CORS and any browser-based client.
- stdio transport.
- A `required` flag per Question.
- Multi-window (one window per round) presentation.
- Automated UI tests with `egui_kittest`.
- Custom fonts beyond what is needed for emoji and German text.
- OAuth or any auth beyond a static bearer token.

## Further Notes

- The exact AskUserQuestion result text is not officially documented; the one-line shape comes from an observed transcript. Our format is defined by this spec and pinned by snapshot tests.
- Claude Code facts that shaped the design (docs): `MCP_TOOL_TIMEOUT` default ~28 h; `CLAUDE_CODE_MCP_TOOL_IDLE_TIMEOUT` default 5 min for HTTP; `CLAUDE_CODE_MCP_AUTO_BACKGROUND_MS` default 2 min. Spike 2 (Claude Code 2.1.311) confirmed: progress notifications are required (without them the call timed out after ~3 min; SSE keep-alive does not count), calls move to the background after 2 min and still deliver, and `permissions.deny: ["AskUserQuestion"]` or `--disallowedTools AskUserQuestion` disables the built-in tool (without it Claude Code prefers the built-in tool).
- In the nono sandbox, `GIT_CONFIG_GLOBAL` from the profile is not present in the Agent's shell; git commands need it set explicitly.
- Toolchain: Rust 1.99 available; `eframe`/`egui_commonmark` need MSRV 1.95, `rmcp` 1.88.

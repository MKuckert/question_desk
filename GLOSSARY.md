# Glossary

**Question Desk** — the desktop app as a whole: a long-running GUI that hosts an MCP server and shows Question Rounds to the user.
_Avoid_: dialog, popup, plugin.

**Agent** — an MCP client (e.g. Claude Code, Codex CLI) that asks the user questions through the Question Desk.

**Agent Session** — the identity under which Question Rounds are grouped in the sidebar. Derived from the MCP `clientInfo` of the call, refined by the optional `source` field the Agent provides.
_Avoid_: connection, MCP session (the server is stateless; there are no MCP sessions).

**Question Round** — one `ask_user_question` call: an ordered list of Questions, an optional `title`, and a Ticket. It is answered and submitted (or cancelled) as a unit.
_Avoid_: form, batch, request.

**Question** — one item of a Question Round: question text (unique within the round, used as the answer key), `header`, optional Markdown `body`, zero or more Options, `multiSelect`, optional Recommendation.

**Option** — a predefined choice of a Question: `label` (unique within the question), `description`, optional `preview`.

**Recommendation** — the Agent's suggested answer for a Question. If it matches an Option label, that Option is badged "Recommended"; otherwise it is shown as recommendation text. Never pre-selected.

**Answer** — what the user gave for one Question: the selected Option label(s), or the free text if no Option is selected. A Question may be left **Unanswered**.

**Note** — free text the user attaches to a Question in addition to (or instead of) selecting Options. Always available.

**Review Page** — the last step of a Question Round: lists every Question with its Answer and Note, highlights Unanswered ones, and holds the Submit action.

**Submit** — the user finishes a Question Round; the Answers are delivered to the Agent with status `answered`.

**Cancel** — the user abandons a Question Round (after confirmation); the partial Answers are delivered with status `cancelled`.

**Ticket** — the identifier of a Question Round, returned to the Agent so it can Pick Up the result later.

**Pickup** — retrieving a Question Round's result via `wait_for_answers(ticket)` after the original blocking call was lost (timeout, disconnect).

**Heartbeat** — the MCP progress notification the server sends periodically while a blocking call waits for the user, to keep client idle timeouts from firing.

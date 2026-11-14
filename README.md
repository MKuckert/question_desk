# Question Desk

A desktop app that replaces Claude Code's built-in `AskUserQuestion` with a large, focused window. Agents call `ask_user_question` over MCP with the same fields they already know, without the limits: any number of Questions, any number of Options (or none), a Markdown `body` and a Recommendation per Question. You answer one Question at a time, review, and Submit; the Agent gets the familiar `Your questions have been answered: "Q"="A". …` result.

You start the app yourself, outside any sandbox. It hosts a Streamable HTTP MCP server on `127.0.0.1`, so sandboxed Agents only need localhost network access. Several Agents can wait at the same time; the sidebar lets you answer their rounds in any order.

Terms used here are defined in [GLOSSARY.md](GLOSSARY.md); design decisions are in [docs/adr/](docs/adr/).

## Run

```sh
cargo run --release
```

The window shows the MCP URL while no questions are open (default `http://127.0.0.1:7457/mcp`). The app needs a GPU, so start it from a normal terminal, not from inside the nono sandbox.

## Configure

Optional settings live in `config.toml` in the platform's config directory:

- macOS: `~/Library/Application Support/question-desk/config.toml`
- Linux: `~/.config/question-desk/config.toml`
- Windows: `%APPDATA%\question-desk\config\config.toml`

```toml
port = 7457                      # listens on 127.0.0.1 only
bearer_token = "a-long-random-string"   # optional; clients must send it
```

With `bearer_token` set, requests without `Authorization: Bearer <token>` get `401`. Requests carrying an `Origin` header always get `403`, so web pages cannot reach the port.

## Connect Claude Code

1. Add the server (here for your user; use `--scope project` to share it via `.mcp.json`):

   ```sh
   claude mcp add --transport http --scope user question-desk http://127.0.0.1:7457/mcp \
     --header "Authorization: Bearer a-long-random-string"
   ```

   Or in `.mcp.json`, reading the token from the environment:

   ```json
   {
     "mcpServers": {
       "question-desk": {
         "type": "http",
         "url": "http://127.0.0.1:7457/mcp",
         "headers": { "Authorization": "Bearer ${QUESTION_DESK_TOKEN}" }
       }
     }
   }
   ```

2. Disable the built-in tool. Otherwise Claude Code keeps using `AskUserQuestion`. In `~/.claude/settings.json` or `.claude/settings.json`:

   ```json
   { "permissions": { "deny": ["AskUserQuestion"] } }
   ```

   `claude --disallowedTools AskUserQuestion` works too.

3. Timeouts. The server sends a progress notification (Heartbeat) every 15 s while you answer, which keeps the call alive. The relevant Claude Code environment variables:

   | Variable | Default | Effect |
   |---|---|---|
   | `MCP_TOOL_TIMEOUT` | ~28 h | Hard limit for one tool call. |
   | `CLAUDE_CODE_MCP_TOOL_IDLE_TIMEOUT` | 5 min (HTTP) | Aborts a call without activity; the Heartbeat counts as activity. |
   | `CLAUDE_CODE_MCP_AUTO_BACKGROUND_MS` | 2 min | Moves a long call to a background task; the answer still arrives. |

   The defaults work. If a call is dropped anyway (Esc, client timeout, a new session), your answers are not lost: the round stays in the window, and the Agent picks them up with `wait_for_answers(ticket)` or by repeating the same call.

### From inside the nono sandbox

Run only the Agent in the sandbox. Its profile needs network access to `127.0.0.1:7457`; the Question Desk runs outside.

## The tools

**`ask_user_question`** takes the AskUserQuestion fields plus a few more:

| Field | Required | Meaning |
|---|---|---|
| `ticket` | yes | Agent-chosen id, `[A-Za-z0-9._-]`, e.g. `db-choice-7f3`. Reuse it to pick up the answers after a dropped call. |
| `questions[]` | yes | `question` (unique), `header`, optional `body` (Markdown), `options[]` (`label`, `description`, optional `preview`), `multiSelect`, optional `recommended`. |
| `title` | no | What the round is about. |
| `source` | no | Project or working directory, to tell Agents apart. |

It blocks until you Submit or Cancel. The text result uses one line for short Answers and Markdown sections once an Answer or Note is long or multi-line. `structuredContent` has `status` (`answered` / `cancelled`), `ticket`, `answers`, `annotations` (your Notes) and `unanswered`.

**`wait_for_answers(ticket)`** blocks up to ~50 s and returns the result, or `status: pending` with the round's `title` and question texts.

A repeated `ask_user_question` with the same Ticket and the same Questions reattaches to the round; with different Questions it is an error.

## Answering

- Options are cards: click or press `1`–`9`. Single-select behaves like radio buttons, `multiSelect` like checkboxes.
- The Recommended Option is badged, never pre-selected. A Recommendation that matches no Option is shown above the Options.
- The Note field is always there. Without a selected Option, the Note is your Answer.
- `Tab` focuses the Note and `Esc` leaves it. `Enter` outside the Note, with nothing selected or noted yet, accepts the Recommendation and moves on. `⌘↩` / `Ctrl+Enter` goes to the next Question (Submit on the Review Page), `⌘⇧↩` goes back, and `Esc` outside the Note cancels the round after confirmation.
- The step bar and the Review Page jump to any Question. Submitting with Unanswered Questions asks first.
- `⌘+` / `⌘-` zoom; zoom and window size are remembered.
- Emoji: Edit → Emoji & Symbols or `Ctrl+⌘+Space` on macOS.

## Try it with the demo client

With the Question Desk running:

```sh
cargo run --bin demo-client                 # menu of sample rounds
cargo run --bin demo-client -- all          # every round at once, from different sources
cargo run --bin demo-client -- grilling emoji
cargo run --bin demo-client -- --pickup grilling-1a2b   # poll wait_for_answers
```

Pass `--token` (or set `QUESTION_DESK_TOKEN`) if you configured one, and `--url` for another port. The client prints each result exactly as the Agent receives it.

## Develop

```sh
cargo test     # headless desk over real MCP/HTTP, plus format snapshots
cargo clippy --all-targets
```

`spikes/` holds the throwaway spikes behind ADR 0003 and 0005; they are separate crates.

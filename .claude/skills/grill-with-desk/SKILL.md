---
name: grill-with-desk
description: Grill the user via the Question Desk, relentlessly stress-testing a plan, decision, or idea one Question Round at a time. Use when the user wants to grill or be grilled with, in, or through the Question Desk ('desk'), or wants a grilling answered in the desk window rather than in chat.
---

Interview the user relentlessly until you reach a shared understanding. Map this as a **design tree**: every decision branches into the decisions that hang off it.

Work the tree in **rounds**. The **frontier** is every decision whose prerequisites are already settled: the questions you can ask _now_ without guessing at answers you haven't heard yet. Ask the whole frontier as one Question Round: a single `ask_user_question` call through the question desk MCP. The call blocks until the user submits; their answers arrive as its result.

## Asking a round

Each frontier decision becomes one Question:

- `header`: a short chip label for the decision, e.g. `Storage`.
- `question`: one sentence naming the decision.
- `body`: the context, the trade-offs, and what each choice implies downstream, in Markdown.
- `options`: the concrete choices, each with a `description`; add a `preview` when the user should compare code or mockups. Use `multiSelect` when several can apply. Leave `options` empty for an open question answered in free text.
- `recommended`: your recommended answer, always. Use an Option label when one fits, free text when your recommendation is nuanced. The user accepts it with a single Enter.

Round-level fields:

- `title`: `Grilling: <topic> — round <n>`.
- `source`: the project name.
- `ticket`: `grill-<topic-slug>-<4 random hex>-r<n>`, fresh for every round.

The round lives in the Question Desk window; in chat, say in one line that round _n_ is waiting there and what it covers.

## Reading the answers

Each answer is a selected Option label, or free text. **Notes** (`annotations`) carry the user's reasoning and objections: weigh them as heavily as the answer, since a Note can reopen a settled branch or add a new one. **Unanswered** questions stay on the frontier for the next round.

If the call is dropped (timeout, interrupt), pick the round up with `wait_for_answers` and the same ticket, repeating while it reports `pending`. The user's answers live in the desk under that ticket.

If the round comes back `cancelled`, keep any partial answers as settled, pause the grilling, and ask in chat how the user wants to continue.

## Advancing the tree

Each round the user answers reshapes the tree: settled decisions push the frontier outward and unblock questions that depended on them. Recompute the frontier and ask the next round. A question whose answer depends on another question still open in this round belongs to a _later_ round, not this one.

Finding _facts_ is your job, never the user's. When a frontier question needs a fact from the environment (filesystem, tools, etc.), dispatch a sub-agent to find it; don't ask the user for anything you could look up yourself. Don't block on it: a running exploration is an unsettled prerequisite, so only the questions downstream of it wait for the sub-agent to report; ask the rest of the frontier now. The _decisions_ are the user's: put each to them and wait.

## Finishing

The session is done when the frontier is empty: every branch of the design tree visited, nothing left silently assumed. Close with a final round holding one Question: its `body` summarises every settled decision, and it asks whether this is the shared understanding, recommending yes. Act on the plan only once the user confirms; a "no" or a Note reopens the branches it names.

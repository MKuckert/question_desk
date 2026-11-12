# 4. Input schema is a superset of AskUserQuestion; output follows its format

Status: accepted (2026-11-12)

## Context

Agents already know the built-in AskUserQuestion tool (1–4 questions, 2–4 options, `header`, `multiSelect`, option `label`/`description`/`preview`; answers keyed by question text). Its limits are too tight for grilling. Its result text is not officially documented; the observed shape is `Your questions have been answered: "Q"="A". You can now continue with these answers in mind.`

## Decision

Keep every AskUserQuestion field name and meaning, lift the count limits (any number of Questions, 0..n Options), and add optional `body` (Markdown), `recommended` (per Question), and `title` / `source` (per call). Validate strictly; invalid input is rejected as a tool error with a precise, correctable message (duplicate question texts or labels, empty question list, `multiSelect` without Options, …).

Output: `structuredContent` with `answers` and `annotations` (notes) keyed by question text like the original, plus `status` (`answered` | `cancelled`) and `ticket`. The text content uses the original one-line shape for short answers and switches to a per-question Markdown block format when an answer is multi-line or long. Markdown is generated with a library (`pulldown-cmark-to-cmark`), never by string concatenation.

## Consequences

- Agents that know AskUserQuestion use the tool correctly without extra instruction.
- Question text must be unique within a Question Round.
- The text format is ours to define; snapshot tests pin it.

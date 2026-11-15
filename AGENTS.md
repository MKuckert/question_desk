## Agent skills

### Issue tracker

Issues live as local markdown files under `.scratch/<feature>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Uses the five default triage labels (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`), recorded as a `Status:` line in each issue file. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

## Seeing the UI

To check a UI change visually, run `tools/screenshot.sh [-o OUT.png] [demo rounds…]` and read the PNG it prints (`--help` for details). It captures the Question Desk window only, which keeps the user's other windows private; take every screenshot through it rather than `screencapture` directly.

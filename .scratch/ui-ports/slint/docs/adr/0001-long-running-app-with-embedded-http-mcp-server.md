# 1. Long-running GUI app with an embedded, stateless Streamable HTTP MCP server

Status: accepted (2026-11-12)

## Context

Agents run inside a nono sandbox. A stdio MCP server spawned by the Agent inherits that sandbox, and opening GUI windows from it is unreliable. Several Agents may ask questions concurrently, and the user wants to switch between their Question Rounds freely. The latest MCP protocol version (2026-07-28) removes protocol-level sessions.

## Decision

The Question Desk is a long-running desktop app started by the user, outside the sandbox. It embeds an MCP server using the Streamable HTTP transport in stateless mode, bound to `127.0.0.1` on a configurable port. Agents connect over HTTP. No stdio transport.

Security: an optional static bearer token (configured in the app's config file, sent by the client as a header); every request carrying an `Origin` header is rejected; the MCP library's Host validation stays on. CORS is out of scope until a browser client is needed.

## Consequences

- One process owns all Question Rounds; concurrency and the sidebar come naturally.
- If the app is not running, the Agent's MCP server is simply unavailable.
- Stateless mode lets the server notice client disconnects (cancellation fires), but `clientInfo` is only available from clients speaking protocol 2026-07-28 or later, which send it (optionally) in each request's `_meta`; read it with rmcp's `RequestContext::client_info()`, never `peer_info()` (a placeholder in stateless mode). Older clients show as "Unknown client" unless they pass `source`. Claude Code 2.1.311 speaks 2026-07-28 and sends `clientInfo` (spike 2).
- The sandbox profile needs network access to localhost only.

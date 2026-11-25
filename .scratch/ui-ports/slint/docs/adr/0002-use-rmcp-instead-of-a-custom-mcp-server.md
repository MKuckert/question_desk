# 2. Use rmcp instead of a hand-rolled MCP server

Status: accepted (2026-11-12)

## Context

We do not want to implement JSON-RPC, Streamable HTTP, SSE, progress notifications or output schemas ourselves.

## Decision

Use `rmcp` (the official Rust MCP SDK, 3.9 at time of writing) with its Streamable HTTP server mounted in `axum`. Bearer-token checking is plain axum/tower middleware. Tool results use rmcp's structured output (`structuredContent` plus a generated `outputSchema`).

Verified in a throwaway prototype: long-running tool handlers can send `notifications/progress`; structured output works; bearer middleware returns 401; a bad `Origin` returns 403; in stateless mode the request's cancellation token fires when the client disconnects. Not yet verified: tool calls that block for many minutes.

## Consequences

- The test suite can use rmcp's own client against the real server.
- We follow rmcp's protocol-version support; upgrading rmcp is how we follow MCP spec changes.
- Alternatives (`rust-mcp-sdk`, `pmcp`, `turbomcp`, …) were not evaluated in depth; revisit only if rmcp blocks us.

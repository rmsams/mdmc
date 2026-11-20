---
type: adr
title: Adopt an MCP server as mdmc's integration surface, replacing the opencode plugin
status: accepted
date: 2026-11-20
design: "[[mcp-server]]"
---

# ADR — Adopt an MCP server, replacing the plugin

## Context

mdmc is exposed to opencode through a separate TypeScript plugin
(`email-tools.ts`) that hand-marshals mdmc's CLI into nine tools — five shims
over `mdmc new`/`compile`/`decompile` and four notmuch/neomutt wrappers that
mdmc itself declares out of scope. The plugin and the CLI are two sources of
truth that drift: an mdmc flag or subcommand change must be mirrored by hand in
the plugin. The plugin also imposes a constant context cost, since its tool
schemas are re-sent to the model on every request.

## Decision

Ship an MCP server inside mdmc (`mdmc mcp`, stdio transport) that exposes
mdmc's own surface — compose, reply, forward, write, decompile, view, account
enumeration, plus message search and address lookup — and delete the plugin
entirely once it ships (`plugin-relationship: replace-all`).

## Rejected alternatives

- `none` — keep the plugin as the only integration: retains the drift and the
  two sources of truth.
- `coexist` — ship the MCP alongside the full plugin: doubles the schema tax
  for the same operations.

## Consequences

- Tool schemas and the implementation live in one codebase; interface changes
  propagate with no plugin edit.
- The plugin is deleted after the MCP ships — an integration step sequenced
  last, not part of building mdmc.
- Any MCP client (opencode or otherwise) can drive mdmc.
- New costs: a JSON-RPC server loop to maintain, and a return-instead-of-print
  refactor (see the library-invocation ADR).

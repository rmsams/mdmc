---
type: adr
title: Derive tool input schemas from shared structs rather than hand-writing them
status: accepted
date: 2026-11-20
design: "[[mcp-server]]"
---

# ADR — Derive tool input schemas

## Context

MCP tools declare their inputs as JSON Schema. If those schemas are written by
hand in the server — as the plugin's TypeScript schemas are today — a change to
mdmc's interface must be mirrored by hand. That is the same drift the MCP is
meant to remove, relocated from TypeScript to Rust.

## Decision

Define each tool's arguments as a serde struct that derives both `clap::Args`
(for the CLI) and `schemars::JsonSchema` (for the schema), and emit the schema
at runtime in `tools/list` via `schema_for!`. One struct is the single source
of truth, so an interface change propagates to both the CLI flag and the MCP
schema with no manual edit.

## Rejected alternatives

- `hand-written` — maintain the JSON schemas by hand in the MCP module: only
  relocates the drift; the development-cost goal is then half met.

## Consequences

- No hand-maintained schema to keep in lockstep with the CLI.
- clap, serde, and schemars attribute namespaces can collide on fields that need
  more than one; the derives must coexist on shared structs.
- The derived schema describes tool *inputs* only. The shipped
  `mail-draft`/`mail-snapshot` schemas describe the *output* contract and stay
  as they are, exported via `mdmc schema`.

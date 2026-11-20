---
type: adr
title: Call mdmc as a library from the MCP server rather than shelling out to the binary
status: accepted
date: 2026-11-20
design: "[[mcp-server]]"
---

# ADR — Call mdmc as a library

## Context

The MCP server lives inside mdmc, so it must execute mdmc's operations. Shelling
out to the `mdmc` binary would re-serialize each tool's struct into argv flags
by hand, letting clap parse them back — the same struct-to-flags mapping the
plugin maintains, reintroducing the drift that `derive` removes. `main.rs`
already dispatches `run(command, cfg)` to typed module functions, so calling
directly is nearly free at that level.

## Decision

Invoke mdmc as a library: the MCP tools call the `run(command, cfg)` dispatch
(or the module functions) directly, with no subprocess. This requires a
return-instead-of-print refactor, because the MCP server's stdout is the
JSON-RPC channel and any stray `println!` would corrupt it.

## Rejected alternatives

- `self-exec` — spawn the `mdmc` binary and parse its stdout: hand-marshals
  struct to argv, defeating `derive`; must locate the binary to spawn itself;
  errors are strings; config is re-discovered in the child.

## Consequences

- No argument drift, typed errors, and no per-call subprocess.
- `run()` and the module functions return their output as values, and both
  `main.rs` and the MCP server become thin front-ends over that core — a
  refactor spanning roughly twenty print sites across eight modules.

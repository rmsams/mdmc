---
type: design-doc
title: Ship an MCP server
status: converged
date: 2026-11-20
dimensions:
  - slug: transport
    status: decided
    choice: stdio
  - slug: tool-surface
    status: decided
    choice: native+message-search+address-lookup
  - slug: plugin-relationship
    status: decided
    choice: replace-all
    depends_on: [tool-surface]
  - slug: schema-provenance
    status: decided
    choice: derive
  - slug: invocation
    status: decided
    choice: library
  - slug: activation
    status: decided
    choice: toggle
  - slug: instructions-home
    status: decided
    choice: embedded+config-hook
---

# Ship an MCP server

## Context

mdmc is a Rust CLI that compiles markdown drafts to RFC 5322 email and never
sends. It is exposed to opencode today through a separate plugin
(`~/.config/opencode/plugins/email-tools.ts`) that wraps mdmc with nine tools:
five are shims over `mdmc new`/`compile`/`decompile` (`compose-draft`,
`compose-draft-reply`, `compose-draft-forward`, `write`, `decompile`); the other
four are notmuch/neomutt wrappers (`find-message`, `resolve-contact`,
`read-message`, `list-accounts`) that mdmc itself declares out of scope.

Two motivations drive considering an MCP server in place of (or alongside) the
plugin:

- Development cost. The plugin shims translate mdmc's CLI surface by hand, so a
  change to an mdmc flag or subcommand must be mirrored in the plugin. The shim
  and the CLI are two sources of truth that drift.
- Token/context cost. Tool schemas are re-sent to the model on every request, so
  the email tools tax the context window each turn even when no email is being
  written. This cost is transport-agnostic — scoping, not MCP, is what removes it
  — but MCP makes the off switch first-class (`disabled`, `/mcps`).

## Goals

- Move the mdmc tool surface into the mdmc codebase so the interface and its
  tool schema are one source of truth.
- Preserve the save-only guarantee: no exposed tool sends mail.
- Give a way to keep email tools out of the default agent's context.

## Non-goals

- Exposing neomutt account discovery: the plugin's `list-accounts` parses
  neomutt config; mdmc enumerates its own `[accounts]` instead.
- A generic query-backend plugin or a `mu` backend now — roadmap items; the
  notmuch coupling is isolated behind one module to leave that door open.
- Any send path, remote hosting, or multi-user access.
- Reaching non-opencode MCP clients (a side benefit, not a requirement).

## Dimensions

## transport

How a client connects to the server.

- `stdio` — `mdmc mcp` runs a JSON-RPC loop over stdin/stdout; the client
  spawns it as a subprocess. No hosting, no auth, no network; matches mdmc's
  local maildir model.
- `http` — a long-running Streamable HTTP endpoint. Enables remote clients but
  adds hosting, auth, and a network surface for no local benefit.
- `none` — no MCP; keep the plugin as the only integration.

Rationale: `stdio` — mdmc reads local maildirs and writes local drafts, so
remote access is out of scope; HTTP adds infra with no payoff.

## tool-surface

Which operations the server exposes. The plugin's four "lookup" helpers split
into two that mdmc already does (read → `view`/`decompile`; account enumeration
→ `[accounts]`) and two that are genuinely outside mdmc: message search and
address resolution.

- `native` — mdmc's own operations only (compose, reply, forward, write,
  decompile, view, account enumeration). No message search, no address
  resolution; a reply/forward needs its `message_id` supplied out-of-band.
- `native+message-search` — `native` plus a notmuch search tool
  (`find-message`), closing the reply/forward entry-point gap. Extends the
  existing `notmuch.rs` dependency. Address resolution stays out (`maddr`).
- `native+message-search+address-lookup` — also adds address resolution
  (`resolve-contact`), pulling address lookup across mdmc's line.

Rationale: `native+message-search+address-lookup` — both search and address are
notmuch-able (the plugin already does `notmuch address`), and mdmc already
shells out to notmuch, so both are cheap to own natively. `maddr` stays the
interactive sent-mail picker, not the queryable lookup. The search and address
commands themselves are a separate change — see `[[search-lookup]]`.

## plugin-relationship

What happens to `email-tools.ts` once the MCP exists. With `tool-surface`
covering every plugin tool (the five shims, `read-message` → `view`/`decompile`,
`list-accounts` → `[accounts]`, `find-message` → search, `resolve-contact` →
address), nothing is left for the plugin to do.

- `replace-shims` — delete the five mdmc shims; keep the four notmuch/neomutt
  helpers in the plugin.
- `replace-all` — delete the whole plugin; every tool it exposed now has a
  native mdmc home.
- `coexist` — ship the MCP unchanged alongside the full plugin. Doubles the
  schema tax for the same operations.

Rationale: `replace-all` — the plugin is fully subsumed, so keeping any of it
leaves dead duplication. This is an integration concern to act on after the MCP
is implemented, not part of building mdmc itself.

## schema-provenance

Where the tool input schemas come from — the lever that decides whether the
development-cost motivation is actually met.

- `hand-written` — maintain the JSON tool schemas by hand in the MCP module, as
  the plugin does now. Reintroduces drift; it just moves the update from
  TypeScript to Rust.
- `derive` — define each tool's arguments as a serde struct that derives both
  `clap::Args` (CLI) and `schemars::JsonSchema` (schema); emit the schema at
  runtime in `tools/list` via `schema_for!`. One struct is the single source of
  truth, so an interface change propagates to both the CLI flag and the MCP
  schema with no manual edit.

The `derive` schema describes the tool *inputs* (to/cc/body/message-id …), not
the frontmatter. The shipped `mail-draft`/`mail-snapshot` schemas describe the
*output* contract and stay as they are, exported via `mdmc schema`.

Rationale: `derive` — it is what meets the development-cost goal; `hand-written`
only relocates the drift.

## invocation

How the MCP tools execute mdmc.

- `library` — the tools call mdmc's `run(command, cfg)` dispatch directly, no
  subprocess. Co-located and type-safe; no argument re-serialization.
- `self-exec` — the server shells out to the `mdmc` binary. Decouples the server
  from internals but re-serializes args and re-parses output.

`main.rs` already dispatches `run(command, cfg) -> Result<(), String>` to typed
module functions, so `library` is nearly free at that level. `self-exec` would
hand-marshal each tool's struct back into argv flags — the exact drift surface
`schema-provenance: derive` exists to remove.

The cost of `library` is a return-instead-of-print refactor: results are
currently emitted with `println!` across the modules, but the MCP server's
stdout is the JSON-RPC channel, so any stray print corrupts it. `run()` and the
module functions must return their output as values, and both `main.rs` and the
MCP server become thin front-ends over that core.

Rationale: `library` — it is the only choice consistent with `derive`;
`self-exec` would reintroduce the hand-written struct-to-flags drift the plugin
suffers from, and its "no refactor" is illusory.

## activation

How the server is gated so email tools do not tax the context window in
non-email sessions.

- `always-on` — connect at startup; tools resident every turn.
- `toggle` — connect on demand via `/mcps` or `disabled`, paying the schema tax
  only while email work happens.
- `subagent` — scope the tools to a dedicated mail subagent so they never enter
  the default agent's context, and the email transcript stays out of the main
  window too.

Rationale: `toggle` — it saves the per-turn schema tax with minimal setup and
matches the current plugin UX plus a manual on/off; the per-directory nature of
`/mcps` is irrelevant because mdmc writes to fixed paths. `subagent` adds
transcript isolation but at the cost of a background plus external-viewer
workflow; it can be layered on later if the email transcript polluting the main
window becomes a problem.

## instructions-home

Where the email operational rules live, given that the MCP `instructions`
channel is a single opaque string per server.

- `embedded-only` — the generic tool contract is embedded in the server;
  installation-specific prose stays in the client (global AGENTS.md).
- `embedded+config-hook` — the generic contract is embedded, and the server also
  reads an optional `[mcp] instructions` config entry (a string or a path to a
  markdown file) and appends it, so installation-specific prose loads only while
  the server is connected.
- `client-side` — the server returns no instructions; all rules live in the
  client.

The split is really three-way: the generic contract (save-only, never sends,
review in neomutt) is embedded; installation *data* (accounts, default account,
aliases, editor) lives in `config.toml` and is not prose; only genuinely
personal *policy prose* is the residue this dimension places.

Rationale: `embedded+config-hook` — it reuses mdmc's existing
embedded-default-plus-config-override pattern (the pandoc assets), and it loads
the personal prose only while email work is in scope.

## Interdependencies

`plugin-relationship` depends on `tool-surface`: with search and address lookup
native to mdmc, every plugin tool has a home, so `replace-all` is the natural
outcome. The search and address commands themselves are a separate change — see
`[[search-lookup]]`. `schema-provenance` is the mechanism behind the
development-cost goal; if it is `hand-written`, that goal is only half met.
`activation` is orthogonal to the code but decides whether the token/context
goal is met, and works identically for MCP and plugin tools.
`instructions-home` is moot if `transport` is `none`; with `stdio` it hosts the
generic email contract plus the optional installation-prose hook.

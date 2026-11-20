---
type: design-doc
title: Maildir search and address lookup
status: converged
date: 2026-11-20
dimensions:
  - slug: cli-surface
    status: decided
    choice: sibling-subcommands
  - slug: address-scoping
    status: decided
    choice: scoped-default
  - slug: backend
    status: decided
    choice: notmuch-native
---

# Maildir search and address lookup

## Context

mdmc declares message and address lookup out of scope today: `notmuch.rs` only
resolves a known `mid:` to a file (with a maildir-scan fallback), and the
picking/address tools live outside mdmc (`mfzf` for message picking, `maddr`
for sent-mail recipients). The MCP work (see `[[mcp-server]]`) needs search and
address lookup as first-class mdmc commands, because a reply/forward workflow
must turn "reply to Sarah's Q2 email" into a Message-ID, and compose must turn
a name into an address.

## Goals

- Add mdmc commands for message search and address lookup.
- Keep mdmc generic: no hard-coding of one installation's corpus or index.
- Keep search and lookup read-only — they never send.

## Non-goals

- A generic query-backend plugin or a `mu` backend now — roadmap items; the
  notmuch coupling is isolated behind one module to leave that door open.
- Replacing the interactive pickers (`mfzf`, `maddr`) unless that is chosen.

## Dimensions

## cli-surface

How the commands are exposed.

- `sibling-subcommands` — `mdmc search` and `mdmc address` as top-level
  subcommands, following the existing `list`/`pick` shape.
- `query-modes` — a single `mdmc query` subcommand with `search` and `address`
  modes under it.
- `extend-list` — fold search into the existing `list`/`pick` domain machinery.

The consumption pattern (stdout lines vs `--json`, plus an interactive `pick`
variant) already exists in `list`/`pick`, so this dimension is mostly about
naming and whether the interactive picker is reused or left to `mfzf`/`maddr`.

Rationale: `sibling-subcommands` — flat top-level verbs match mdmc's existing
CLI (no nested subcommands anywhere), and `--json` follows `list --json` for
MCP/scripting consumption. Interactive picking stays with `mfzf`/`maddr`.

## address-scoping

How address lookup avoids the naive full-index search. `maddr` exists because
`notmuch address --output=recipients` reads every message file (recipient
addresses are not cached in the notmuch database), so it takes seconds — and
over the full index it returns addresses you would never email.

- `full-index` — `notmuch address *` over every indexed message (what the
  plugin does today); slow and low-quality.
- `scoped-default` — default the notmuch address query to sent mail, derived
  from `[accounts].from`, overridable via a `[lookup]` config field. Fewer
  files to read (faster) and only people you email (higher quality).
- `curated-cache` — build a side SQLite index of sent-mail recipients, as
  `maddr` does; instant, but re-implements `maddr` inside mdmc.

Rationale: `scoped-default` — the latency that drove `maddr` is a picker
concern (interactive, must be instant), not an agent-lookup concern (occasional,
seconds are fine); scoping to sent mail via `[accounts].from` fixes both quality
and speed without a second index. `maddr` stays the interactive picker.

## backend

How the search and address tools are backed, given mdmc already shells out to
notmuch for `mid:` resolution.

- `notmuch-native` — call notmuch directly for search (`notmuch search`) and
  address (`notmuch address`), as `notmuch.rs` does today for resolution.
  Isolate the calls behind the one module so a second backend is a later swap.
- `configurable-template` — a generic json-contract + command-template plugin,
  so the user supplies the query command and output shape. More general, but
  speculative while notmuch is the only backend.

Rationale: `notmuch-native` — notmuch is the only backend in use and the
coupling already exists; a module boundary preserves the option to add `mu` or
a generic plugin later without building the abstraction now.

## Interdependencies

`address-scoping` is where the `maddr` lesson lands; it is a config decision,
not a backend one, so it is independent of `backend`. This feature is upstream
of `[[mcp-server]]`, whose `tool-surface` decision presupposes these commands
exist.

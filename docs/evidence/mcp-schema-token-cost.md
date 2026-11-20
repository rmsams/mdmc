---
type: evidence
title: MCP tool-schema context cost vs the plugin it replaced
date: 2026-11-20
informs: ["[[ship-mcp-server]]"]
---

# MCP tool-schema context cost vs the plugin it replaced

## Setup

Measured the fixed context overhead of mdmc's MCP server against the deleted
opencode plugin it replaced. Both expose nine tools whose descriptions and
argument schemas are re-sent to the model on every request.

- New MCP: captured the live `tools/list` response by driving `mdmc mcp` over
  stdio (`initialize` → `notifications/initialized` → `tools/list`), then
  measured the tool-schema JSON and the embedded `instructions` string.
- Old plugin: recovered `home-files/.config/opencode/plugins/email-tools.ts`
  from home-manager git history (`git show HEAD:…`) and summed the `description`
  and `args` blocks of its nine `tool({…})` calls.

## Results

| integration | tool schemas | instructions | total bytes | est. tokens (~4 c/tok) |
| --- | ---: | ---: | ---: | ---: |
| old plugin (9 tools) | 4,734 | — | 4,734 | ~1,180 |
| new MCP (9 tools) | 3,568 | 358 | 3,926 | ~980 |

The MCP server's fixed overhead is ~20% smaller than the plugin's.

## Findings

- Like-for-like, replacing the plugin with the MCP server did not increase the
  tool-schema tax; it reduced it by ~800 bytes (~200 tokens).
- The overhead is real (the schemas ride in the context prefix on every
  request) but cheap: DeepSeek caches the stable prefix, and `cache_read`
  (`$2.8e-08`/token) is 10× cheaper than fresh input (`$2.8e-07`/token), so the
  ~1,000 schema tokens cost roughly `$0.000028`/request after the first.
- An apparent post-deployment token increase was baseline confusion: the
  `21,731 → 21,965` delta observed while toggling measured MCP-disabled →
  MCP-enabled, not plugin → MCP. The plugin carried the same ~1,000-token cost
  when it was always-on.
- The `session` table in `opencode-stable.db` has `tokens_input`/`cost` columns
  intended for this comparison, but they are all `0` for recent sessions; the
  per-message `$.tokens.cache.read`/`$.tokens.input` fields in the `message`
  table are the populated source for per-session measurement.

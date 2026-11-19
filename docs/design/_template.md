---
type: design-doc
title: <feature title>
status: draft
date: YYYY-MM-DD
dimensions:
  - slug: <dimension-slug>
    status: open
    # choice: <option-slug>          # set when leaning/decided
    # depends_on: [<other-slug>]     # dimension-level dependencies
---

# <feature title>

## Context

What problem this addresses, and what unit of change it covers.

## Goals

## Non-goals

## Dimensions

One `## <dimension-slug>` section per frontmatter dimension. Each section lists
the options as `- \`<option-slug>\` — description`, and ends with the rationale
once decided. The frontmatter `dimensions` list is the canonical state; the body
sections carry the options and the reasoning only.

## <dimension-slug>

- `<option-a>` — description and tradeoff.
- `<option-b>` — description and tradeoff.

Rationale (when leaning/decided): `<option-b>` — the reason.

## Interdependencies

Cross-dimension constraints that do not fit a single `depends_on` edge.

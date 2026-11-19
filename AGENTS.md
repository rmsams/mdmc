# mdmc

Markdown mail compiler. Compiles markdown draft notes to RFC 5322 email, reusing
the markdown → pandoc → HTML/plain pipeline and MML body construction from the
Python `mail` tool, but reimplemented in Rust as a standalone CLI.

The design lives in the docs (see Documentation below).

## Development environment

Use `devenv` for all commands. The project pins its own toolchain and packages.

```sh
devenv shell
```

## Commands

- `mdmc <args>` — run the CLI via `cargo run`
- `build` — `cargo build --release`
- `tests` — `cargo test`

Direct checks (run inside the devenv shell):

```sh
cargo clippy
cargo fmt --check
nix flake check
tkb --root docs validate
```

## Conventions

- Version is declared in `Cargo.toml` only. `flake.nix` derives `pname` and `version` from it via `builtins.fromTOML`.
- Commit messages follow `<file>: <lowercase description>`.
- The design lives in the docs (see Documentation below); update them alongside code changes.

## Documentation

The `docs/` directory is itself a tkb collection (root `docs/`, types `spec`,
`adr`, `design-doc`, `evidence`, `howto`, `reference`). Three tiers with distinct
lifecycles, plus supporting types and user documentation:

- `docs/spec.md` — the living as-built description ("what mdmc is now"), type `spec`. Updated continuously.
- `docs/design/<slug>.md` — per-change design docs, type `design-doc`. `status: draft | converged`. Draft is the working exploration; Converged means every dimension is `decided`. One doc covers one unit of change; its body `## <slug>` sections correspond 1:1 with the frontmatter `dimensions[].slug`.
- `docs/adr/<slug>.md` — immutable decision records, type `adr`. `status: draft | proposed | accepted | superseded | rejected`. Accepted ADRs are never edited, only superseded; rejected ADRs are kept. An ADR extracted from a design doc links back via `design: "[[<slug>]]"`.
- `docs/evidence/<slug>.md` — frozen empirical results (benchmarks, evals), type `evidence`. Dated and immutable; `informs: ["[[...]]"]` links the decision it supports.
- `docs/reference/<slug>.md` — reference material (external standards and how mdmc handles them), type `reference`. Living; updated with the standard it describes.
- `docs/howto/<slug>.md` — user/operator documentation, type `howto`. Living; updated with the procedure it describes.

Rules:

- Before proposing a design, read `docs/design/*` and `docs/adr/*`; stop and ask on conflict.
- `proposed` (ADR) and `draft` (design) mean do not implement.
- A design-doc dimension becomes an ADR only when the decision is hard to reverse, surprising, or a real tradeoff — not every dimension.
- Dimension state lives in the design doc's frontmatter (`dimensions: [{slug, status, choice, depends_on}]`, `status: open | exploring | leaning | decided`); the body carries options (`- \`<option-slug>\` — description`) and rationale. Dimension and option slugs are canonical referents — quote them verbatim.
- Start new docs from the directory's `_template.md`.
- When a change ships, reflect it in `docs/spec.md` (the living as-built); the design doc goes dormant and is superseded by a new design doc on redesign, not edited.
- Validate with `tkb --root docs validate`.

## Transient memory

`memory/` (gitignored) holds notes that persist context between sessions but
are not durable knowledge. Each note starts with frontmatter:

```text
status: active      # active | done | abandoned
expires: YYYY-MM-DD # optional
```

Removal rule: delete a note when `status` is `done` or `abandoned`, or when
`expires` is in the past. Mark `status: done` once the task it tracks completes.
The frontmatter is the relevance signal — never read the body to decide.

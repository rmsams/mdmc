# mdmc — design

`mdmc` (markdown mail compiler) is a Rust reimplementation of the Python `mail`
tool. It compiles markdown draft notes to RFC 5322 email and never sends.

## Commands

`mdmc new` creates a draft note (frontmatter + markdown body) under
`paths.drafts`, assigns its `message-id`, validates its frontmatter against the
shipped schema, and optionally opens `$EDITOR`. `mdmc write` compiles a draft to
RFC 5322 and delivers it. Neither ever sends.

## Message-id semantics

A draft's Message-ID is a random UUID v4 plus the local hostname, assigned once
at `new` time and persisted in the note's frontmatter. `write` reuses the
persisted id (falling back to generating one only for drafts that lack it), so
every delivery mode — `--stdout`, `--folder`, and the drafts maildir — emits the
same Message-ID. Maildir delivery overwrites any existing message carrying the
same Message-ID (scanning `new/` and `cur/` headers), so re-running `write` is
idempotent and never leaves stale duplicate drafts.

## Compile pipeline

`write <draft>`:

1. Load `config.toml` (paths, folders, accounts, defaults).
2. Load the draft note's `---` frontmatter (YAML) + body.
3. Resolve the account's `from`/`realname` from `[accounts.<name>]`.
4. Reuse the note's `message-id` (or generate one for legacy drafts).
5. Optionally append a reply quote (`notmuch show` → `id:` lookup).
6. Build the MML body: markdown → pandoc (lua filters, template, CSS) → html/plain,
   with `![[…]]` image embeds becoming inline `cid:` parts.
7. Compile the MML source (headers + body) to MIME via the `mime-meta-language`
   crate (`compiler` feature only, no `cli`).
8. Post-compile: inject `In-Reply-To`/`References` and inline `Content-ID`s,
   restore CRLF.
9. Deliver: `--stdout` → stdout, `--folder NAME` → `[folders]` maildir, otherwise
   the account's `drafts` maildir (then write `status: pending` back to the note).

## Config schema

```toml
[paths]
drafts = "~/kb/ops/mail/drafts"      # draft note directory
attachments = "~/kb/attachments"
pandoc_data = "~/.local/share/pandoc"

[accounts.rmsams-gmail]
from = "rmsams@gmail.com"
realname = "Robert Sams"
drafts = "~/Mail/rmsams-gmail.com/[Google Mail].Drafts"  # delivery maildir

[folders]
refs = "~/Mail/rmsams-gmail.com/Reference"

[defaults]
css = "email"
template = "email.html"
lua-filters = ["email"]
```

`mdmc` reads `--config PATH` (default `~/.config/mail/config.toml`). It does not
read neomutt config. Bare delivery paths are maildirs; a `maildir:`/`imaps:` URI
scheme is deferred until a second backend lands.

## Schema

The `mail-draft` frontmatter JSON Schema (draft 2020-12) is shipped at
`schema/mail-draft.schema.json` and embedded in the binary via `include_str!`.
`new` validates the frontmatter it builds against it (using the `jsonschema`
crate) before writing, and `write` validates the frontmatter of the note it
loads before compiling. There is no dependency on `~/kb` or the external
`kb-validate` tool. The kb-specific `paths` allow-list is not enforced — draft
location is controlled by `paths.drafts` in config.

## Module map

- `config.rs` — config.toml loading, `~` expansion, `[accounts]`/`[folders]` resolution.
- `frontmatter.rs` — `---`-delimited YAML → `Draft`; schema validation; ordered YAML rendering; `set_fields` in-place rewrite.
- `new.rs` — the `new` subcommand (draft creation).
- `mml.rs` — MML body construction + `compile_mml` (the mml crate).
- `quote.rs` — notmuch reply quoting.
- `util.rs` — hostname and Message-ID generation.
- `write.rs` — the `write` subcommand (compile + maildir delivery).

## MML crate

Dependency `mime-meta-language` links as lib name `mml`. API:

```rust
mml::compiler::message::MmlCompilerBuilder::new().build(&source)?.compile()?.into_vec()?
```

`compile` is synchronous and takes the full message (headers + blank line + MML
body). The compiler adds `MIME-Version: 1.0` and a `Date` header, and re-adds
angle brackets to `Message-ID`/`In-Reply-To`/`References`.

## Deferred

- The `msmtp-sent` equivalent (IMAP-append to Sent).
- Non-maildir delivery backends (URI scheme).
- Account signature support (neomutt `signature`; currently always empty).

## Open questions (from `~/kb/ops/notes/mail-generalization-rust-rewrite.md`)

- Keep the MML intermediate (current choice) or assemble MIME with `mail-builder`/`melib` directly.
- Consolidate account config across `mail.py` and `msmtp-sent.py` (mdmc now has its own `[accounts]`, which temporarily duplicates neomutt).
- License choice before open-sourcing.

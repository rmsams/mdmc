# mdmc — design

`mdmc` (markdown mail compiler) is a Rust reimplementation of the Python `mail`
tool. It compiles markdown draft notes to RFC 5322 email and never sends.

## Commands

`mdmc new` creates a draft note (frontmatter + markdown body) under
`paths.source`, assigns its `message-id`, validates its frontmatter against the
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
source = "~/kb/ops/mail/drafts"      # markdown draft-note directory
attachments = "~/kb/attachments"     # bare-filename base for embeds and attachments
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

mdmc resolves its config in this order:

1. `--config PATH` (explicit override).
2. A `.mdmc.toml` found in the current directory or any parent (per-repo config).
3. `~/.config/mail/config.toml`.

A local `.mdmc.toml` is merged over the global config: local fields take
precedence, and the `accounts`/`folders` maps are a union with local precedence.
Relative paths in a config file resolve against that file's directory, so a repo
can use `source = "drafts"`. mdmc does not read neomutt config. Bare delivery
paths are maildirs; a `maildir:`/`imaps:` URI scheme is deferred until a second
backend lands.

Embed and `attachments` references resolve by shape: a bare filename under
`paths.attachments`, a path with a separator relative to the note's directory,
`~` against home, and an absolute path as-is.

## Pandoc defaults

HTML email depends on a trio of pandoc assets: a template (`email.html`), a Lua
filter (`email.lua`, Outlook-safe image widths), and a CSS theme (`email.css`,
the 600px column). These are generic, so mdmc ships them embedded in the binary
(`assets/pandoc/`) as *hidden fallback defaults*: they apply only when neither
the draft's frontmatter nor `[defaults]` specifies an asset. They have no
user-visible name.

Resolution: a value containing `/` is a path (used literally); otherwise it is a
name resolved under `paths.pandoc_data` with a per-type convention:

| field | name | resolves to |
| --- | --- | --- |
| `lua-filter: email` | `email` | `pandoc_data/filters/email.lua` |
| `template: email.html` | `email.html` | `pandoc_data/templates/email.html` |
| `css: email` | `email` | `pandoc_data/css/themes/email.css` |

(`template` takes the full filename, no extension appended.) `css` also accepts
`http(s)://` URLs. Writing `css: email` resolves to the path, never the embedded
asset.

`pandoc-raw: true` (frontmatter, optional boolean) suppresses the trio entirely —
vanilla pandoc with no template/filter/css.

Implementation: embedded assets are materialized to temp files before invoking
pandoc (its `--template`/`--lua-filter`/`--include-in-header` flags take file
paths). Once shipped, the `~/.local/share/pandoc/email.*` files and the
corresponding `[defaults]` entries become redundant and are removed at
integration time.

## Schema

The `mail-draft` frontmatter JSON Schema (draft 2020-12) is shipped at
`schema/mail-draft.schema.json` and embedded in the binary via `include_str!`.
`new` validates the frontmatter it builds against it (using the `jsonschema`
crate) before writing, and `write` validates the frontmatter of the note it
loads before compiling. There is no dependency on `~/kb` or the external
`kb-validate` tool. The kb-specific `paths` allow-list is not enforced — draft
location is controlled by `paths.source` in config.

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

## Crypto (OpenPGP)

Signing/encryption is a compile-time MIME transformation (RFC 3156 PGP/MIME),
not an RFC 5322 header concern and not a send-time concern. PGP/MIME only — no
inline-PGP. It applies to the message body, so the RFC 5322 headers are
untouched.

### Intent — frontmatter

Two optional booleans, default off:

```yaml
sign: true      # multipart/signed
encrypt: true   # multipart/encrypted
```

Both true means sign-then-encrypt (the signed structure wrapped in encrypted).
The draft body is markdown, so intent is expressed as fields, not as MML
directives.

### Key material — config

Per-account, mirroring neomutt's `pgp_sign_as`:

```toml
[accounts.rmsams-gmail]
signing_key = "0x12A96FA9"   # gpg -u / --local-user
encrypt_to = "0x12A96FA9"    # gpg --encrypt-to; defaults to signing_key when absent
```

`encrypt_to` adds the sender's own key as an additional session-key recipient on
the *same* ciphertext (OpenPGP encrypts the content once with a symmetric
session key and wraps that key once per recipient), so the single Sent copy
remains decryptable by the sender. One ciphertext, no duplication.

### gpg mapping

- sign: `gpg --detach-sign --armor -u <signing_key>` → `application/pgp-signature`
- encrypt: `gpg --encrypt --armor -r <recipient> --encrypt-to <encrypt_to || signing_key>`
  → `application/octet-stream`

(`--armor` is required: PGP/MIME parts are ASCII-armored.)

### Implementation approach

The `mime-meta-language` compiler has no PGP support (no `sign`/`encrypt`
grammar, no pgp features), and `mail-builder` has no crypto helpers. The seam is
`MmlCompileResult::into_msg_builder()`, which yields the `mail_builder`
`MessageBuilder` before serialization; `MessageBuilder.body` is a public field.
`compile_mml` keeps the builder in scope for the whole pipeline: inject inline
`Content-ID`s (builder level), optionally sign (serialize the body part via
`write_part` → `gpg --detach-sign --armor --digest-algo sha512` → wrap in
`multipart/signed`), then optionally encrypt (serialize again → `gpg --encrypt
--armor -r <recipient> --encrypt-to <self>` → wrap in `multipart/encrypted` with
`application/pgp-encrypted` + `application/octet-stream` parts) → serialize.
`mail-builder` emits CRLF consistently, so the signed bytes verify.

### Schema

Added optional `sign` and `encrypt` booleans to `mail-draft.schema.json`.

## Deferred

- The `msmtp-sent` equivalent (IMAP-append to Sent).
- Non-maildir delivery backends (URI scheme).
- Account signature support (neomutt `signature`; currently always empty).

## Open questions (from `~/kb/ops/notes/mail-generalization-rust-rewrite.md`)

- Keep the MML intermediate (current choice) or assemble MIME with `mail-builder`/`melib` directly.
- Consolidate account config across `mail.py` and `msmtp-sent.py` (mdmc now has its own `[accounts]`, which temporarily duplicates neomutt).
- License choice before open-sourcing.

# mdmc — design

`mdmc` (markdown mail compiler) is a Rust reimplementation of the Python `mail`
tool. It compiles markdown drafts to RFC 5322 email and never sends.

## Commands

`mdmc new` creates a draft (frontmatter + markdown body) under
`paths.source`, assigns its `message-id`, validates its frontmatter against the
shipped schema, and optionally opens `$EDITOR`. `mdmc compile` compiles a draft to
RFC 5322 and delivers it. `mdmc reconcile` marks drafts `sent` once their
message-id is in the account's sent maildir. `mdmc list` prints `status  slug`
rows with the status colour-coded. `mdmc decompile` turns a raw message back
into a markdown snapshot note. None of them ever send.

## Decompile

`mdmc decompile <input> [--stdout]` is the read side: it turns a raw RFC 5322
message into a markdown snapshot note (a `mail-snapshot`). The inverse of
`compile`. It never sends.

The `<input>` is disambiguated by shape: `-` reads a raw message from stdin; a
path to an existing file reads that file as raw message; anything else is
treated as a Message-ID, resolved by scanning the maildir tree under
`[paths] mail` (reusing `maildir::read_message_id`, which reads only the header
prefix). Notmuch is not required.

The snapshot note lives at `[paths] snapshots/<slug>.md`, where `slug` is a
filesystem-safe form of the message's Message-ID (identity for
`[A-Za-z0-9._@+-]`, percent-encode otherwise, strip a leading `.`, truncate ids
over 240 characters with an 8-hex hash). `--stdout` prints the note instead of
writing it (and suppresses attachment extraction). Extraction is idempotent — a
Message-ID whose snapshot already exists is skipped.

The frontmatter carries the envelope: `title` (the Subject, falling back to
`(no subject)`), `type` (`mail-snapshot`), `message-id` (the raw id, the key),
`from`, `date` (RFC 3339), and optional `to`/`cc` (address arrays),
`in-reply-to`, `references`, `attachments` (an array of `{ file, content-type }`
objects). The body is the converted message; `--link-attachments` additionally
appends an `## Attachments` section listing the discrete attachment files.

### Conversion

`mdmc decompile` parses the raw message with the `mail-parser` crate and renders
the body to markdown. Text is preferred: a `text/plain` part is used verbatim;
otherwise the `text/html` part is converted with `pandoc -f html -t gfm`.
Quoted reply chains stay in the single body; threads are not split.

Inline `cid:` images are rewritten by a generated Lua filter that walks the
pandoc AST: `mdmc` emits a temporary filter holding the `cid → filename` map,
and the filter rewrites `Image` sources before the writer runs. This operates on
the AST, not the HTML text, so it touches only real image elements.

### Attachments

Discrete attachments and inline `cid:` images are extracted to
`[paths] attachments/<message-id>--<name>` (the name sanitised to a single path
component). Inline images are embedded in the body (see Conversion) and are not
re-listed; discrete attachments appear in the `## Attachments` section only
under `--link-attachments`. The frontmatter `attachments` field always lists
every extracted file as `{ file, content-type }` — the MIME `Content-Type` is
preserved, not just the filename. A tracking-pixel filter is deferred.

## Status lifecycle

A draft's `status` moves `draft` (created, no message-id) → `pending`
(compiled and delivered to the drafts maildir) → `sent` (confirmed present in
the sent maildir). `reconcile` does the last transition by scanning `paths.source`
and, for each draft that is not yet `sent`, looking up its message-id in the
account's `sent` maildir.

## Message-id semantics

A draft's Message-ID is a random UUID v4 plus the local hostname, assigned once
at `new` time and persisted in the draft's frontmatter. `compile` reuses the
persisted id (falling back to generating one only for drafts that lack it), so
every delivery mode — `--stdout`, `--folder`, and the drafts maildir — emits the
same Message-ID. Maildir delivery overwrites any existing message carrying the
same Message-ID (scanning `new/` and `cur/` headers), so re-running `compile` is
idempotent and never leaves stale duplicate drafts.

## Compile pipeline

`compile <draft>`:

1. Load `config.toml` (paths, folders, accounts, defaults).
2. Load the draft's `---` frontmatter (YAML) + body.
3. Resolve the account's `from`/`realname` from `[accounts.<name>]`.
4. Reuse the draft's `message-id` (or generate one for legacy drafts).
5. Optionally append a reply quote (`notmuch show` → `id:` lookup).
6. Build the MML body: markdown → pandoc (lua filters, template, CSS) → html/plain,
   with `![[…]]` image embeds becoming inline `cid:` parts.
7. Compile the MML source (headers + body) to MIME via the `mime-meta-language`
   crate (`compiler` feature only, no `cli`).
8. Post-compile: inject `In-Reply-To`/`References` and inline `Content-ID`s,
   restore CRLF.
9. Deliver: `--stdout` → stdout, `--folder NAME` → `[folders]` maildir, otherwise
   the account's `drafts` maildir (then write `status: pending` back to the draft).

## Config schema

```toml
[paths]
source = "~/kb/ops/mail/drafts"      # markdown drafts directory
attachments = "~/kb/attachments"     # bare-filename base for embeds and attachments
pandoc_data = "~/.local/share/pandoc"
mail = "~/Mail"                      # maildir root for Message-ID resolution
snapshots = "~/kb/ops/mail/refs"     # decompiled snapshot notes

[accounts.rmsams-gmail]
from = "rmsams@gmail.com"
realname = "Robert Sams"
drafts = "~/Mail/rmsams-gmail.com/[Google Mail].Drafts"  # delivery maildir
sent = "~/Mail/rmsams-gmail.com/[Google Mail].Sent Mail"  # sent maildir

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
can use `source = "drafts"`. `paths.source` and `paths.attachments` are required
(no default — the tool must not assume a drafts location). `paths.mail` is
required only for Message-ID resolution (`decompile`), and `paths.snapshots`
only for `decompile` without `--stdout`. `paths.pandoc_data`
defaults to `~/.local/share/pandoc`. mdmc does not read neomutt config. Bare delivery
paths are maildirs; a `maildir:`/`imaps:` URI scheme is deferred until a second
backend lands.

Embed and `attachments` references resolve by shape: a bare filename under
`paths.attachments`, a path with a separator relative to the draft's directory,
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
crate) before writing, and `compile` validates the frontmatter of the draft it
loads before compiling. There is no dependency on `~/kb` or the external
`kb-validate` tool. The kb-specific `paths` allow-list is not enforced — draft
location is controlled by `paths.source` in config.

The read side's `mail-snapshot` schema ships at
`schema/mail-snapshot.schema.json`. `decompile` generates valid frontmatter by
construction, so it does not validate; the schema is for external validation
(e.g. `kb-validate`).

## Module map

- `config.rs` — config.toml loading, `~` expansion, `[accounts]`/`[folders]` resolution.
- `compile.rs` — the `compile` subcommand (compile + maildir delivery).
- `decompile.rs` — the `decompile` subcommand (raw message → markdown snapshot).
- `frontmatter.rs` — `---`-delimited YAML → `Draft`; schema validation; ordered YAML rendering; `set_fields` in-place rewrite.
- `new.rs` — the `new` subcommand (draft creation).
- `mml.rs` — MML body construction + `compile_mml` (the mml crate).
- `quote.rs` — notmuch reply quoting.
- `util.rs` — hostname and Message-ID generation.

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

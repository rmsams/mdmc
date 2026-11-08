# mdmc

mdmc compiles markdown drafts into RFC 5322 email. It never sends.

A draft is a markdown file with YAML frontmatter that holds the envelope
(account, subject, recipients) and the message body below it. `mdmc new`
creates the draft; `mdmc compile` compiles it to MIME and delivers it to stdout, a
named folder, or an account's drafts maildir.

## Features

- Markdown body converted with pandoc: to HTML (with a template, Lua filters,
  and a CSS theme) or to plain text.
- An embedded email template, filter, and theme are used by default, so HTML
  email works without extra setup. Override them per draft or per config, or
  disable them with `pandoc-raw`.
- OpenPGP signing and encryption (PGP/MIME, RFC 3156) via gpg.
- A message-id is assigned at `new` time and reused on every `compile`, so
  recompiling is idempotent.
- `decompile` turns a raw message back into a markdown snapshot note (the read
  side): text/plain verbatim, or text/html via pandoc, with attachments
  extracted.
- Frontmatter is validated against a JSON Schema shipped with the binary.
- Delivery is to stdout (`--stdout`), a `[folders]` entry (`--folder`), or the
  account's drafts maildir. Never to an SMTP server.

## Install

```sh
cargo install --path .
```

Or build the Nix package from the flake:

```sh
nix build
```

For development, use devenv:

```sh
devenv shell
```

## Usage

Create a draft:

```sh
mdmc new --account rmsams-gmail --to "a@example.com" --subject "Hello" --format html --body "Hi **there**."
```

Compile it and print to stdout:

```sh
mdmc compile hello --stdout
```

Compile it and deliver it to the account's drafts maildir:

```sh
mdmc compile hello
```

Mark drafts sent once their message-id is in the account's sent maildir:

```sh
mdmc reconcile
```

List drafts with their status (colour-coded):

```sh
mdmc list
```

Decompile a message to a markdown snapshot note — by Message-ID (scanned from
the maildir tree), by file, or from stdin:

```sh
mdmc decompile 20260101.123456.abc@x1g11
mdmc decompile ./raw.eml
notmuch show --format=raw id:20260101.123456.abc@x1g11 | mdmc decompile -
```

`--stdout` prints the note instead of writing it to `paths.snapshots`, and
`--link-attachments` appends an `## Attachments` section listing the extracted files.

None of these commands send. Sending is left to the tool that owns the
delivery maildir, or to an SMTP client that reads RFC 5322 from standard input.
For example, pipe the compiled message straight into msmtp:

```sh
mdmc compile hello --stdout | msmtp --read-envelope-from --read-recipients
```

Run `mdmc new --help` or `mdmc compile --help` for the full option list.

## Configuration

mdmc reads its config from `--config PATH`, or a `.mdmc.toml` in the current
directory (or any parent), or `~/.config/mail/config.toml` as a fallback. A
local `.mdmc.toml` merges over the global one, and relative paths resolve
against the config file's directory.

```toml
[paths]
source = "~/mail/drafts"             # where markdown drafts live (required)
attachments = "~/mail/attachments"   # bare-filename base for embeds and attachments (required)
pandoc_data = "~/.local/share/pandoc"
mail = "~/Mail"                      # maildir root for decompile Message-ID lookup
snapshots = "~/mail/refs"            # where decompiled snapshot notes live

[accounts.rmsams-gmail]
from = "rmsams@gmail.com"
realname = "Robert Sams"
drafts = "~/Mail/example/Drafts"     # delivery maildir
sent = "~/Mail/example/Sent"         # sent maildir (for reconcile)
signing_key = "0x12A96FA9"           # gpg key for signing
encrypt_to = "0x12A96FA9"            # optional; defaults to signing_key

[folders]
refs = "~/Mail/example/Reference"

[defaults]
account = "rmsams-gmail"
css = "email"                        # name or path; html only
template = "email.html"              # name or path
lua-filters = ["email"]              # names or paths
```

An asset name without a `/` resolves under `pandoc_data` (filters, templates,
and CSS themes). A value with a `/` is a path and is used directly.

Embed and attachment references resolve by shape: a bare filename (`foo.png`)
resolves under `paths.attachments`; a path with a `/` resolves relative to the
draft's directory; `~` resolves against home; an absolute path is used as-is.

## Draft format

The frontmatter fields:

```yaml
---
title: "Hello"
type: mail-draft
status: draft
account: rmsams-gmail
format: html            # text (default), html, or plain
subject: "Hello"
to:
  - a@example.com
sign: true              # optional: sign with the account's key
encrypt: true           # optional: encrypt to the recipients
template: email.html    # optional override
css: email              # optional override
lua-filter: email       # optional override
pandoc-raw: true        # optional: skip template, filter, and css
---

Body goes here, as markdown.
```

See `docs/spec.md` for the design, and `docs/rfcs.md` for how the tool handles
the relevant email RFCs.

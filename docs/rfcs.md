# Email RFCs

A reference of the RFCs that govern email messages, and how mdmc handles each.
The decompile side (`decompile`) parses with `mail-parser`; the compile side
(`new`/`compile`) assembles with `mime-meta-language` (`mml`) and `mail-builder`.

## Message format

### RFC 5322 — Internet Message Format

The envelope: header fields plus a body of lines, CRLF-terminated. Defines
`Message-ID`, `In-Reply-To`, `References`, `Date`, `From`/`To`/`Cc`.

- **Compile** emits RFC 5322 (via `mml` + `mail-builder`).
- **Decompile** parses it (via `mail-parser`); the envelope becomes the snapshot
  frontmatter (`message-id`, `from`, `date`, `to`/`cc`, `in-reply-to`,
  `references`).

### RFC 5321 — SMTP

Transport only. Out of scope — mdmc never sends; delivery is to a maildir.

## MIME

### RFC 2045 — MIME Part One: Format of Internet Message Bodies

Defines `Content-Type` and `Content-Transfer-Encoding` (7bit, 8bit, base64,
quoted-printable).

- **Compile** selects a transfer encoding (via `mml`).
- **Decompile** decodes it (`mail-parser` `decoded()` yields the raw bytes).

### RFC 2046 — MIME Part Two: Media Types

Defines the multipart types (`mixed`, `alternative`, `related`, `digest`) and
the `text/*` types.

- **Compile** builds `multipart/alternative` (text + html), `multipart/related`
  (inline images), `multipart/mixed` (attachments).
- **Decompile** walks the tree and collapses it to text body / html body /
  attachments (the RFC 8621 model). `text/plain` is preferred, else `text/html`
  is converted.

### RFC 2047 — MIME Part Three: Message Header Extensions for Non-ASCII Text

Encoded-words (`=?UTF-8?...?=`) for non-ASCII header text — subject lines and
display names.

- **Compile** emits encoded-words (via `mml`).
- **Decompile** decodes them (via `mail-parser`).

### RFC 2049 — MIME Part Five: Conformance Criteria and Examples

Best-current-practice; nothing to implement.

## Headers and parameters

### RFC 2183 — Content-Disposition

`attachment` vs `inline`, plus the `filename` parameter.

- **Compile** emits a disposition for attachments.
- **Decompile** reads `filename` (`mail-parser` `attachment_name()`, which prefers
  the disposition filename and falls back to the `Content-Type` `name`).

### RFC 2231 — MIME Parameter Value and Encoded Word Extensions

How non-ASCII and over-long parameter values (notably filenames) are encoded
(`filename*=UTF-8''…` and `parameter*N=` continuation).

- **Decompile** decodes these (`mail-parser`). The decoded filename is honoured: it
  is sanitised to a single path component, prefixed with `<hash>-` (inline
  assets `cid-<hash>-`), and written to `[paths] snapshot-assets/`. The
  extension is just part of that string — it carries no semantic weight; the
  authoritative type is `Content-Type`, which is what mdmc keys on (e.g.
  detecting `image/*`), never the extension.

## Identifiers

### RFC 2392 — Content-ID and Message-ID URLs

Defines the `cid:` (and `mid:`) URI schemes.

- **Compile** emits `Content-ID` for inline images, referenced as `cid:`.
- **Decompile** rewrites `cid:` image sources to the extracted files (a Lua filter
  walks the pandoc AST), yielding `![alt](<name>)`.

## Body formats

### RFC 3676 — text/plain format=flowed

Soft line-break markers that let clients reflow plain text.

- **Decompile** renders it verbatim (no reflow); it degrades to plain text.

### RFC 1896 — text/enriched (obsolete)

An obsolete rich-text body format.

- **Decompile** treats it as plain text (`mail-parser` classifies any `text/*`
  except html as text), so it renders as a verbatim dump.

### RFC 5545 — iCalendar

`text/calendar` invitations, often carried as a body part.

- Not special-cased: surfaces as an attachment, handled by calendar tooling,
  not mdmc.

## Security

### RFC 1847 — Security Multiparts for MIME

The general framework for `multipart/signed` and `multipart/encrypted`. No
direct handling; the concrete binding is RFC 3156.

### RFC 3156 — MIME Security with OpenPGP (PGP/MIME)

`multipart/signed` and `multipart/encrypted` for OpenPGP.

- **Compile** signs and/or encrypts via gpg.
- **Decompile** is not implemented: the signature/ciphertext parts surface as
  attachments; there is no verification or decryption.

## Conceptual model

### RFC 8621 — JMAP mail

Not a wire format; it defines the `textBody` / `htmlBody` / `attachments`
model that `mail-parser` exposes and that `decompile` maps onto: text →
verbatim markdown, html → gfm via pandoc, attachments → extracted files.

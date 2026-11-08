# Ideas

Potential feature enhancements and open questions for mdmc. Not scheduled.

## Enhancements

- **Export the JSON Schema** — a CLI command (e.g. `mdmc schema mail-draft`) that
  prints a shipped schema to stdout, so external tools (`kb-validate`,
  note-typing) can consume it without reading the source tree.
- **Print the effective config** — a command (e.g. `mdmc config`) that prints the
  merged, resolved config (after local/global merge, `~` expansion, and path
  resolution), to debug path/editor/account resolution.
- **Batch decompile** — decompile every message in a maildir (or a set of
  Message-IDs), rather than one at a time. Kept notmuch-free by scanning the
  maildir tree.
- **Tracking-pixel filter** — drop 1×1 images and known tracker domains when
  extracting attachments (deferred from the read-side design).
- **Forwarded messages** — a nested `message/rfc822` part renders as nothing:
  the outer body is empty and only the nested attachments surface. Consider
  walking the nested message's body.
- **Quote collapsing** — long reply chains render as a wall of `>` lines; an
  option to trim or collapse quoted context would improve readability.
- **Maildir scan cache** — `find_message_in_tree` walks the whole mail tree per
  Message-ID; a cached id→file map would matter once decompile runs in bulk.
- **Drop the notmuch dependency** — reply quoting (`quote.rs`) is the last place
  mdmc shells out to `notmuch show`; rework it to resolve the message via the
  same maildir scan `decompile` uses, making mdmc fully notmuch-free.
- **Auto-fill recipients on reply** — `mdmc new --in-reply-to` should look up the
  original message and pre-populate `to`/`cc`, supporting both `reply` (to the
  sender) and `reply all` (to sender + other recipients) variants.

## Open questions

- **Missing `Message-ID`** — decompile hard-errors when a message has no
  `Message-ID` header; consider falling back to a content hash.
- **Timezone** — `date` is normalised to UTC (`to_rfc3339`), discarding the
  sender's original offset. Decide whether to preserve it.
- **`--stdout` preview** — the printed note references attachment filenames that
  were never extracted; acceptable as a preview, but a consumer could mistake it
  for a valid note.

- **Encrypted messages** — `multipart/encrypted` (PGP/MIME) currently yields an
  empty body and the ciphertext is silently dropped (it has no filename or
  `Content-ID`, so it is not extracted). Decide whether to surface the
  ciphertext as an attachment instead of losing it.
- **Preserve the raw HTML** — the `text/html` body is converted to markdown and
  the original HTML discarded, so the snapshot is lossy. Consider saving the
  raw HTML as an attachment for byte-faithful fidelity.
- **Attachment cleanup on delete** — when a snapshot note is deleted, should its
  `<message-id>--` files in `attachments/` be removed too?
- **Deep-open link** — the snapshot has no `neomutt:`/deep-open field (kept
  MUA-agnostic); decide whether and how the integration layer re-adds one.
- **Inline embed syntax** — inline `cid:` images are currently emitted as
  standard markdown `![alt](<disk-name>)`; Obsidian resolves bare names, but a
  stricter renderer would want a relative path. Confirm the preferred form.

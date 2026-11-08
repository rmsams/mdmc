# Ideas

Potential feature enhancements and open questions for mdmc. Not scheduled.

## Enhancements

- **Export the JSON Schema** — a CLI command (e.g. `mdmc schema mail-draft`) that
  prints a shipped schema to stdout, so external tools (`kb-validate`,
  note-typing) can consume it without reading the source tree.
- **Batch decompile** — decompile every message in a maildir (or a set of
  Message-IDs), rather than one at a time. Kept notmuch-free by scanning the
  maildir tree.
- **Tracking-pixel filter** — drop 1×1 images and known tracker domains when
  extracting attachments (deferred from the read-side design).

## Open questions

- **Attachment cleanup on delete** — when a snapshot note is deleted, should its
  `<message-id>--` files in `attachments/` be removed too?
- **Deep-open link** — the snapshot has no `neomutt:`/deep-open field (kept
  MUA-agnostic); decide whether and how the integration layer re-adds one.
- **Inline embed syntax** — inline `cid:` images are currently emitted as
  standard markdown `![alt](<disk-name>)`; Obsidian resolves bare names, but a
  stricter renderer would want a relative path. Confirm the preferred form.

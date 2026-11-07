# mdmc — design

`mdmc` (markdown mail compiler) is a Rust reimplementation of the Python `mail`
tool. It compiles markdown draft notes to RFC 5322 email and never sends.

## Compile pipeline

`write <draft> --stdout`:

1. Load `~/.config/mail/config.toml` (paths, defaults).
2. Load the draft note's `---` frontmatter (YAML) + body.
3. Resolve the account's `from`/`realname` from neomutt config.
4. Optionally append a reply quote (`notmuch show` → `id:` lookup).
5. Build the MML body: markdown → pandoc (lua filters, template, CSS) → html/plain,
   with `![[…]]` image embeds becoming inline `cid:` parts.
6. Compile the MML source (headers + body) to MIME via the `mime-meta-language`
   crate (`compiler` feature only, no `cli`).
7. Post-compile: inject `In-Reply-To`/`References` and inline `Content-ID`s,
   restore CRLF, emit to stdout.

## Module map

- `config.rs` — config.toml loading, `~` expansion, path helpers.
- `frontmatter.rs` — `---`-delimited YAML → `Draft`.
- `account.rs` — neomutt account discovery/parse.
- `mml.rs` — MML body construction + `compile_mml` (the mml crate).
- `quote.rs` — notmuch reply quoting.
- `write.rs` — the `write` subcommand.

## MML crate

Dependency `mime-meta-language` links as lib name `mml`. API:

```rust
mml::compiler::message::MmlCompilerBuilder::new().build(&source)?.compile()?.into_vec()?
```

`compile` is synchronous and takes the full message (headers + blank line + MML
body). The compiler adds `MIME-Version: 1.0` and a `Date` header, and re-adds
angle brackets to `Message-ID`/`In-Reply-To`/`References`.

## Deferred

- `--folder` / maildir delivery (the delivery-target abstraction).
- The `new` subcommand (draft note creation).
- The `msmtp-sent` equivalent (IMAP-append to Sent).

## Open questions (from `~/kb/ops/notes/mail-generalization-rust-rewrite.md`)

- Keep the MML intermediate (current choice) or assemble MIME with `mail-builder`/`melib` directly.
- Consolidate account config across `mail.py` and `msmtp-sent.py`.
- License choice before open-sourcing.

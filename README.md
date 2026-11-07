# mdmc

Markdown mail compiler — a Rust reimplementation of the Python `mail` tool.

`mdmc` composes email from markdown draft notes. `mdmc new` creates a draft
note; `mdmc write` compiles it to RFC 5322 via MML and either prints it to
stdout (`--stdout`) or delivers it to a maildir. `mdmc` never sends.

The compile pipeline is markdown → pandoc (custom filters, templates, CSS) →
HTML/plain, wrapped in MML, then compiled to MIME by the `mime-meta-language`
crate.

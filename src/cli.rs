use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "mdmc",
    version,
    about = "Markdown mail compiler — compile markdown drafts to RFC 5322 email"
)]
pub struct Cli {
    /// Path to config.toml (default ~/.config/mdmc/config.toml)
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum Command {
    /// Create a new email draft
    New {
        /// Account name (default: [defaults].account in config)
        #[arg(long)]
        account: Option<String>,
        /// Recipient(s), comma-separated
        #[arg(long)]
        to: Option<String>,
        /// CC recipient(s), comma-separated
        #[arg(long)]
        cc: Option<String>,
        /// BCC recipient(s), comma-separated
        #[arg(long)]
        bcc: Option<String>,
        /// Email subject line
        #[arg(long)]
        subject: Option<String>,
        /// Message-ID to reply to (sender only); not with --to/--cc
        #[arg(long, conflicts_with_all = ["reply_all", "forward", "forward_attach"])]
        reply: Option<String>,
        /// Message-ID to reply to all recipients; --to/--cc override the defaults
        #[arg(long, conflicts_with_all = ["reply", "forward", "forward_attach"])]
        reply_all: Option<String>,
        /// Message-ID of the message to forward (embedded as text at compile time)
        #[arg(long, conflicts_with_all = ["reply", "reply_all", "forward_attach"])]
        forward: Option<String>,
        /// Message-ID of the message to forward as a message/rfc822 attachment
        #[arg(long, conflicts_with_all = ["reply", "reply_all", "forward"])]
        forward_attach: Option<String>,
        /// Attachment file path(s)
        #[arg(long)]
        attachments: Vec<String>,
        /// Message body (markdown)
        #[arg(long)]
        body: Option<String>,
        /// Body format: text (default), html, or plain
        #[arg(long)]
        format: Option<String>,
        /// CSS theme name/path/URL (html only)
        #[arg(long)]
        css: Option<String>,
        /// Pandoc template name or path
        #[arg(long)]
        template: Option<String>,
        /// Pandoc lua-filter name or path
        #[arg(long = "lua-filter")]
        lua_filter: Option<String>,
        /// Use vanilla pandoc (no template/filter/css)
        #[arg(long = "pandoc-raw")]
        pandoc_raw: bool,
        /// Sign the message with the account's signing key
        #[arg(long)]
        sign: bool,
        /// Encrypt the message to the recipients
        #[arg(long)]
        encrypt: bool,
        /// Open $EDITOR on the draft after writing
        #[arg(long)]
        editor: bool,
    },
    /// Open a draft in $EDITOR, importing it from a drafts maildir when its markdown doesn't exist yet
    Draft {
        /// Draft slug, path, or Message-ID; omit to pick a draft interactively
        input: Option<String>,
        /// Editor name from [editors] (default: first configured, else $EDITOR)
        #[arg(long)]
        editor: Option<String>,
    },
    /// Compile a draft to a message (RFC 5322)
    Compile {
        /// Draft filename (slug) or path to compile
        draft: String,
        /// Print the compiled message to stdout instead of a maildir
        #[arg(long)]
        stdout: bool,
        /// Write to this maildir: a [folders] config name or a literal path
        #[arg(long)]
        folder: Option<String>,
        /// Allow compiling a draft already marked sent
        #[arg(long)]
        again: bool,
    },
    /// Decompile a message to a markdown snapshot note
    Decompile {
        /// Message-ID, a raw message file path, or `-` for stdin
        input: String,
        /// Print the note to stdout instead of writing to [paths] snapshots
        #[arg(long)]
        stdout: bool,
        /// Append an `## Attachments` section listing extracted files
        #[arg(long = "link-attachments")]
        link_attachments: bool,
    },
    /// Sync draft status with the maildirs (forward-only)
    ///
    /// Marks a draft `pending` when its message-id is in the account's drafts
    /// maildir, and `sent` when it is in the sent maildir. Never downgrades:
    /// `pending` is not regressed to `draft`, and `sent` is terminal. A message
    /// found in both drafts and sent is marked `sent` with a warning about the
    /// stale drafts copy.
    Reconcile,
    /// Pick a draft or snapshot (interactive).
    Pick {
        #[command(subcommand)]
        domain: PickDomain,
    },
    /// List drafts or snapshots to stdout.
    List {
        #[command(subcommand)]
        domain: ListDomain,
    },
    /// Print a message as markdown to stdout (no attachment extraction).
    View {
        /// Message-ID, a raw message file path, or `-` for stdin
        input: String,
        /// Print only a header + body, omitting the frontmatter
        #[arg(long)]
        body_only: bool,
    },
    /// Print the effective (merged, resolved) config as TOML.
    Config,
}

#[derive(Subcommand)]
pub enum ListDomain {
    /// List draft file paths
    Drafts {
        /// Print a JSON array instead of plain paths
        #[arg(long)]
        json: bool,
    },
    /// List snapshot file paths
    Snapshots {
        /// Print a JSON array instead of plain paths
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum PickDomain {
    /// Pick a draft and print its path
    Drafts {
        /// Allow selecting multiple drafts
        #[arg(long)]
        multi: bool,
        /// Number of picker rows to show (default: [lookup] page_size)
        #[arg(long = "page-size")]
        page_size: Option<usize>,
    },
    /// Pick a snapshot and print its path
    Snapshots {
        /// Allow selecting multiple snapshots
        #[arg(long)]
        multi: bool,
        /// Number of picker rows to show (default: [lookup] page_size)
        #[arg(long = "page-size")]
        page_size: Option<usize>,
    },
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::CommandFactory;

    /// Generate roff man pages under `man/man1/` (run explicitly:
    /// `cargo test --bin mdmc generate_man_pages -- --ignored`).
    #[test]
    #[ignore]
    fn generate_man_pages() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("man")
            .join("man1");
        std::fs::create_dir_all(&dir).unwrap();
        clap_mangen::generate_to(Cli::command(), &dir).unwrap();
    }
}

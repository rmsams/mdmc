mod compile;
mod config;
mod decompile;
mod edit;
mod frontmatter;
mod list;
mod maildir;
mod mml;
mod new;
mod quote;
mod reconcile;
mod util;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "mdmc",
    version,
    about = "Markdown mail compiler — compile markdown drafts to RFC 5322 email"
)]
struct Cli {
    /// Path to config.toml (default ~/.config/mail/config.toml)
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Command {
    /// Create a new email draft. Never sends.
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
        /// Message-ID being replied to
        #[arg(long)]
        in_reply_to: Option<String>,
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
    /// Open a draft in $EDITOR (interactive picker when no input). Never sends.
    Edit {
        /// Draft slug, path, or Message-ID to open; omit for a picker
        input: Option<String>,
        /// Editor name from [editors] (default: first configured, else $EDITOR)
        #[arg(long)]
        editor: Option<String>,
        /// Include sent drafts in the picker
        #[arg(long)]
        all: bool,
    },
    /// Compile a draft to a message (RFC 5322). Never sends.
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
    /// Decompile a message to a markdown snapshot note. Never sends.
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
    /// Mark drafts sent when their message-id is in the account's sent maildir.
    Reconcile,
    /// List drafts (or snapshots). Never sends.
    List {
        /// List snapshots instead of drafts
        #[arg(long)]
        snapshots: bool,
        /// Print file paths only
        #[arg(long)]
        paths: bool,
        /// Pick files interactively (multi-select) and print their paths
        #[arg(long)]
        pick: bool,
    },
    /// Print the effective (merged, resolved) config as TOML. Never sends.
    Config,
}

fn main() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let cli = Cli::parse();
    let cfg = config::discover(cli.config);

    let result = match cli.command {
        Command::New {
            account,
            to,
            cc,
            bcc,
            subject,
            in_reply_to,
            attachments,
            body,
            format,
            css,
            template,
            lua_filter,
            pandoc_raw,
            sign,
            encrypt,
            editor,
        } => new::new(
            &cfg,
            account,
            to,
            cc,
            bcc,
            subject,
            in_reply_to,
            attachments,
            body,
            format,
            css,
            template,
            lua_filter,
            pandoc_raw,
            sign,
            encrypt,
            editor,
        ),
        Command::Edit { input, editor, all } => {
            edit::edit(input.as_deref(), editor.as_deref(), all, &cfg)
        }
        Command::Compile {
            draft,
            stdout,
            folder,
            again,
        } => compile::compile(stdout, folder, again, &draft, &cfg),
        Command::Decompile {
            input,
            stdout,
            link_attachments,
        } => decompile::decompile(&input, stdout, link_attachments, &cfg),
        Command::Reconcile => reconcile::reconcile(&cfg),
        Command::List {
            snapshots,
            paths,
            pick,
        } => list::list(snapshots, paths, pick, &cfg),
        Command::Config => cfg.to_toml().map(|s| println!("{s}")),
    };

    if let Err(msg) = result {
        eprintln!("Error: {msg}");
        std::process::exit(1);
    }
}

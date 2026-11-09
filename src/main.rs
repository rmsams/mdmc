mod addr;
mod compile;
mod config;
mod decompile;
mod edit;
mod frontmatter;
mod index;
mod list;
mod lookup;
mod maildir;
mod mml;
mod new;
mod notmuch;
mod quote;
mod reconcile;
mod reply;
mod util;
mod view;

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
        #[arg(long, conflicts_with = "reply_all")]
        reply: Option<String>,
        /// Message-ID to reply to all recipients; --to/--cc override the defaults
        #[arg(long, conflicts_with = "reply")]
        reply_all: Option<String>,
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
    /// Open a draft in $EDITOR
    Edit {
        /// Draft slug, path, or Message-ID to open
        input: String,
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
    /// Mark drafts sent when their message-id is in the account's sent maildir.
    Reconcile,
    /// Pick a draft, snapshot, message, or address (interactive).
    Pick {
        #[command(subcommand)]
        domain: PickDomain,
    },
    /// List drafts, snapshots, messages, or addresses to stdout.
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
    /// Build (or rebuild) the lookup index from notmuch.
    Index,
}

#[derive(Subcommand)]
enum ListDomain {
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
    /// List message-ids
    Msgs {
        /// Restrict to messages whose sender matches this substring
        #[arg(long)]
        from: Option<String>,
        /// Only consider messages from the last N days (0 = no bound)
        #[arg(long)]
        last: Option<u32>,
        /// Print a JSON array instead of plain ids
        #[arg(long)]
        json: bool,
    },
    /// List addresses
    Addr {
        /// Only consider addresses seen in the last N days (0 = no bound)
        #[arg(long)]
        last: Option<u32>,
        /// Print a JSON array instead of plain values
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum PickDomain {
    /// Pick a draft and print its path
    Drafts {
        /// Allow selecting multiple drafts
        #[arg(long)]
        multi: bool,
        /// Print multiple selections one per line instead of comma-separated
        #[arg(long)]
        lines: bool,
    },
    /// Pick a snapshot and print its path
    Snapshots {
        /// Allow selecting multiple snapshots
        #[arg(long)]
        multi: bool,
        /// Print multiple selections one per line instead of comma-separated
        #[arg(long)]
        lines: bool,
    },
    /// Pick a message and print its message-id
    Msgs {
        /// Restrict to messages whose sender matches this substring
        #[arg(long)]
        from: Option<String>,
        /// Only consider messages from the last N days (0 = no bound)
        #[arg(long)]
        last: Option<u32>,
        /// Allow selecting multiple messages
        #[arg(long)]
        multi: bool,
        /// Print multiple selections one per line instead of comma-separated
        #[arg(long)]
        lines: bool,
    },
    /// Pick an address and print `Name <email>`
    Addr {
        /// Only consider addresses seen in the last N days (0 = no bound)
        #[arg(long)]
        last: Option<u32>,
        /// Allow selecting multiple addresses
        #[arg(long)]
        multi: bool,
        /// Print multiple selections one per line instead of comma-separated
        #[arg(long)]
        lines: bool,
    },
}

fn main() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let cli = Cli::parse();
    let cfg = config::discover(cli.config);

    if let Err(msg) = run(cli.command, &cfg) {
        eprintln!("Error: {msg}");
        std::process::exit(1);
    }
}

fn run(command: Command, cfg: &config::Config) -> Result<(), String> {
    match command {
        Command::New {
            account,
            to,
            cc,
            bcc,
            subject,
            reply,
            reply_all,
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
            cfg,
            account,
            to,
            cc,
            bcc,
            subject,
            reply,
            reply_all,
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
        Command::Edit { input, editor } => edit::edit(&input, editor.as_deref(), cfg),
        Command::Compile {
            draft,
            stdout,
            folder,
            again,
        } => compile::compile(stdout, folder, again, &draft, cfg),
        Command::Decompile {
            input,
            stdout,
            link_attachments,
        } => decompile::decompile(&input, stdout, link_attachments, cfg),
        Command::Reconcile => reconcile::reconcile(cfg),
        Command::Config => cfg.to_toml().map(|s| println!("{s}")),
        Command::View { input, body_only } => view::view(&input, body_only, cfg),
        Command::Index => {
            let query = cfg.index_query()?;
            let db = cfg.paths_index();
            let n = crate::index::build(query, &db)?;
            println!("indexed {n} messages in {}", db.display());
            Ok(())
        }
        Command::List { domain } => match domain {
            ListDomain::Drafts { json } => list::list_drafts(json, cfg),
            ListDomain::Snapshots { json } => list::list_snapshots(json, cfg),
            ListDomain::Msgs { from, last, json } => {
                lookup::list_msgs(from.as_deref(), last.unwrap_or(30), json, cfg)
            }
            ListDomain::Addr { last, json } => lookup::list_addr(last.unwrap_or(30), json, cfg),
        },
        Command::Pick { domain } => match domain {
            PickDomain::Drafts { multi, lines } => list::pick_drafts(multi, lines, cfg),
            PickDomain::Snapshots { multi, lines } => list::pick_snapshots(multi, lines, cfg),
            PickDomain::Msgs {
                from,
                last,
                multi,
                lines,
            } => lookup::pick_msgs(from.as_deref(), last.unwrap_or(30), multi, lines, cfg),
            PickDomain::Addr { last, multi, lines } => {
                lookup::pick_addr(last.unwrap_or(30), multi, lines, cfg)
            }
        },
    }
}

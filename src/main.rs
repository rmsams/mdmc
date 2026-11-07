mod config;
mod frontmatter;
mod mml;
mod new;
mod quote;
mod util;
mod write;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "mdmc",
    version,
    about = "Markdown mail compiler — compile markdown draft notes to RFC 5322 email"
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
    /// Create a new email draft note. Never sends.
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
        /// Open $EDITOR on the note after writing
        #[arg(long)]
        editor: bool,
    },
    /// Compile a draft note to a message (RFC 5322). Never sends.
    Write {
        /// Draft filename (slug) or path to compile
        draft: String,
        /// Print the compiled message to stdout instead of a maildir
        #[arg(long)]
        stdout: bool,
        /// Write to this maildir: a [folders] config name or a literal path
        #[arg(long)]
        folder: Option<String>,
    },
}

fn main() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let cli = Cli::parse();
    let config_path = cli
        .config
        .unwrap_or_else(|| config::expand_home("~/.config/mail/config.toml"));

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
            editor,
        } => new::new(
            &config_path,
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
            editor,
        ),
        Command::Write {
            draft,
            stdout,
            folder,
        } => write::write(stdout, folder, &draft, &config_path),
    };

    if let Err(msg) = result {
        eprintln!("Error: {msg}");
        std::process::exit(1);
    }
}

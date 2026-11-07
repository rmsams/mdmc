mod account;
mod config;
mod frontmatter;
mod mml;
mod quote;
mod write;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "mdmc",
    version,
    about = "Markdown mail compiler — compile markdown draft notes to RFC 5322 email"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
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
    let result = match cli.command {
        Command::Write {
            draft,
            stdout,
            folder,
        } => write::write(stdout, folder, &draft),
    };

    if let Err(msg) = result {
        eprintln!("Error: {msg}");
        std::process::exit(1);
    }
}

mod addr;
mod cli;
mod compile;
mod config;
mod decompile;
mod draft;
mod frontmatter;
mod list;
mod maildir;
mod mcp;
mod mml;
mod new;
mod notmuch;
mod quote;
mod reconcile;
mod reply;
mod search;
mod util;
mod view;

use cli::{Cli, Command, ListDomain, PickDomain, SchemaType};

use clap::Parser;

fn main() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let cli = Cli::parse();
    let cfg = match config::discover(cli.config) {
        Ok(cfg) => cfg,
        Err(msg) => {
            eprintln!("Error: {msg}");
            std::process::exit(1);
        }
    };

    if matches!(&cli.command, Command::Mcp) {
        if let Err(msg) = mcp::serve(&cfg) {
            eprintln!("Error: {msg}");
            std::process::exit(1);
        }
        return;
    }

    match run(cli.command, &cfg) {
        Ok(out) => {
            use std::io::Write;
            let mut stdout = std::io::stdout().lock();
            let _ = stdout.write_all(out.as_bytes());
        }
        Err(msg) => {
            eprintln!("Error: {msg}");
            std::process::exit(1);
        }
    }
}

fn run(command: Command, cfg: &config::Config) -> Result<String, String> {
    match command {
        Command::New {
            account,
            to,
            cc,
            bcc,
            subject,
            reply,
            reply_all,
            forward,
            forward_attach,
            attachments,
            body,
            format,
            css,
            template,
            lua_filter,
            link_format,
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
            forward,
            forward_attach,
            attachments,
            body,
            format,
            css,
            template,
            lua_filter,
            link_format.map(|l| l.as_str().to_string()),
            pandoc_raw,
            sign,
            encrypt,
            editor,
        ),
        Command::Draft {
            input,
            editor,
            compile,
        } => draft::draft(input.as_deref(), editor.as_deref(), compile, cfg),
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
            link_format,
        } => decompile::decompile(
            &input,
            stdout,
            link_attachments,
            link_format.map(|l| l.as_str()),
            cfg,
        ),
        Command::Reconcile => reconcile::reconcile(cfg),
        Command::Config => cfg.to_toml().map(|s| format!("{s}\n")),
        Command::View { input, body_only } => view::view(&input, body_only, cfg),
        Command::List { domain } => match domain {
            ListDomain::Drafts { json } => list::list_drafts(json, cfg),
            ListDomain::Snapshots { json } => list::list_snapshots(json, cfg),
        },
        Command::Pick { domain } => match domain {
            PickDomain::Drafts {
                multi,
                page_size,
                field,
            } => list::pick_drafts(
                multi,
                page_size.unwrap_or_else(|| cfg.page_size()),
                field.as_deref(),
                cfg,
            ),
            PickDomain::Snapshots {
                multi,
                page_size,
                field,
            } => list::pick_snapshots(
                multi,
                page_size.unwrap_or_else(|| cfg.page_size()),
                field.as_deref(),
                cfg,
            ),
        },
        Command::Search { query, limit, json } => search::search(&query, limit, json),
        Command::Address { query, json } => search::address(query.as_deref(), json, cfg),
        Command::Schema { schema } => {
            let s = match schema {
                SchemaType::MailDraft => frontmatter::mail_draft_schema(),
                SchemaType::MailSnapshot => frontmatter::mail_snapshot_schema(),
            };
            Ok(format!("{s}\n"))
        }
        Command::Hash { id } => Ok(format!("{}\n", util::hash_message_id(&id))),
        Command::Mcp => unreachable!("mcp is dispatched before run"),
    }
}

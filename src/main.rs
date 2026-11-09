mod addr;
mod cli;
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

use cli::{Cli, Command, ListDomain, PickDomain};

use clap::Parser;

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
            forward,
            forward_attach,
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
            forward,
            forward_attach,
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
            ListDomain::Msgs {
                folders,
                from,
                last,
                json,
            } => lookup::list_msgs(
                from.as_deref(),
                last.unwrap_or_else(|| cfg.msgs_last()),
                json,
                &cfg.resolve_folders(&folders)?,
                cfg,
            ),
            ListDomain::Addr {
                folders,
                last,
                json,
            } => lookup::list_addr(
                last.unwrap_or_else(|| cfg.addr_last()),
                json,
                &cfg.resolve_folders(&folders)?,
                cfg,
            ),
        },
        Command::Pick { domain } => match domain {
            PickDomain::Drafts {
                multi,
                lines,
                page_size,
            } => list::pick_drafts(
                multi,
                lines,
                page_size.unwrap_or_else(|| cfg.page_size()),
                cfg,
            ),
            PickDomain::Snapshots {
                multi,
                lines,
                page_size,
            } => list::pick_snapshots(
                multi,
                lines,
                page_size.unwrap_or_else(|| cfg.page_size()),
                cfg,
            ),
            PickDomain::Msgs {
                folders,
                from,
                last,
                multi,
                lines,
                page_size,
            } => lookup::pick_msgs(
                from.as_deref(),
                last.unwrap_or_else(|| cfg.msgs_last()),
                multi,
                lines,
                page_size.unwrap_or_else(|| cfg.page_size()),
                &cfg.resolve_folders(&folders)?,
                cfg,
            ),
            PickDomain::Addr {
                folders,
                last,
                multi,
                lines,
                page_size,
            } => lookup::pick_addr(
                last.unwrap_or_else(|| cfg.addr_last()),
                multi,
                lines,
                page_size.unwrap_or_else(|| cfg.page_size()),
                &cfg.resolve_folders(&folders)?,
                cfg,
            ),
        },
    }
}

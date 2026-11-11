mod addr;
mod cli;
mod compile;
mod config;
mod decompile;
mod draft;
mod frontmatter;
mod list;
mod maildir;
mod mml;
mod new;
mod notmuch;
mod quote;
mod reconcile;
mod reply;
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
        Command::Config => cfg.to_toml().map(|s| println!("{s}")),
        Command::View { input, body_only } => view::view(&input, body_only, cfg),
        Command::List { domain } => match domain {
            ListDomain::Drafts { json } => list::list_drafts(json, cfg),
            ListDomain::Snapshots { json } => list::list_snapshots(json, cfg),
        },
        Command::Pick { domain } => match domain {
            PickDomain::Drafts { multi, page_size } => {
                list::pick_drafts(multi, page_size.unwrap_or_else(|| cfg.page_size()), cfg)
            }
            PickDomain::Snapshots { multi, page_size } => {
                list::pick_snapshots(multi, page_size.unwrap_or_else(|| cfg.page_size()), cfg)
            }
        },
        Command::Schema { schema } => {
            let s = match schema {
                SchemaType::MailDraft => frontmatter::mail_draft_schema(),
                SchemaType::MailSnapshot => frontmatter::mail_snapshot_schema(),
            };
            println!("{s}");
            Ok(())
        }
        Command::Hash { id } => {
            println!("{}", util::hash_message_id(&id));
            Ok(())
        }
    }
}

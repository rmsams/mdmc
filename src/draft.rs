use std::path::{Path, PathBuf};

use mail_parser::{Message, MessageParser};

use crate::addr::addresses;
use crate::config::Config;
use crate::frontmatter::{self, Field};

/// Open a draft in an editor, resolving a slug, path, or Message-ID. When the
/// Message-ID has no markdown draft yet, it is imported from the account's
/// drafts maildir (decompiled to a `mail-draft` in the source directory). With
/// no input, pick a draft interactively. The editor is a `--editor` name, else
/// the first configured editor, else `$EDITOR`. Prints the draft path after the
/// editor closes, unless `--compile` is given, in which case it compiles the
/// draft back to the drafts maildir instead. Never sends.
pub fn draft(
    input: Option<&str>,
    editor: Option<&str>,
    compile: bool,
    cfg: &Config,
) -> Result<(), String> {
    let path = match input {
        Some(arg) => resolve_draft_or_create(arg, cfg)?,
        None => crate::list::pick_draft_path(cfg.page_size(), cfg)?,
    };
    let command = cfg.editor(editor)?;
    crate::util::run_editor_command(&command, &path);
    if compile {
        crate::compile::compile(false, None, false, &path.display().to_string(), cfg)?;
    } else {
        println!("{}", path.display());
    }
    Ok(())
}

/// Resolve an input to a draft path: an existing file (slug or path), a
/// Message-ID present in a source-dir draft, or a Message-ID present in one of
/// the accounts' drafts maildirs (imported on the fly).
fn resolve_draft_or_create(arg: &str, cfg: &Config) -> Result<PathBuf, String> {
    let id = arg.trim().trim_start_matches('<').trim_end_matches('>');
    let source_dir = cfg.paths_source()?;

    let mut path = crate::config::expand_home(arg);
    if !path.is_absolute() {
        path = source_dir.join(arg);
    }
    if path.extension().and_then(|e| e.to_str()) != Some("md") {
        path.set_extension("md");
    }
    if path.is_file() {
        return Ok(path);
    }

    if let Some(found) = find_by_message_id(&source_dir, id) {
        return Ok(found);
    }

    if let Some((account, message_path)) = find_in_drafts_maildirs(cfg, id) {
        return import_draft(id, &account, &message_path, cfg);
    }

    Err(format!("no draft found for '{arg}'"))
}

/// Search the source directory for a draft whose `message-id` matches.
fn find_by_message_id(source_dir: &Path, message_id: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(source_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let text = std::fs::read_to_string(&path).ok()?;
        if let (Some(fm), _, _) = frontmatter::load(&text) {
            if fm.message_id.as_deref() == Some(message_id) {
                return Some(path);
            }
        }
    }
    None
}

/// Find a message-id in the accounts' drafts maildirs (config order), returning
/// the account name and message path. Warns when the id is found in more than
/// one drafts folder.
fn find_in_drafts_maildirs(cfg: &Config, id: &str) -> Option<(String, PathBuf)> {
    let mut found: Option<(String, PathBuf)> = None;
    for (name, acct) in &cfg.accounts {
        let Some(drafts) = acct.drafts.as_deref() else {
            continue;
        };
        if let Some(path) = crate::maildir::find_message_in_maildir(Path::new(drafts), id) {
            if found.is_some() {
                eprintln!("warning: message-id '{id}' found in multiple drafts folders");
            } else {
                found = Some((name.clone(), path));
            }
        }
    }
    found
}

/// Decompile a message from a drafts maildir into a `mail-draft` note in the
/// source directory, returning the new file path.
fn import_draft(
    id: &str,
    account: &str,
    message_path: &Path,
    cfg: &Config,
) -> Result<PathBuf, String> {
    let raw = std::fs::read(message_path)
        .map_err(|e| format!("cannot read {}: {e}", message_path.display()))?;
    let message = MessageParser::new()
        .parse_owned(raw)
        .ok_or("cannot parse message: not a valid RFC 5322 message")?;

    let subject = message
        .subject()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("(no subject)");
    let (body, _html) = crate::decompile::render_body(&message, &[])?;
    let fields = build_draft_fields(&message, id, account);

    if let Err(errors) = frontmatter::validate(&frontmatter::to_json(&fields)) {
        return Err(errors.join("\n"));
    }

    let source_dir = cfg.paths_source()?;
    std::fs::create_dir_all(&source_dir)
        .map_err(|e| format!("cannot create {}: {e}", source_dir.display()))?;
    let slug = crate::new::slugify(subject);
    let mut path = source_dir.join(format!("{slug}.md"));
    let mut n = 2;
    while path.exists() {
        path = source_dir.join(format!("{slug}-{n}.md"));
        n += 1;
    }

    let note = frontmatter::render_note(&fields, body.trim_end());
    std::fs::write(&path, note).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(path)
}

/// Build the `mail-draft` frontmatter for a message imported from a drafts
/// maildir: `pending` (it is already compiled), the matched account, and the
/// message's subject and recipients.
fn build_draft_fields(message: &Message, message_id: &str, account: &str) -> Vec<(String, Field)> {
    let subject = message
        .subject()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("(no subject)");
    let mut fields = vec![
        ("title".to_string(), Field::Str(subject.to_string())),
        ("type".to_string(), Field::Str("mail-draft".to_string())),
        ("status".to_string(), Field::Str("pending".to_string())),
        ("account".to_string(), Field::Str(account.to_string())),
        ("format".to_string(), Field::Str("text".to_string())),
        ("subject".to_string(), Field::Str(subject.to_string())),
        ("message-id".to_string(), Field::Str(message_id.to_string())),
    ];
    if let Some(to) = message.to().filter(|l| !l.is_empty()) {
        fields.push(("to".to_string(), Field::List(addresses(&to))));
    }
    if let Some(cc) = message.cc().filter(|l| !l.is_empty()) {
        fields.push(("cc".to_string(), Field::List(addresses(&cc))));
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Account, Paths};
    use std::collections::BTreeMap;

    fn draft(path: &Path, slug: &str, message_id: Option<&str>, status: &str) {
        let mid = message_id
            .map(|m| format!("message-id: {m}\n"))
            .unwrap_or_default();
        std::fs::write(
            path.join(format!("{slug}.md")),
            format!("---\ntitle: {slug}\ntype: mail-draft\nstatus: {status}\n{mid}---\n\nbody\n"),
        )
        .unwrap();
    }

    #[test]
    fn resolve_draft_matches_slug_and_message_id() {
        let dir = std::env::temp_dir().join(format!("mdmc-draft-{}", uuid::Uuid::new_v4()));
        let source = dir.join("source");
        std::fs::create_dir_all(&source).unwrap();
        draft(&source, "hello", Some("abc@x"), "draft");

        let cfg = Config {
            paths: Paths {
                source: Some(source.to_string_lossy().into_owned()),
                ..Default::default()
            },
            ..Default::default()
        };

        assert_eq!(
            resolve_draft_or_create("hello", &cfg).unwrap(),
            source.join("hello.md")
        );
        assert_eq!(
            resolve_draft_or_create("abc@x", &cfg).unwrap(),
            source.join("hello.md")
        );
        assert!(resolve_draft_or_create("nope", &cfg).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_draft_imports_from_drafts_maildir() {
        let dir = std::env::temp_dir().join(format!("mdmc-draft-{}", uuid::Uuid::new_v4()));
        let source = dir.join("source");
        let drafts = dir.join("drafts");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(drafts.join("cur")).unwrap();
        std::fs::write(
            drafts.join("cur/m1"),
            concat!(
                "From: Me <me@example.com>\r\n",
                "To: Ann <ann@example.com>\r\n",
                "Subject: Hello World\r\n",
                "Message-ID: <abc@x>\r\n",
                "\r\n",
                "hi there\r\n",
            ),
        )
        .unwrap();

        let cfg = Config {
            paths: Paths {
                source: Some(source.to_string_lossy().into_owned()),
                ..Default::default()
            },
            accounts: BTreeMap::from([(
                "acct".to_string(),
                Account {
                    drafts: Some(drafts.to_string_lossy().into_owned()),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };

        let path = resolve_draft_or_create("abc@x", &cfg).unwrap();
        assert_eq!(path, source.join("hello-world.md"));

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("type: mail-draft"));
        assert!(text.contains("status: pending"));
        assert!(text.contains("message-id: abc@x"));
        assert!(text.contains("account: acct"));
        assert!(text.contains("hi there"));
        std::fs::remove_dir_all(&dir).ok();
    }
}

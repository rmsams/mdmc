use std::path::PathBuf;

use crate::config::Config;
use crate::frontmatter::{self, Field};

/// Split a comma-separated address list, respecting `"quoted names"` (which
/// may contain commas) and `<angle-bracketed addresses>`.
fn split_addresses(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut in_brackets = false;
    let mut escaped = false;
    for c in value.chars() {
        if escaped {
            current.push(c);
            escaped = false;
            continue;
        }
        if c == '\\' && in_quotes {
            escaped = true;
            current.push(c);
            continue;
        }
        match c {
            '"' if !in_brackets => {
                in_quotes = !in_quotes;
                current.push(c);
            }
            '<' if !in_quotes => {
                in_brackets = true;
                current.push(c);
            }
            '>' if !in_quotes => {
                in_brackets = false;
                current.push(c);
            }
            ',' if !in_quotes && !in_brackets => {
                let part = current.trim();
                if !part.is_empty() {
                    parts.push(part.to_string());
                }
                current.clear();
            }
            _ => current.push(c),
        }
    }
    let part = current.trim();
    if !part.is_empty() {
        parts.push(part.to_string());
    }
    parts
}

fn slugify(text: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for c in text.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "draft".to_string()
    } else {
        slug
    }
}

/// Create a new email draft. Never sends.
#[allow(clippy::too_many_arguments)]
pub fn new(
    cfg: &Config,
    account: Option<String>,
    to: Option<String>,
    cc: Option<String>,
    bcc: Option<String>,
    subject: Option<String>,
    in_reply_to: Option<String>,
    reply_all: bool,
    attachments: Vec<String>,
    body: Option<String>,
    format: Option<String>,
    css: Option<String>,
    template: Option<String>,
    lua_filter: Option<String>,
    pandoc_raw: bool,
    sign: bool,
    encrypt: bool,
    editor: bool,
) -> Result<(), String> {
    if let Some(fmt) = &format {
        if !matches!(fmt.as_str(), "text" | "html" | "plain") {
            return Err("--format must be one of text, html, plain.".into());
        }
    }
    if reply_all && in_reply_to.is_none() {
        return Err("--reply-all requires --in-reply-to.".into());
    }

    let drafts_dir = cfg.paths_source()?;

    let acct_name = account
        .clone()
        .or_else(|| cfg.default_account().map(String::from));
    let Some(acct_name) = acct_name else {
        return Err("no account specified and no default account configured.".into());
    };
    // Validate the account exists (signature/send identity are not needed here).
    let acct = cfg.resolve_account(&acct_name)?;
    let self_addr = acct.from.as_deref().unwrap_or("").to_string();

    let (reply_to, reply_cc, reply_subject) = match &in_reply_to {
        Some(irt) => {
            let mail_root = cfg.paths_mail()?;
            let message = crate::reply::resolve(irt, &mail_root)?;
            let env = crate::reply::envelope(&message, &self_addr, reply_all)?;
            (env.to, env.cc, env.subject)
        }
        None => (Vec::new(), Vec::new(), None),
    };

    let effective_subject = subject.clone().or(reply_subject);
    let effective_to = to.as_deref().map(split_addresses).unwrap_or(reply_to);
    let effective_cc = cc.as_deref().map(split_addresses).unwrap_or(reply_cc);

    let mut fields: Vec<(String, Field)> = vec![
        (
            "title".to_string(),
            Field::Str(
                effective_subject
                    .clone()
                    .unwrap_or_else(|| "(no subject)".to_string()),
            ),
        ),
        ("type".to_string(), Field::Str("mail-draft".to_string())),
        ("status".to_string(), Field::Str("draft".to_string())),
        ("account".to_string(), Field::Str(acct_name)),
        (
            "format".to_string(),
            Field::Str(format.unwrap_or_else(|| "text".to_string())),
        ),
    ];
    if let Some(subject) = &effective_subject {
        fields.push(("subject".to_string(), Field::Str(subject.clone())));
    }
    if !effective_to.is_empty() {
        fields.push(("to".to_string(), Field::List(effective_to)));
    }
    if !effective_cc.is_empty() {
        fields.push(("cc".to_string(), Field::List(effective_cc)));
    }
    if let Some(bcc) = &bcc {
        let list = split_addresses(bcc);
        if !list.is_empty() {
            fields.push(("bcc".to_string(), Field::List(list)));
        }
    }
    if let Some(in_reply_to) = &in_reply_to {
        fields.push(("in-reply-to".to_string(), Field::Str(in_reply_to.clone())));
        fields.push(("quote".to_string(), Field::Str("auto".to_string())));
    }
    if !attachments.is_empty() {
        fields.push(("attachments".to_string(), Field::List(attachments.clone())));
    }
    if let Some(css) = &css {
        fields.push(("css".to_string(), Field::Str(css.clone())));
    }
    if let Some(template) = &template {
        fields.push(("template".to_string(), Field::Str(template.clone())));
    }
    if let Some(lua_filter) = &lua_filter {
        fields.push(("lua-filter".to_string(), Field::Str(lua_filter.clone())));
    }
    if pandoc_raw {
        fields.push(("pandoc-raw".to_string(), Field::Bool(true)));
    }
    if sign {
        fields.push(("sign".to_string(), Field::Bool(true)));
    }
    if encrypt {
        fields.push(("encrypt".to_string(), Field::Bool(true)));
    }
    fields.push((
        "message-id".to_string(),
        Field::Str(crate::util::gen_message_id()),
    ));

    if let Err(errors) = frontmatter::validate(&frontmatter::to_json(&fields)) {
        return Err(errors.join("\n"));
    }

    let body_text = body.unwrap_or_default();
    let body_text = body_text.trim_end_matches('\n');

    let slug = slugify(effective_subject.as_deref().unwrap_or("draft"));
    let mut path: PathBuf = drafts_dir.join(format!("{slug}.md"));
    let mut n = 2;
    while path.exists() {
        path = drafts_dir.join(format!("{slug}-{n}.md"));
        n += 1;
    }

    std::fs::create_dir_all(&drafts_dir).map_err(|e| format!("cannot create drafts dir: {e}"))?;

    let note = frontmatter::render_note(&fields, body_text);
    std::fs::write(&path, note).map_err(|e| format!("cannot write draft: {e}"))?;

    if editor {
        if let Ok(command) = cfg.editor(None) {
            crate::util::run_editor_command(&command, &path);
        }
    }

    println!("draft: {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_lowercases_and_hyphenates() {
        assert_eq!(slugify("Re: CFSS-13158383"), "re-cfss-13158383");
        assert_eq!(slugify("Hello World!"), "hello-world");
        assert_eq!(slugify("  spaces  "), "spaces");
    }

    #[test]
    fn slugify_falls_back_to_draft() {
        assert_eq!(slugify(""), "draft");
        assert_eq!(slugify("---"), "draft");
    }

    #[test]
    fn split_addresses_handles_commas_and_whitespace() {
        assert_eq!(split_addresses("a@b.c, c@d.e"), vec!["a@b.c", "c@d.e"]);
        assert_eq!(split_addresses("  a@b.c  "), vec!["a@b.c"]);
        assert_eq!(split_addresses(""), Vec::<String>::new());
    }

    #[test]
    fn split_addresses_respects_quoted_names_and_brackets() {
        assert_eq!(
            split_addresses("\"Doe, John\" <john@x.com>, a@b.c"),
            vec!["\"Doe, John\" <john@x.com>", "a@b.c"]
        );
        assert_eq!(
            split_addresses("Joe Blogs <joe@x.com>, c@d.e"),
            vec!["Joe Blogs <joe@x.com>", "c@d.e"]
        );
        assert_eq!(
            split_addresses("\"He said \\\"hi\\\"\" <x@y.com>"),
            vec!["\"He said \\\"hi\\\"\" <x@y.com>"]
        );
    }

    fn setup_maildir() -> (std::path::PathBuf, Config, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("mdmc-new-{}", uuid::Uuid::new_v4()));
        let source = dir.join("drafts");
        let mail = dir.join("mail");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(mail.join("acct").join("cur")).unwrap();
        std::fs::write(
            mail.join("acct").join("cur").join("m1"),
            concat!(
                "From: Ann <ann@example.com>\r\n",
                "To: Bob <bob@example.com>, me@self.com\r\n",
                "Cc: Cy <cy@example.com>\r\n",
                "Subject: Lunch\r\n",
                "Message-ID: <m1@example.com>\r\n",
                "\r\n",
                "Body\r\n",
            ),
        )
        .unwrap();
        let cfg = Config {
            paths: crate::config::Paths {
                source: Some(source.to_string_lossy().into_owned()),
                mail: Some(mail.to_string_lossy().into_owned()),
                ..Default::default()
            },
            accounts: std::collections::BTreeMap::from([(
                "test".to_string(),
                crate::config::Account {
                    from: Some("me@self.com".to_string()),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        (dir, cfg, source)
    }

    fn run_new(
        cfg: &Config,
        in_reply_to: Option<&str>,
        reply_all: bool,
        to: Option<&str>,
        cc: Option<&str>,
        subject: Option<&str>,
    ) -> Result<(), String> {
        new(
            cfg,
            Some("test".to_string()),
            to.map(str::to_string),
            cc.map(str::to_string),
            None,
            subject.map(str::to_string),
            in_reply_to.map(str::to_string),
            reply_all,
            Vec::new(),
            None,
            None,
            None,
            None,
            None,
            false,
            false,
            false,
            false,
        )
    }

    #[test]
    fn new_reply_fills_envelope() {
        let (dir, cfg, source) = setup_maildir();
        run_new(&cfg, Some("m1@example.com"), false, None, None, None).unwrap();

        let text = std::fs::read_to_string(source.join("re-lunch.md")).unwrap();
        assert!(text.contains("subject: 'Re: Lunch'\n"));
        assert!(text.contains("to:\n  - Ann <ann@example.com>\n"));
        assert!(text.contains("in-reply-to: m1@example.com\n"));
        assert!(text.contains("quote: auto\n"));
        assert!(!text.contains("cc:"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn new_reply_all_fills_cc_and_excludes_self() {
        let (dir, cfg, source) = setup_maildir();
        run_new(&cfg, Some("m1@example.com"), true, None, None, None).unwrap();

        let text = std::fs::read_to_string(source.join("re-lunch.md")).unwrap();
        assert!(text.contains("to:\n  - Ann <ann@example.com>\n"));
        assert!(text.contains("cc:\n  - Bob <bob@example.com>\n  - Cy <cy@example.com>\n"));
        assert!(!text.contains("me@self.com"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn new_reply_respects_explicit_flags() {
        let (dir, cfg, source) = setup_maildir();
        run_new(
            &cfg,
            Some("m1@example.com"),
            true,
            Some("override@example.com"),
            Some("override-cc@example.com"),
            Some("My subject"),
        )
        .unwrap();

        let text = std::fs::read_to_string(source.join("my-subject.md")).unwrap();
        assert!(text.contains("subject: My subject\n"));
        assert!(text.contains("to:\n  - override@example.com\n"));
        assert!(text.contains("cc:\n  - override-cc@example.com\n"));
        assert!(!text.contains("Ann <ann@example.com>"));
        assert!(!text.contains("Bob <bob@example.com>"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn new_reply_errors_when_message_missing() {
        let (dir, cfg, _source) = setup_maildir();
        let err = run_new(&cfg, Some("missing@example.com"), false, None, None, None).unwrap_err();
        assert!(err.contains("not found"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn new_reply_all_requires_in_reply_to() {
        let (dir, cfg, source) = setup_maildir();
        let err = run_new(&cfg, None, true, None, None, None).unwrap_err();
        assert!(err.contains("--reply-all requires --in-reply-to"));
        assert!(!source.join("draft.md").exists());

        std::fs::remove_dir_all(&dir).ok();
    }
}

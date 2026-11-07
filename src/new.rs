use std::path::PathBuf;

use crate::config::Config;
use crate::frontmatter::{self, Field};

fn split_addresses(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect()
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

fn run_editor(path: &std::path::Path) {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());
    let parts: Vec<&str> = editor.split_whitespace().collect();
    if parts.is_empty() {
        return;
    }
    let _ = std::process::Command::new(parts[0])
        .args(&parts[1..])
        .arg(path)
        .status();
}

/// Create a new email draft note. Never sends.
#[allow(clippy::too_many_arguments)]
pub fn new(
    config_path: &std::path::Path,
    account: Option<String>,
    to: Option<String>,
    cc: Option<String>,
    bcc: Option<String>,
    subject: Option<String>,
    in_reply_to: Option<String>,
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

    let cfg = Config::load(config_path);
    let drafts_dir = cfg.paths_drafts();

    let acct_name = account
        .clone()
        .or_else(|| cfg.default_account().map(String::from));
    let Some(acct_name) = acct_name else {
        return Err("no account specified and no default account configured.".into());
    };
    // Validate the account exists (signature/send identity are not needed here).
    cfg.resolve_account(&acct_name)?;

    let mut fields: Vec<(String, Field)> = vec![
        (
            "title".to_string(),
            Field::Str(
                subject
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
    if let Some(subject) = &subject {
        fields.push(("subject".to_string(), Field::Str(subject.clone())));
    }
    if let Some(to) = &to {
        let list = split_addresses(to);
        if !list.is_empty() {
            fields.push(("to".to_string(), Field::List(list)));
        }
    }
    if let Some(cc) = &cc {
        let list = split_addresses(cc);
        if !list.is_empty() {
            fields.push(("cc".to_string(), Field::List(list)));
        }
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

    let slug = slugify(subject.as_deref().unwrap_or("draft"));
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
        run_editor(&path);
    }

    println!("draft: {}", path.display());
    Ok(())
}

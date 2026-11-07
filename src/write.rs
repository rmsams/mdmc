use std::io::Write;
use std::path::{Path, PathBuf};

use crate::account;
use crate::config::{self, Config};
use crate::frontmatter;
use crate::mml;
use crate::quote;

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "localhost".to_string())
}

fn gen_message_id() -> String {
    format!("{}@{}", uuid::Uuid::new_v4(), hostname())
}

fn list_draft_stems(drafts_dir: &Path) -> Vec<String> {
    let mut stems = Vec::new();
    if let Ok(entries) = std::fs::read_dir(drafts_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                stems.push(path.file_stem().unwrap().to_string_lossy().into_owned());
            }
        }
    }
    stems.sort();
    stems
}

fn inject_headers(message: &str, headers: &[String]) -> String {
    let mut out_lines: Vec<&str> = Vec::new();
    for line in message.split('\n') {
        let name = line
            .split_once(':')
            .map(|(k, _)| k.trim().to_lowercase())
            .unwrap_or_default();
        if name == "in-reply-to" || name == "references" {
            continue;
        }
        out_lines.push(line);
    }
    let mut inject_at = out_lines.len();
    for (i, line) in out_lines.iter().enumerate() {
        if line.is_empty() {
            inject_at = i;
            break;
        }
    }
    for h in headers {
        out_lines.insert(inject_at, h.as_str());
        inject_at += 1;
    }
    out_lines.join("\n")
}

fn inject_content_ids(message: &str, cids: &[String]) -> String {
    if cids.is_empty() {
        return message.to_string();
    }
    let mut out: Vec<String> = Vec::new();
    let mut idx = 0;
    for line in message.split('\n') {
        out.push(line.to_string());
        if line.trim().starts_with("Content-Disposition: inline") && idx < cids.len() {
            out.push(format!("Content-ID: <{}>", cids[idx]));
            idx += 1;
        }
    }
    out.join("\n")
}

/// Compile a draft note to a message. With `--stdout`, print the RFC 5322
/// message; otherwise deliver to a maildir (not yet implemented).
pub fn write(stdout: bool, folder: Option<String>, draft_arg: &str) -> Result<(), String> {
    if stdout && folder.is_some() {
        return Err("--stdout and --folder are mutually exclusive.".into());
    }
    if !stdout {
        return Err("maildir delivery is not yet implemented; use --stdout".into());
    }

    let cfg = Config::load(&config::expand_home("~/.config/mail/config.toml"));
    let drafts_dir = cfg.paths_drafts();
    let attachments_dir = cfg.paths_attachments();
    let neomutt_dir = cfg.neomutt_dir();

    let mut arg = config::expand_home(draft_arg);
    if !arg.is_absolute() {
        arg = drafts_dir.join(arg);
    }
    if arg.extension().and_then(|e| e.to_str()) != Some("md") {
        arg.set_extension("md");
    }
    if !arg.exists() {
        let available = list_draft_stems(&drafts_dir).join(", ");
        return Err(format!(
            "no draft at {}. Available: {}",
            arg.display(),
            if available.is_empty() {
                "(none)"
            } else {
                &available
            }
        ));
    }

    let text = std::fs::read_to_string(&arg).map_err(|e| format!("cannot read draft: {e}"))?;
    let (Some(fm), body) = frontmatter::load(&text) else {
        return Err(format!("{} has no valid frontmatter.", arg.display()));
    };

    let message_id = gen_message_id();

    let account_name = fm.account.clone();
    let to = fm.to.0.clone();
    let cc = fm.cc.0.clone();
    let bcc = fm.bcc.0.clone();
    let subject = fm.subject.as_deref().unwrap_or("").trim().to_string();

    let mut missing = Vec::new();
    if account_name.is_none() {
        missing.push("account");
    }
    if to.is_empty() && cc.is_empty() && bcc.is_empty() {
        missing.push("recipient (to/cc/bcc)");
    }
    if subject.is_empty() {
        missing.push("subject");
    }
    if !missing.is_empty() {
        return Err(missing.join(", "));
    }

    let acct = account::resolve_account(account_name.as_deref().unwrap(), &neomutt_dir)?;

    let in_reply_to = fm.in_reply_to.as_deref().unwrap_or("").trim().to_string();
    let quote = fm.quote.as_deref().unwrap_or("auto").trim();
    let mut body_text = body.trim_matches('\n').to_string();
    if !in_reply_to.is_empty() && quote == "auto" {
        let q = quote::build_quote(&in_reply_to);
        if !q.is_empty() {
            body_text.push_str(&q);
        }
    }

    let mut attachment_paths: Vec<PathBuf> = Vec::new();
    for raw in &fm.attachments {
        let p = config::expand_home(raw);
        let p = if p.is_absolute() {
            p
        } else {
            attachments_dir.join(p)
        };
        if p.exists() {
            attachment_paths.push(p);
        }
    }

    let from_hdr = if acct.realname.is_empty() {
        acct.from.clone()
    } else {
        format!("{} <{}>", acct.realname, acct.from)
    };
    let mut headers = vec![
        format!("From: {from_hdr}"),
        format!("To: {}", to.join(", ")),
    ];
    if !cc.is_empty() {
        headers.push(format!("Cc: {}", cc.join(", ")));
    }
    if !bcc.is_empty() {
        headers.push(format!("Bcc: {}", bcc.join(", ")));
    }
    headers.push(format!("Subject: {subject}"));
    headers.push(format!("Message-ID: <{message_id}>"));

    let (mml_body, inline_cids) =
        mml::build_mml_body(&fm, &body_text, &cfg, &attachment_paths, &attachments_dir)?;
    let mml_source = format!("{}\n\n{mml_body}\n", headers.join("\n"));

    let compiled = mml::compile_mml(&mml_source)?;

    let mut message = String::from_utf8_lossy(&compiled).into_owned();
    message = message.replace("\r\n", "\n");

    if !in_reply_to.is_empty() {
        message = inject_headers(
            &message,
            &[
                format!("In-Reply-To: <{in_reply_to}>"),
                format!("References: <{in_reply_to}>"),
            ],
        );
    }
    if !inline_cids.is_empty() {
        message = inject_content_ids(&message, &inline_cids);
    }

    let message = message.replace('\n', "\r\n");

    std::io::stdout()
        .write_all(message.as_bytes())
        .map_err(|e| format!("write failed: {e}"))
}

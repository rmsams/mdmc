use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::{self, Config};
use crate::frontmatter;
use crate::mml;
use crate::quote;
use crate::util;

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

/// Write a message to a maildir's `tmp/` then atomically move it to `new/`,
/// replacing any existing message with the same Message-ID.
fn maildir_deliver(dst: &Path, message: &str, message_id: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("cannot create {dst:?}: {e}"))?;
    let tmp = dst.join("tmp");
    let new = dst.join("new");
    std::fs::create_dir_all(&tmp).map_err(|e| format!("cannot create {tmp:?}: {e}"))?;
    std::fs::create_dir_all(&new).map_err(|e| format!("cannot create {new:?}: {e}"))?;

    // Locate any prior copy before writing, so a re-write replaces it.
    let existing = crate::maildir::find_message_in_maildir(dst, message_id);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let uniq = format!(
        "{}.M{}P{}.{}",
        now,
        std::process::id(),
        &uuid::Uuid::new_v4().simple().to_string()[..8],
        util::hostname()
    );
    let tmpf = tmp.join(&uniq);
    std::fs::write(&tmpf, message).map_err(|e| format!("cannot write {tmpf:?}: {e}"))?;
    let final_path = new.join(&uniq);
    std::fs::rename(&tmpf, &final_path)
        .map_err(|e| format!("cannot move {tmpf:?} to {final_path:?}: {e}"))?;

    // Remove the superseded copy (best-effort: the new one is already in place).
    if let Some(existing) = existing {
        let _ = std::fs::remove_file(existing);
    }
    Ok(final_path)
}

/// Extract the bare email address from an address string (`Name <addr>` → `addr`).
fn extract_email(addr: &str) -> &str {
    let addr = addr.trim();
    if let Some(start) = addr.find('<') {
        if let Some(end) = addr.find('>') {
            if end > start {
                return addr[start + 1..end].trim();
            }
        }
    }
    addr
}

fn recipient_emails(to: &[String], cc: &[String], bcc: &[String]) -> Vec<String> {
    to.iter()
        .chain(cc)
        .chain(bcc)
        .map(|a| extract_email(a).to_string())
        .collect()
}

fn compile_message(
    cfg: &Config,
    fm: &frontmatter::Draft,
    body_text: &str,
    message_id: &str,
    attachments_dir: &Path,
    note_dir: &Path,
    acct: &config::Account,
) -> Result<String, String> {
    let to = &fm.to.0;
    let cc = &fm.cc.0;
    let bcc = &fm.bcc.0;
    let subject = fm.subject.as_deref().unwrap_or("").trim();
    let in_reply_to = fm.in_reply_to.as_deref().unwrap_or("").trim();
    let from_hdr = acct.sender();
    let signing_key = acct.signing_key.as_deref();

    let mut attachment_paths: Vec<PathBuf> = Vec::new();
    for raw in &fm.attachments {
        let p = mml::resolve_reference(raw, note_dir, attachments_dir);
        if p.exists() {
            attachment_paths.push(p);
        }
    }

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

    let (mml_body, inline_cids) = mml::build_mml_body(
        fm,
        body_text,
        cfg,
        &attachment_paths,
        attachments_dir,
        note_dir,
    )?;
    let mml_source = format!("{}\n\n{mml_body}\n", headers.join("\n"));

    let sign_key = if fm.sign.unwrap_or(false) {
        Some(signing_key.ok_or("cannot sign: account has no signing_key configured.")?)
    } else {
        None
    };
    let encrypt_to = acct.encrypt_to.as_deref().or(signing_key);
    let recipients = recipient_emails(to, cc, bcc);
    let encrypt = if fm.encrypt.unwrap_or(false) {
        Some(mml::EncryptSpec {
            recipients: &recipients,
            encrypt_to,
        })
    } else {
        None
    };
    let compiled = mml::compile_mml(&mml_source, &inline_cids, sign_key, encrypt)?;

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

    Ok(message.replace('\n', "\r\n"))
}

/// Compile a draft to a message. With `--stdout`, print the RFC 5322
/// message; with `--folder`, deliver to a `[folders]` target; otherwise
/// deliver to the account's `drafts` maildir.
pub fn write(
    stdout: bool,
    folder: Option<String>,
    draft_arg: &str,
    cfg: &Config,
) -> Result<(), String> {
    if stdout && folder.is_some() {
        return Err("--stdout and --folder are mutually exclusive.".into());
    }

    let drafts_dir = cfg.paths_source();
    let attachments_dir = cfg.paths_attachments();

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
    let (Some(fm), fm_json, body) = frontmatter::load(&text) else {
        return Err(format!("{} has no valid frontmatter.", arg.display()));
    };
    if let Some(fm_json) = fm_json {
        if let Err(errors) = frontmatter::validate(&fm_json) {
            return Err(format!("invalid frontmatter:\n{}", errors.join("\n")));
        }
    }

    // Reuse the message-id assigned at `new` time; generate a fallback only
    // for drafts created without one (e.g. before message-id-at-new landed).
    let message_id = fm.message_id.clone().unwrap_or_else(util::gen_message_id);

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

    let acct = cfg.resolve_account(account_name.as_deref().unwrap())?;

    let in_reply_to = fm.in_reply_to.as_deref().unwrap_or("").trim().to_string();
    let quote = fm.quote.as_deref().unwrap_or("auto").trim();
    let mut body_text = body.trim_matches('\n').to_string();
    if !in_reply_to.is_empty() && quote == "auto" {
        let q = quote::build_quote(&in_reply_to);
        if !q.is_empty() {
            body_text.push_str(&q);
        }
    }

    let note_dir = arg.parent().unwrap_or_else(|| Path::new("."));
    let message = compile_message(
        cfg,
        &fm,
        &body_text,
        &message_id,
        &attachments_dir,
        note_dir,
        acct,
    )?;

    if stdout {
        std::io::stdout()
            .write_all(message.as_bytes())
            .map_err(|e| format!("write failed: {e}"))?;
        return Ok(());
    }

    // Resolve delivery target: --folder, else the account's drafts maildir.
    let target = if let Some(ref name) = folder {
        let t = cfg.resolve_target_folder(name);
        if !t.is_dir() {
            return Err(format!("folder does not exist: {}", t.display()));
        }
        if !t.join("new").is_dir() || !t.join("tmp").is_dir() {
            return Err(format!(
                "{} is not a maildir (missing new/ or tmp/).",
                t.display()
            ));
        }
        t
    } else {
        let drafts = acct.drafts.as_deref().ok_or_else(|| {
            format!(
                "account '{}' has no drafts folder configured.",
                account_name.as_deref().unwrap()
            )
        })?;
        PathBuf::from(drafts)
    };

    let dest = maildir_deliver(&target, &message, &message_id)?;

    if folder.is_some() {
        println!("Delivered to {}", dest.display());
        return Ok(());
    }

    let updated = frontmatter::set_fields(
        &text,
        &[("message-id", message_id.as_str()), ("status", "pending")],
    );
    std::fs::write(&arg, updated).map_err(|e| format!("cannot update draft: {e}"))?;

    println!("Draft compiled: {}", dest.display());
    println!("Message-ID: {message_id}");
    println!(
        "Account: {}  To: {}  Subject: {subject}",
        acct.from.as_deref().unwrap_or(""),
        to.join(", ")
    );
    println!("Open neomutt Drafts to review and send (press S).");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_email_handles_variants() {
        assert_eq!(
            extract_email("Abdul Azim <Abdul.Azim@x.com>"),
            "Abdul.Azim@x.com"
        );
        assert_eq!(extract_email("a@b.c"), "a@b.c");
        assert_eq!(extract_email("<a@b.c>"), "a@b.c");
    }

    #[test]
    fn recipient_emails_extracts_all_fields() {
        let to = vec!["a@b.c".to_string()];
        let cc = vec!["Name <c@d.e>".to_string()];
        let bcc = vec!["<f@g.h>".to_string()];
        assert_eq!(
            recipient_emails(&to, &cc, &bcc),
            vec!["a@b.c", "c@d.e", "f@g.h"]
        );
    }

    #[test]
    fn inject_headers_inserts_before_blank_line() {
        let msg = "From: a@b.c\nTo: c@d.e\nSubject: S\n\nBody\n";
        let out = inject_headers(msg, &["In-Reply-To: <x@y>".to_string()]);
        assert!(out.contains("Subject: S\nIn-Reply-To: <x@y>\n\nBody\n"));
    }

    #[test]
    fn inject_headers_replaces_existing() {
        let msg = "From: a@b.c\nIn-Reply-To: <old>\nSubject: S\n\nBody\n";
        let out = inject_headers(msg, &["In-Reply-To: <new>".to_string()]);
        assert!(!out.contains("old"));
        assert!(out.contains("In-Reply-To: <new>"));
    }

    #[test]
    fn maildir_deliver_overwrites_duplicate_message_id() {
        let dir = std::env::temp_dir().join(format!("mdmc-test-{}", uuid::Uuid::new_v4()));
        maildir_deliver(&dir, "Message-ID: <a@x>\n\nold body\n", "a@x").unwrap();
        maildir_deliver(&dir, "Message-ID: <a@x>\n\nnew body\n", "a@x").unwrap();

        let files: Vec<_> = std::fs::read_dir(dir.join("new"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(files.len(), 1, "re-write should leave a single message");
        let content = std::fs::read_to_string(files[0].path()).unwrap();
        assert!(content.contains("new body"));
        assert!(!content.contains("old body"));

        // a distinct message-id is not overwritten
        maildir_deliver(&dir, "Message-ID: <b@x>\n\nother\n", "b@x").unwrap();
        assert_eq!(std::fs::read_dir(dir.join("new")).unwrap().count(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }
}

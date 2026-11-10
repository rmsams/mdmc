use std::path::Path;

use crate::config::Config;
use crate::frontmatter;

/// Sync each draft's `status` with where its message actually lives, forward
/// only. Marks a draft `pending` when its message-id is in the account's
/// drafts maildir, and `sent` when it is in the sent maildir. Never downgrades:
/// `pending` is not regressed to `draft`, and `sent` is terminal. A message
/// found in both drafts and sent is marked `sent` with a warning about the
/// stale drafts copy. Never sends.
pub fn reconcile(cfg: &Config) -> Result<(), String> {
    let source_dir = cfg.paths_source()?;
    let Ok(entries) = std::fs::read_dir(&source_dir) else {
        return Ok(());
    };

    let mut sent: Vec<String> = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let (Some(fm), _, _) = frontmatter::load(&text) else {
            continue;
        };

        if fm.status.as_deref() == Some("sent") {
            continue;
        }
        let (Some(message_id), Some(account_name)) =
            (fm.message_id.as_deref(), fm.account.as_deref())
        else {
            continue;
        };
        let Ok(acct) = cfg.resolve_account(account_name) else {
            continue;
        };

        let slug = path.file_stem().unwrap().to_string_lossy().into_owned();

        // In the sent maildir → sent (warn when a stale drafts copy remains).
        if let Some(sent_dir) = acct.sent.as_deref() {
            if crate::maildir::find_message_in_maildir(Path::new(sent_dir), message_id).is_some() {
                if let Some(drafts_dir) = acct.drafts.as_deref() {
                    if crate::maildir::find_message_in_maildir(Path::new(drafts_dir), message_id)
                        .is_some()
                    {
                        eprintln!(
                            "warning: {slug} is in both drafts and sent maildirs (stale drafts copy)"
                        );
                    }
                }
                let updated = frontmatter::set_fields(&text, &[("status", "sent")]);
                std::fs::write(&path, updated).map_err(|e| format!("cannot update draft: {e}"))?;
                sent.push(slug);
                continue;
            }
        }

        // In the drafts maildir (not sent) → pending, but only forward.
        if fm.status.as_deref() != Some("pending") {
            if let Some(drafts_dir) = acct.drafts.as_deref() {
                if crate::maildir::find_message_in_maildir(Path::new(drafts_dir), message_id)
                    .is_some()
                {
                    let updated = frontmatter::set_fields(&text, &[("status", "pending")]);
                    std::fs::write(&path, updated)
                        .map_err(|e| format!("cannot update draft: {e}"))?;
                    pending.push(slug);
                }
            }
        }
    }

    sent.sort();
    pending.sort();
    if !sent.is_empty() {
        println!("marked sent:");
        for slug in &sent {
            println!("{slug}");
        }
    }
    if !pending.is_empty() {
        println!("marked pending:");
        for slug in &pending {
            println!("{slug}");
        }
    }
    println!(
        "{} draft(s) marked sent, {} marked pending",
        sent.len(),
        pending.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Account, Config, Paths};
    use std::collections::BTreeMap;

    fn setup() -> (std::path::PathBuf, Config) {
        let dir = std::env::temp_dir().join(format!("mdmc-recon-{}", uuid::Uuid::new_v4()));
        let source = dir.join("drafts");
        let sent = dir.join("sent");
        let drafts = dir.join("maildrafts");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(sent.join("new")).unwrap();
        std::fs::create_dir_all(sent.join("cur")).unwrap();
        std::fs::create_dir_all(drafts.join("new")).unwrap();
        std::fs::create_dir_all(drafts.join("cur")).unwrap();

        let cfg = Config {
            paths: Paths {
                source: Some(source.to_string_lossy().into_owned()),
                ..Default::default()
            },
            accounts: BTreeMap::from([(
                "rmsams-gmail".to_string(),
                Account {
                    sent: Some(sent.to_string_lossy().into_owned()),
                    drafts: Some(drafts.to_string_lossy().into_owned()),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        (dir, cfg)
    }

    fn write_note(cfg: &Config, slug: &str, status: &str, message_id: Option<&str>) {
        let msg = message_id
            .map(|m| format!("message-id: {m}\n"))
            .unwrap_or_default();
        let text = format!(
            "---\ntitle: {slug}\ntype: mail-draft\nstatus: {status}\naccount: rmsams-gmail\n{msg}---\n\nbody\n"
        );
        std::fs::write(cfg.paths_source().unwrap().join(format!("{slug}.md")), text).unwrap();
    }

    #[test]
    fn reconcile_marks_sent_when_message_in_sent_maildir() {
        let (dir, cfg) = setup();
        write_note(&cfg, "foo", "pending", Some("abc@x"));
        std::fs::write(
            dir.join("sent/new/m1"),
            "Message-ID: <abc@x>\n\nsent body\n",
        )
        .unwrap();

        reconcile(&cfg).unwrap();

        let text = std::fs::read_to_string(cfg.paths_source().unwrap().join("foo.md")).unwrap();
        assert!(text.contains("status: sent"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reconcile_marks_pending_when_message_in_drafts_maildir() {
        let (dir, cfg) = setup();
        write_note(&cfg, "foo", "draft", Some("abc@x"));
        std::fs::write(
            dir.join("maildrafts/new/m1"),
            "Message-ID: <abc@x>\n\ndraft body\n",
        )
        .unwrap();

        reconcile(&cfg).unwrap();

        let text = std::fs::read_to_string(cfg.paths_source().unwrap().join("foo.md")).unwrap();
        assert!(text.contains("status: pending"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reconcile_does_not_downgrade_pending_when_message_gone() {
        let (dir, cfg) = setup();
        write_note(&cfg, "foo", "pending", Some("abc@x"));

        reconcile(&cfg).unwrap();

        let text = std::fs::read_to_string(cfg.paths_source().unwrap().join("foo.md")).unwrap();
        assert!(text.contains("status: pending"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reconcile_marks_sent_when_in_both_drafts_and_sent() {
        let (dir, cfg) = setup();
        write_note(&cfg, "foo", "pending", Some("abc@x"));
        std::fs::write(
            dir.join("sent/new/m1"),
            "Message-ID: <abc@x>\n\nsent body\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("maildrafts/new/m1"),
            "Message-ID: <abc@x>\n\ndraft body\n",
        )
        .unwrap();

        reconcile(&cfg).unwrap();

        let text = std::fs::read_to_string(cfg.paths_source().unwrap().join("foo.md")).unwrap();
        assert!(text.contains("status: sent"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reconcile_skips_drafts_without_message_id() {
        let (dir, cfg) = setup();
        write_note(&cfg, "foo", "draft", None);

        reconcile(&cfg).unwrap();

        let text = std::fs::read_to_string(cfg.paths_source().unwrap().join("foo.md")).unwrap();
        assert!(text.contains("status: draft"));
        std::fs::remove_dir_all(&dir).ok();
    }
}

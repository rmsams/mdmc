use std::path::Path;

use crate::config::Config;
use crate::frontmatter;

/// Mark drafts as `sent` when their message-id is present in the account's
/// sent maildir. Never sends.
pub fn reconcile(cfg: &Config) -> Result<(), String> {
    let source_dir = cfg.paths_source()?;
    let Ok(entries) = std::fs::read_dir(&source_dir) else {
        return Ok(());
    };

    let mut sent = 0;
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
        let Some(sent_dir) = acct.sent.as_deref() else {
            continue;
        };

        if crate::maildir::find_message_in_maildir(Path::new(sent_dir), message_id).is_none() {
            continue;
        }

        let updated = frontmatter::set_fields(&text, &[("status", "sent")]);
        std::fs::write(&path, updated).map_err(|e| format!("cannot update draft: {e}"))?;
        sent += 1;
        println!("sent: {}", path.file_stem().unwrap().to_string_lossy());
    }

    println!("{sent} draft(s) marked sent");
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
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(sent.join("new")).unwrap();
        std::fs::create_dir_all(sent.join("cur")).unwrap();

        let cfg = Config {
            paths: Paths {
                source: Some(source.to_string_lossy().into_owned()),
                ..Default::default()
            },
            accounts: BTreeMap::from([(
                "rmsams-gmail".to_string(),
                Account {
                    sent: Some(sent.to_string_lossy().into_owned()),
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
    fn reconcile_leaves_unsent_untouched() {
        let (dir, cfg) = setup();
        write_note(&cfg, "foo", "pending", Some("abc@x"));

        reconcile(&cfg).unwrap();

        let text = std::fs::read_to_string(cfg.paths_source().unwrap().join("foo.md")).unwrap();
        assert!(text.contains("status: pending"));
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

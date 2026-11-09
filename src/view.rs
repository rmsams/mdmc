use mail_parser::MessageParser;

use crate::addr::{addresses, format_mailbox};
use crate::config::Config;
use crate::frontmatter;

/// Print a message as markdown to stdout. By default the output is a full
/// snapshot note (frontmatter + body); with `body_only`, only a subject/meta
/// header and the body are printed. Neither mode extracts attachments or
/// rewrites `cid:` images. Never sends.
pub fn view(input: &str, body_only: bool, cfg: &Config) -> Result<(), String> {
    print!("{}", render(input, body_only, cfg)?);
    Ok(())
}

fn render(input: &str, body_only: bool, cfg: &Config) -> Result<String, String> {
    let raw = crate::decompile::resolve_input(input, cfg)?;
    let message = MessageParser::new()
        .parse_owned(raw)
        .ok_or("cannot parse message: not a valid RFC 5322 message")?;

    let (body, _) = crate::decompile::render_body(&message, &[])?;

    if body_only {
        Ok(markdown_body(&message, &body))
    } else {
        let fields = crate::decompile::build_fields(&message, message.message_id(), &[]);
        Ok(frontmatter::render_note(&fields, body.trim_end()))
    }
}

fn markdown_body(message: &mail_parser::Message, body: &str) -> String {
    let subject = message
        .subject()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("(no subject)");

    let mut out = format!("# {subject}\n\n");
    let mut meta = Vec::new();
    if let Some(from) = message.from().and_then(|list| list.first()) {
        meta.push(format!("From: {}", format_mailbox(&from)));
    }
    if let Some(date) = message.date() {
        meta.push(format!("Date: {}", date.to_rfc822()));
    }
    if let Some(to) = message.to().filter(|l| !l.is_empty()) {
        meta.push(format!("To: {}", addresses(&to).join(", ")));
    }
    if let Some(cc) = message.cc().filter(|l| !l.is_empty()) {
        meta.push(format!("Cc: {}", addresses(&cc).join(", ")));
    }
    out.push_str(&meta.join("\n"));
    out.push_str("\n\n");
    out.push_str(body.trim());
    out.push('\n');

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_maildir() -> (std::path::PathBuf, Config) {
        let dir = std::env::temp_dir().join(format!("mdmc-view-{}", uuid::Uuid::new_v4()));
        let mail = dir.join("mail");
        std::fs::create_dir_all(mail.join("acct").join("cur")).unwrap();
        std::fs::write(
            mail.join("acct").join("cur").join("m1"),
            concat!(
                "From: Ann <ann@example.com>\r\n",
                "To: Bob <bob@example.com>\r\n",
                "Cc: Cy <cy@example.com>\r\n",
                "Subject: Lunch\r\n",
                "Message-ID: <m1@example.com>\r\n",
                "Date: Mon, 9 Nov 2026 09:32:15 +0000\r\n",
                "\r\n",
                "Hello body.\r\n",
            ),
        )
        .unwrap();
        let cfg = Config {
            paths: crate::config::Paths {
                mail: Some(mail.to_string_lossy().into_owned()),
                ..Default::default()
            },
            ..Default::default()
        };
        (dir, cfg)
    }

    #[test]
    fn view_default_includes_frontmatter() {
        let (dir, cfg) = setup_maildir();
        let out = render("m1@example.com", false, &cfg).unwrap();
        assert!(out.starts_with("---\n"));
        assert!(out.contains("title: Lunch\n"));
        assert!(out.contains("type: mail-snapshot\n"));
        assert!(out.contains("message-id: m1@example.com\n"));
        assert!(out.contains("from: Ann <ann@example.com>\n"));
        assert!(out.contains("Hello body."));
        assert!(!out.contains("attachments:"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn view_body_only_renders_header_and_body() {
        let (dir, cfg) = setup_maildir();
        let out = render("m1@example.com", true, &cfg).unwrap();
        assert!(out.starts_with("# Lunch\n\n"));
        assert!(out.contains("From: Ann <ann@example.com>\n"));
        assert!(out.contains("Date: Mon, 9 Nov 2026 09:32:15 +0000\n"));
        assert!(out.contains("To: Bob <bob@example.com>\n"));
        assert!(out.contains("Cc: Cy <cy@example.com>\n"));
        assert!(out.contains("Hello body."));
        assert!(!out.contains("---"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn view_errors_when_message_missing() {
        let dir = std::env::temp_dir().join(format!("mdmc-view-{}", uuid::Uuid::new_v4()));
        let mail = dir.join("mail");
        std::fs::create_dir_all(&mail).unwrap();
        let cfg = Config {
            paths: crate::config::Paths {
                mail: Some(mail.to_string_lossy().into_owned()),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(render("missing@example.com", false, &cfg)
            .unwrap_err()
            .contains("not found"));
        std::fs::remove_dir_all(&dir).ok();
    }
}

use std::path::Path;

use mail_parser::MessageParser;

use crate::addr::format_mailbox;

/// Build the reply quote for a message-id, or an empty string when the
/// message cannot be found. The message is resolved by a notmuch `mid:` lookup
/// with a maildir-scan fallback.
pub fn build_quote(in_reply_to: &str, mail_root: Option<&Path>) -> String {
    let Some(root) = mail_root else {
        return String::new();
    };
    let Some(path) = crate::notmuch::resolve(in_reply_to, Some(root)) else {
        return String::new();
    };
    let Ok(raw) = std::fs::read(path) else {
        return String::new();
    };
    let Some(message) = MessageParser::new().parse(&raw) else {
        return String::new();
    };

    let name = message
        .from()
        .and_then(|list| list.first())
        .map(|m| format_mailbox(&m))
        .unwrap_or_default();
    let date = message.date().map(|d| d.to_rfc822()).unwrap_or_default();

    let body = message
        .body_text(0)
        .map(|c| c.into_owned())
        .unwrap_or_default();
    let quoted = body
        .trim()
        .split('\n')
        .map(|l| {
            if l.is_empty() {
                ">".to_string()
            } else {
                format!("> {l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let who = if name.is_empty() {
        "wrote:".to_string()
    } else {
        format!("{name} wrote:")
    };
    let when = if date.is_empty() {
        String::new()
    } else {
        format!("On {date}, ")
    };
    format!("\n\n{when}{who}\n\n{quoted}")
}

/// Build the forwarded-message block for a message-id: a header block plus the
/// body, unquoted, or an empty string when the message cannot be found.
pub fn build_forward(message_id: &str, mail_root: Option<&Path>) -> String {
    let Some(root) = mail_root else {
        return String::new();
    };
    let Some(path) = crate::notmuch::resolve(message_id, Some(root)) else {
        return String::new();
    };
    let Ok(raw) = std::fs::read(path) else {
        return String::new();
    };
    let Some(message) = MessageParser::new().parse(&raw) else {
        return String::new();
    };

    let mut out = String::from("\n\n---------- Forwarded message ---------\n");
    if let Some(from) = message.from().and_then(|list| list.first()) {
        out.push_str(&format!("From: {}\n", format_mailbox(&from)));
    }
    if let Some(date) = message.date() {
        out.push_str(&format!("Date: {}\n", date.to_rfc822()));
    }
    if let Some(subject) = message.subject().map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str(&format!("Subject: {subject}\n"));
    }
    if let Some(to) = message.to().filter(|l| !l.is_empty()) {
        out.push_str(&format!("To: {}\n", crate::addr::addresses(&to).join(", ")));
    }
    let body = message
        .body_text(0)
        .map(|c| c.into_owned())
        .unwrap_or_default();
    out.push_str(&format!("\n{}", body.trim()));
    out
}

/// Resolve a message-id to its raw RFC 5322 bytes (notmuch `mid:` lookup with a
/// maildir-scan fallback), or `None` when it cannot be found.
pub fn resolve_raw(message_id: &str, mail_root: Option<&Path>) -> Option<Vec<u8>> {
    let root = mail_root?;
    let path = crate::notmuch::resolve(message_id, Some(root))?;
    std::fs::read(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mdmc-quote-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("acct").join("cur")).unwrap();
        dir
    }

    #[test]
    fn build_quote_resolves_from_maildir() {
        let dir = setup();
        std::fs::write(
            dir.join("acct").join("cur").join("m1"),
            concat!(
                "From: Ann <ann@example.com>\r\n",
                "To: Bob <bob@example.com>\r\n",
                "Subject: Re: Hi\r\n",
                "Message-ID: <m1@example.com>\r\n",
                "Date: Mon, 9 Nov 2026 09:32:15 +0000\r\n",
                "\r\n",
                "Line one.\r\n",
                "Line two.\r\n",
            ),
        )
        .unwrap();

        let quote = build_quote("m1@example.com", Some(&dir));
        assert!(quote.starts_with("\n\nOn "));
        assert!(quote.contains("Ann <ann@example.com> wrote:"));
        assert!(quote.contains("> Line one."));
        assert!(quote.contains("> Line two."));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn build_quote_empty_when_not_found() {
        let dir = setup();
        assert_eq!(build_quote("missing@example.com", Some(&dir)), "");
        assert_eq!(build_quote("m1@example.com", None), "");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn build_forward_embeds_headers_and_body_unquoted() {
        let dir = setup();
        std::fs::write(
            dir.join("acct").join("cur").join("m1"),
            concat!(
                "From: Ann <ann@example.com>\r\n",
                "To: Bob <bob@example.com>\r\n",
                "Subject: Report\r\n",
                "Message-ID: <m1@example.com>\r\n",
                "Date: Mon, 9 Nov 2026 09:32:15 +0000\r\n",
                "\r\n",
                "Line one.\r\n",
                "Line two.\r\n",
            ),
        )
        .unwrap();

        let fwd = build_forward("m1@example.com", Some(&dir));
        assert!(fwd.contains("---------- Forwarded message ---------"));
        assert!(fwd.contains("From: Ann <ann@example.com>"));
        assert!(fwd.contains("Subject: Report"));
        assert!(fwd.contains("To: Bob <bob@example.com>"));
        assert!(fwd.contains("Line one."));
        assert!(!fwd.contains("> Line one."));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn build_forward_empty_when_not_found() {
        let dir = setup();
        assert_eq!(build_forward("missing@example.com", Some(&dir)), "");
        assert_eq!(build_forward("m1@example.com", None), "");
        std::fs::remove_dir_all(&dir).ok();
    }
}

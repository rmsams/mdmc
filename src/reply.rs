use std::path::Path;

use mail_parser::{Mailbox, Message, MessageParser};

use crate::addr;

/// Resolve a Message-ID to a parsed message by scanning the maildir tree under
/// `mail_root`.
pub fn resolve(in_reply_to: &str, mail_root: &Path) -> Result<Message<'static>, String> {
    let id = in_reply_to
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>');
    let path = crate::maildir::find_message_in_tree(mail_root, id)
        .ok_or_else(|| format!("message-id '{id}' not found under {}", mail_root.display()))?;
    let raw = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    MessageParser::new()
        .parse_owned(raw)
        .ok_or_else(|| "cannot parse message: not a valid RFC 5322 message".to_string())
}

/// The reply envelope: `to`, `cc`, and a `Re:`-prefixed subject.
pub struct Envelope {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: Option<String>,
}

/// Compute the reply envelope from an original message. `self_addr` is the
/// sending account's address, excluded from a reply-all `cc`.
pub fn envelope(message: &Message, self_addr: &str, reply_all: bool) -> Result<Envelope, String> {
    let sender = message
        .reply_to()
        .and_then(|list| list.first())
        .or_else(|| message.from().and_then(|list| list.first()))
        .ok_or("cannot determine sender (no From or Reply-To header)")?;
    let sender_addr = sender.address().unwrap_or_default().to_string();

    let mut cc = Vec::new();
    if reply_all {
        let mut boxes: Vec<Mailbox> = Vec::new();
        if let Some(list) = message.to() {
            boxes.extend(list.mailboxes());
        }
        if let Some(list) = message.cc() {
            boxes.extend(list.mailboxes());
        }
        let mut seen = std::collections::BTreeSet::new();
        for mailbox in boxes {
            let a = mailbox.address().unwrap_or_default().to_string();
            if a.is_empty() || a == sender_addr || a == self_addr {
                continue;
            }
            if seen.insert(a) {
                cc.push(addr::format_mailbox(&mailbox));
            }
        }
    }

    Ok(Envelope {
        to: vec![addr::format_mailbox(&sender)],
        cc,
        subject: prefixed_subject(message),
    })
}

/// The original subject with a `Re:` prefix, unless it already has one.
fn prefixed_subject(message: &Message) -> Option<String> {
    let subject = message.subject().map(str::trim).filter(|s| !s.is_empty())?;
    if subject.to_lowercase().starts_with("re:") {
        Some(subject.to_string())
    } else {
        Some(format!("Re: {subject}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> Message<'static> {
        MessageParser::new()
            .parse_owned(raw.as_bytes().to_vec())
            .unwrap()
    }

    #[test]
    fn reply_targets_sender() {
        let message = parse(concat!(
            "From: Ann <ann@example.com>\r\n",
            "To: Bob <bob@example.com>, me@self.com\r\n",
            "Cc: Cy <cy@example.com>\r\n",
            "Subject: Lunch\r\n",
            "\r\n",
            "Body\r\n",
        ));
        let env = envelope(&message, "me@self.com", false).unwrap();
        assert_eq!(env.to, vec!["Ann <ann@example.com>"]);
        assert!(env.cc.is_empty());
        assert_eq!(env.subject.as_deref(), Some("Re: Lunch"));
    }

    #[test]
    fn reply_all_fills_cc_and_excludes_self_and_sender() {
        let message = parse(concat!(
            "From: Ann <ann@example.com>\r\n",
            "To: Bob <bob@example.com>, me@self.com\r\n",
            "Cc: Cy <cy@example.com>\r\n",
            "Subject: Lunch\r\n",
            "\r\n",
            "Body\r\n",
        ));
        let env = envelope(&message, "me@self.com", true).unwrap();
        assert_eq!(env.to, vec!["Ann <ann@example.com>"]);
        assert_eq!(env.cc, vec!["Bob <bob@example.com>", "Cy <cy@example.com>"]);
    }

    #[test]
    fn reply_prefers_reply_to_header() {
        let message = parse(concat!(
            "From: Ann <ann@example.com>\r\n",
            "Reply-To: Support <support@example.com>\r\n",
            "Subject: Hi\r\n",
            "\r\n",
            "Body\r\n",
        ));
        let env = envelope(&message, "me@self.com", false).unwrap();
        assert_eq!(env.to, vec!["Support <support@example.com>"]);
    }

    #[test]
    fn envelope_errors_without_sender() {
        let message = parse("Subject: No sender\r\n\r\nBody\r\n");
        assert!(envelope(&message, "me@self.com", false).is_err());
    }

    #[test]
    fn prefixed_subject_reuses_existing_re() {
        let message = parse("From: Ann <ann@example.com>\r\nSubject: Re: Lunch\r\n\r\nBody\r\n");
        let env = envelope(&message, "me@self.com", false).unwrap();
        assert_eq!(env.subject.as_deref(), Some("Re: Lunch"));
    }
}

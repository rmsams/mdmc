use mail_parser::{AddressList, Mailbox};

/// Format a mailbox as `Name <addr>`, the bare address, or the bare name.
pub fn format_mailbox(mailbox: &Mailbox) -> String {
    match (mailbox.name(), mailbox.address()) {
        (Some(name), Some(addr)) if !name.is_empty() => format!("{name} <{addr}>"),
        (_, Some(addr)) => addr.to_string(),
        (Some(name), None) => name.to_string(),
        (None, None) => String::new(),
    }
}

/// Format every mailbox in an address list, dropping empty entries.
pub fn addresses(list: &AddressList) -> Vec<String> {
    list.mailboxes()
        .map(|mailbox| format_mailbox(&mailbox))
        .filter(|s| !s.is_empty())
        .collect()
}

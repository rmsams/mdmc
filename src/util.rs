/// The local hostname, read from `/etc/hostname` with a `localhost` fallback.
pub fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "localhost".to_string())
}

/// Generate a Message-ID: a random UUID v4 plus the local hostname.
pub fn gen_message_id() -> String {
    format!("{}@{}", uuid::Uuid::new_v4(), hostname())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_id_is_uuid_at_hostname() {
        let id = gen_message_id();
        let (left, right) = id.split_once('@').unwrap();
        assert_eq!(left.len(), 36, "uuid v4 is 36 chars");
        assert!(uuid::Uuid::parse_str(left).is_ok());
        assert!(!right.is_empty());
    }
}

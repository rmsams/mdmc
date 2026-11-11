use sha2::{Digest, Sha256};

/// The snapshot-naming digest of a Message-ID: the first 16 bytes of its
/// SHA-256 (surrounding angle brackets stripped, no case folding), rendered as
/// 32 lowercase hex characters. This is the `msgid → path` key; `mdmc utils
/// hash` exposes it so external tooling can reproduce the mapping without
/// mdmc.
pub fn hash_message_id(id: &str) -> String {
    let normalized = id.trim().trim_start_matches('<').trim_end_matches('>');
    let digest = Sha256::digest(normalized.as_bytes());
    let mut out = String::with_capacity(32);
    for b in &digest[..16] {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

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

/// Run an editor command string on a file via `sh -c`, with the file path
/// available as `$1` (and `$@`). The command must reference `$1`/`$@` where the
/// path belongs — e.g. `emacsclient -t "$@"` or `obs open path="$1"`.
pub fn run_editor_command(command: &str, path: &std::path::Path) {
    let _ = std::process::Command::new("sh")
        .args(["-c", command, "mdmc-editor"])
        .arg(path)
        .status();
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

    #[test]
    fn hash_message_id_is_known_value() {
        assert_eq!(hash_message_id("abc@x"), "b74912a7d942c1e77f964f045d7459c2");
    }

    #[test]
    fn hash_message_id_strips_angle_brackets() {
        assert_eq!(hash_message_id("<abc@x>"), hash_message_id("abc@x"));
        assert_eq!(hash_message_id("  abc@x  "), hash_message_id("abc@x"));
    }

    #[test]
    fn hash_message_id_is_32_lowercase_hex() {
        let h = hash_message_id("anything@example.com");
        assert_eq!(h.len(), 32);
        assert!(h
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }
}

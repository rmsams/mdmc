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
/// path belongs — e.g. `emacsclient -t "$@"` or `obs open path="$1"`. The
/// editor's stdio is redirected to `/dev/tty` so it stays interactive even when
/// mdmc's own stdout is captured, falling back to inheritance when there is no
/// controlling terminal.
pub fn run_editor_command(command: &str, path: &std::path::Path) {
    use std::process::Stdio;
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", command, "mdmc-editor"]).arg(path);
    if let Ok(tty) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
    {
        if let Ok(t) = tty.try_clone() {
            cmd.stdin(Stdio::from(t));
        }
        if let Ok(t) = tty.try_clone() {
            cmd.stdout(Stdio::from(t));
        }
        if let Ok(t) = tty.try_clone() {
            cmd.stderr(Stdio::from(t));
        }
    }
    let _ = cmd.status();
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

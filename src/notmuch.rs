use std::path::{Path, PathBuf};

/// The notmuch binary name.
pub(crate) fn binary() -> &'static str {
    "notmuch"
}

/// Resolve a Message-ID (angle brackets optional) to a file path via notmuch's
/// `mid:` term. Returns `None` when notmuch is unavailable, fails, or the id is
/// not indexed.
pub fn resolve_message_id(id: &str) -> Option<PathBuf> {
    resolve_message_id_with(binary(), id)
}

fn resolve_message_id_with(bin: &str, id: &str) -> Option<PathBuf> {
    let out = std::process::Command::new(bin)
        .args([
            "search",
            "--output=files",
            "--limit=1",
            &format!("mid:{id}"),
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
}

/// Resolve a Message-ID, falling back to a full maildir scan when notmuch has
/// no answer.
pub fn resolve(id: &str, mail_root: Option<&Path>) -> Option<PathBuf> {
    resolve_message_id(id)
        .or_else(|| mail_root.and_then(|root| crate::maildir::find_message_in_tree(root, id)))
}

/// The file paths matching a notmuch query (for the index build), streamed
/// from notmuch's stdout.
pub(crate) fn search_files_with(bin: &str, query: &str) -> Result<Vec<PathBuf>, String> {
    let mut child = std::process::Command::new(bin)
        .args(["search", "--output=files", query])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("notmuch failed to start: {e}"))?;

    let mut paths = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        use std::io::BufRead;
        for line in std::io::BufReader::new(stdout).lines() {
            let line = line.map_err(|e| format!("cannot read notmuch output: {e}"))?;
            if !line.is_empty() {
                paths.push(PathBuf::from(line));
            }
        }
    }
    let status = child
        .wait()
        .map_err(|e| format!("notmuch wait failed: {e}"))?;
    if !status.success() {
        return Err("notmuch search failed".to_string());
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake notmuch that answers `search --output=files --limit=1 mid:X`.
    #[test]
    fn resolve_message_id_parses_first_path() {
        let dir = std::env::temp_dir().join(format!("mdmc-notmuch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("notmuch");
        std::fs::write(
            &script,
            "#!/bin/sh\nif [ \"$1\" = search ] && [ \"$2\" = --output=files ]; then echo /mail/cur/msg1; fi\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        assert_eq!(
            resolve_message_id_with(script.to_str().unwrap(), "abc@x"),
            Some(PathBuf::from("/mail/cur/msg1"))
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

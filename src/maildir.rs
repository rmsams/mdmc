use std::path::{Path, PathBuf};

/// Read a file's `Message-ID` header value (angle brackets stripped), reading
/// only the header block prefix.
pub fn read_message_id(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = [0u8; 65536];
    let n = file.read(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf[..n]);
    for line in text.lines() {
        if line.is_empty() {
            return None;
        }
        if let Some((name, val)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("message-id") {
                return Some(
                    val.trim()
                        .trim_start_matches('<')
                        .trim_end_matches('>')
                        .to_string(),
                );
            }
        }
    }
    None
}

/// Find a message with the given Message-ID in a maildir's `new/` and `cur/`.
pub fn find_message_in_maildir(dst: &Path, message_id: &str) -> Option<PathBuf> {
    for sub in ["new", "cur"] {
        let dir = dst.join(sub);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && read_message_id(&path).as_deref() == Some(message_id) {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// Find a message with the given Message-ID anywhere under a maildir tree,
/// walking subdirectories for maildir `cur`/`new` folders.
pub fn find_message_in_tree(root: &Path, message_id: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut subdirs = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = path.file_name()?.to_string_lossy();
            if name == "cur" {
                if let Some(found) = find_message_in_maildir(&dir, message_id) {
                    return Some(found);
                }
            } else if name != "new" {
                subdirs.push(path);
            }
        }
        stack.extend(subdirs);
    }
    None
}

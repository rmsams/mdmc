use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Read a file's `Message-ID` header value (angle brackets stripped), reading
/// only the header block — up to the first blank line — so a large message
/// body is never read.
pub fn read_message_id(path: &Path) -> Option<String> {
    use std::io::{BufRead, BufReader};
    let file = std::fs::File::open(path).ok()?;
    for raw in BufReader::new(file).lines() {
        let line = raw.ok()?;
        if line.trim_end_matches('\r').is_empty() {
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

/// Find a message with the given Message-ID anywhere under a maildir tree.
/// All maildir folders (directories holding a `cur/` subdirectory) are scanned
/// in parallel across the available cores.
pub fn find_message_in_tree(root: &Path, message_id: &str) -> Option<PathBuf> {
    let folders = collect_maildir_folders(root);
    let found = AtomicBool::new(false);
    let result: Mutex<Option<PathBuf>> = Mutex::new(None);

    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(1);
    let chunk = folders.len().div_ceil(threads).max(1);

    std::thread::scope(|scope| {
        for group in folders.chunks(chunk) {
            let found = &found;
            let result = &result;
            scope.spawn(move || {
                for dst in group {
                    if found.load(Ordering::Relaxed) {
                        return;
                    }
                    if let Some(path) = find_message_in_maildir(dst, message_id) {
                        let mut guard = result.lock().unwrap();
                        if guard.is_none() {
                            *guard = Some(path);
                            found.store(true, Ordering::Relaxed);
                        }
                        return;
                    }
                }
            });
        }
    });

    result.into_inner().unwrap()
}

/// Collect every directory under `root` that contains a `cur/` subdirectory
/// (a maildir folder), walking subdirectories but not descending into `cur/`
/// or `new/`.
fn collect_maildir_folders(root: &Path) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut subdirs = Vec::new();
        let mut has_cur = false;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default();
            if name == "cur" {
                has_cur = true;
            } else if name != "new" {
                subdirs.push(path);
            }
        }
        if has_cur {
            folders.push(dir);
        }
        stack.extend(subdirs);
    }
    folders
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, raw: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, raw).unwrap();
    }

    #[test]
    fn read_message_id_reads_header_only() {
        let dir = std::env::temp_dir().join(format!("mdmc-maildir-{}", uuid::Uuid::new_v4()));
        let path = dir.join("m1");
        write(
            &path,
            "From: a@b.c\r\nSubject: hi\r\nMessage-ID: <abc@x>\r\n\r\nbody with \n no id\n",
        );
        assert_eq!(read_message_id(&path).as_deref(), Some("abc@x"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_message_id_missing_when_absent() {
        let dir = std::env::temp_dir().join(format!("mdmc-maildir-{}", uuid::Uuid::new_v4()));
        let path = dir.join("m1");
        write(&path, "From: a@b.c\r\nSubject: hi\r\n\r\nbody\n");
        assert_eq!(read_message_id(&path), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn find_message_in_tree_finds_nested_message() {
        let dir = std::env::temp_dir().join(format!("mdmc-maildir-{}", uuid::Uuid::new_v4()));
        write(
            &dir.join("acct").join("a").join("cur").join("m1"),
            "Message-ID: <one@x>\r\n\r\nbody\n",
        );
        write(
            &dir.join("acct").join("b").join("cur").join("m2"),
            "Message-ID: <two@x>\r\n\r\nbody\n",
        );

        assert_eq!(
            find_message_in_tree(&dir, "two@x"),
            Some(dir.join("acct").join("b").join("cur").join("m2"))
        );
        assert_eq!(find_message_in_tree(&dir, "nope@x"), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}

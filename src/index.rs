use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

pub const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS messages (
  message_id TEXT PRIMARY KEY,
  from_name  TEXT NOT NULL DEFAULT '',
  from_addr  TEXT NOT NULL DEFAULT '',
  subject    TEXT NOT NULL DEFAULT '',
  date       INTEGER NOT NULL DEFAULT 0,
  path       TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_messages_date ON messages(date);
";

struct Row {
    message_id: String,
    from_name: String,
    from_addr: String,
    subject: String,
    date: i64,
    path: String,
}

/// Build the index: run the notmuch `query` to get the scoped file list, read
/// each message's header block, and store rows in `db`. Returns the row count.
pub fn build(query: &str, db: &Path) -> Result<usize, String> {
    build_with(query, db, crate::notmuch::binary())
}

fn build_with(query: &str, db: &Path, bin: &str) -> Result<usize, String> {
    let files = crate::notmuch::search_files_with(bin, query)?;

    let rows: Mutex<Vec<Row>> = Mutex::new(Vec::with_capacity(files.len()));
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(1);
    let chunk = files.len().div_ceil(threads).max(1);

    std::thread::scope(|scope| {
        for group in files.chunks(chunk) {
            let rows = &rows;
            scope.spawn(move || {
                let mut local = Vec::with_capacity(group.len());
                for path in group {
                    if let Some(row) = read_row(path) {
                        local.push(row);
                    }
                }
                rows.lock().unwrap().extend(local);
            });
        }
    });

    let rows = rows.into_inner().unwrap();
    write_rows(db, &rows)?;
    Ok(rows.len())
}

/// Parse a message's header block into an index row.
fn read_row(path: &Path) -> Option<Row> {
    let header = read_header(path).ok()?;
    let message = mail_parser::MessageParser::new().parse(header.as_bytes())?;
    let message_id = message
        .message_id()
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let (from_name, from_addr) = message
        .from()
        .and_then(|list| list.first())
        .map(|m| {
            (
                m.name().map(str::trim).unwrap_or("").to_string(),
                m.address().map(str::trim).unwrap_or("").to_string(),
            )
        })
        .unwrap_or_default();
    let subject = message.subject().map(str::trim).unwrap_or("").to_string();
    let date = message.date().map(|d| d.to_timestamp()).unwrap_or(0);
    Some(Row {
        message_id,
        from_name,
        from_addr,
        subject,
        date,
        path: path.to_string_lossy().into_owned(),
    })
}

/// Read only the header block (through the first blank line) of a message.
fn read_header(path: &Path) -> std::io::Result<String> {
    use std::io::BufRead;
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    let mut header = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        header.push_str(&line);
        if line == "\r\n" || line == "\n" {
            break;
        }
    }
    Ok(header)
}

fn open(db: &Path) -> Result<Connection, String> {
    if let Some(parent) = db.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let conn =
        Connection::open(db).map_err(|e| format!("cannot open index {}: {e}", db.display()))?;
    conn.execute_batch(SCHEMA)
        .map_err(|e| format!("cannot init index schema: {e}"))?;
    Ok(conn)
}

fn write_rows(db: &Path, rows: &[Row]) -> Result<(), String> {
    let mut conn = open(db)?;
    let tx = conn
        .transaction()
        .map_err(|e| format!("cannot begin transaction: {e}"))?;
    tx.execute_batch("DELETE FROM messages")
        .map_err(|e| format!("cannot clear index: {e}"))?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT OR REPLACE INTO messages (message_id, from_name, from_addr, subject, date, path)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(|e| format!("cannot prepare insert: {e}"))?;
        for row in rows {
            stmt.execute(rusqlite::params![
                row.message_id,
                row.from_name,
                row.from_addr,
                row.subject,
                row.date,
                row.path,
            ])
            .map_err(|e| format!("cannot insert {}: {e}", row.message_id))?;
        }
    }
    tx.commit()
        .map_err(|e| format!("cannot commit index: {e}"))?;
    Ok(())
}

/// `(address, name)` pairs ordered by most recent, deduplicated by address.
pub fn addresses(db: &Path, since: i64) -> Result<Vec<(String, String)>, String> {
    let conn = open(db)?;
    let mut stmt = conn
        .prepare(
            "SELECT from_addr, from_name, MAX(date) AS last_seen
             FROM messages
             WHERE date >= ?1 AND from_addr != ''
             GROUP BY from_addr
             ORDER BY last_seen DESC",
        )
        .map_err(|e| format!("cannot prepare address query: {e}"))?;
    let rows = stmt
        .query_map([since], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| format!("cannot query addresses: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("cannot query addresses: {e}"))?;
    Ok(rows)
}

/// A row of the message lookup index.
pub struct MessageRow {
    pub message_id: String,
    pub from_name: String,
    pub from_addr: String,
    pub subject: String,
    pub date: i64,
}

/// Messages ordered by most recent, optionally filtered by a `from` substring
/// matched against name or address.
pub fn messages(db: &Path, since: i64, from: Option<&str>) -> Result<Vec<MessageRow>, String> {
    let conn = open(db)?;
    let mut stmt = conn
        .prepare(
            "SELECT message_id, from_name, from_addr, subject, date
             FROM messages
             WHERE date >= ?1
               AND (?2 IS NULL OR instr(lower(from_name), lower(?2)) > 0
                    OR instr(lower(from_addr), lower(?2)) > 0)
             ORDER BY date DESC",
        )
        .map_err(|e| format!("cannot prepare message query: {e}"))?;
    let rows = stmt
        .query_map(rusqlite::params![since, from], |r| {
            Ok(MessageRow {
                message_id: r.get(0)?,
                from_name: r.get(1)?,
                from_addr: r.get(2)?,
                subject: r.get(3)?,
                date: r.get(4)?,
            })
        })
        .map_err(|e| format!("cannot query messages: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("cannot query messages: {e}"))?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_db() -> (PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("mdmc-index-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        (dir.join("index.db"), dir)
    }

    #[test]
    fn read_header_stops_at_blank_line() {
        let dir = std::env::temp_dir().join(format!("mdmc-header-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m1");
        std::fs::write(
            &path,
            "From: a@b.c\r\nSubject: hi\r\n\r\nbody with\n\nmore\n",
        )
        .unwrap();
        let header = read_header(&path).unwrap();
        assert!(header.contains("From: a@b.c"));
        assert!(header.contains("Subject: hi"));
        assert!(!header.contains("body"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn build_and_query_roundtrip() {
        let (db, dir) = temp_db();

        // Write two fake messages and a stub notmuch that lists them.
        let mail = dir.join("mail");
        std::fs::create_dir_all(mail.join("cur")).unwrap();
        std::fs::write(
            mail.join("cur").join("m1"),
            concat!(
                "From: Ann <ann@example.com>\r\n",
                "Subject: First\r\n",
                "Message-ID: <one@x>\r\n",
                "Date: Mon, 9 Nov 2026 09:00:00 +0000\r\n",
                "\r\n",
                "body\r\n",
            ),
        )
        .unwrap();
        std::fs::write(
            mail.join("cur").join("m2"),
            concat!(
                "From: Bob <bob@example.com>\r\n",
                "Subject: Second\r\n",
                "Message-ID: <two@x>\r\n",
                "Date: Tue, 10 Nov 2026 09:00:00 +0000\r\n",
                "\r\n",
                "body\r\n",
            ),
        )
        .unwrap();
        let script = dir.join("notmuch");
        std::fs::write(
            &script,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' '{}' '{}'\n",
                mail.join("cur").join("m1").display(),
                mail.join("cur").join("m2").display(),
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let n = build_with("*", &db, script.to_str().unwrap()).unwrap();
        assert_eq!(n, 2);

        let addrs = addresses(&db, 0).unwrap();
        assert_eq!(addrs.len(), 2);
        assert_eq!(addrs[0], ("bob@example.com".to_string(), "Bob".to_string()));

        let msgs = messages(&db, 0, None).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].message_id, "two@x");

        let filtered = messages(&db, 0, Some("ann")).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].message_id, "one@x");

        std::fs::remove_dir_all(&dir).ok();
    }
}

use std::fmt;
use std::io::Write;
use std::path::Path;

use crate::config::Config;

/// An address row, rendered as `Name <email>` (or the bare address).
struct Addr {
    name: String,
    address: String,
}

impl Addr {
    fn name_addr(&self) -> String {
        if self.name.is_empty() {
            self.address.clone()
        } else {
            format!("{} <{}>", self.name, self.address)
        }
    }
}

impl fmt::Display for Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name_addr())
    }
}

/// Maximum rendered width of the sender and subject columns; longer values are
/// truncated with an ellipsis so the picker rows stay a manageable width.
const MAX_SENDER: usize = 32;
const MAX_SUBJECT: usize = 60;

/// A message row, rendered as three aligned columns: `sender`, `datetime`, and
/// `subject`.
struct Msg {
    id: String,
    from_name: String,
    from_addr: String,
    sender: String,
    datetime: String,
    subject: String,
    sender_width: usize,
}

impl fmt::Display for Msg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let subject = if self.subject.is_empty() {
            "(no subject)"
        } else {
            &self.subject
        };
        write!(
            f,
            "{:<sw$}  {:<16}  {}",
            truncate(&self.sender, MAX_SENDER),
            self.datetime,
            truncate(subject, MAX_SUBJECT),
            sw = self.sender_width
        )
    }
}

/// Truncate a string to at most `max` characters, appending an ellipsis when
/// it is longer.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let head: String = s.chars().take(max - 1).collect();
    format!("{head}…")
}

/// Format a sender as `Name <email>`, the bare address, or the bare name.
fn format_sender(name: &str, addr: &str) -> String {
    if name.is_empty() {
        addr.to_string()
    } else if addr.is_empty() {
        name.to_string()
    } else {
        format!("{name} <{addr}>")
    }
}

/// Format a unix timestamp as local `YYYY-MM-DD HH:MM`, or empty when absent.
fn format_datetime(ts: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

/// Build message rows, sizing the sender column to the longest sender so the
/// picker aligns the three columns.
fn build_msgs(rows: Vec<crate::index::MessageRow>) -> Vec<Msg> {
    let mut msgs: Vec<Msg> = rows
        .into_iter()
        .map(|r| {
            let sender = format_sender(&r.from_name, &r.from_addr);
            Msg {
                id: r.message_id,
                from_name: r.from_name,
                from_addr: r.from_addr,
                sender,
                datetime: format_datetime(r.date),
                subject: r.subject,
                sender_width: 0,
            }
        })
        .collect();
    let width = msgs
        .iter()
        .map(|m| m.sender.chars().count())
        .max()
        .unwrap_or(0)
        .min(MAX_SENDER);
    for m in &mut msgs {
        m.sender_width = width;
    }
    msgs
}

/// The lower bound (unix epoch seconds) for `--last N` days, or `0` when
/// `last == 0` (no bound).
fn since(last: u32) -> i64 {
    if last == 0 {
        return 0;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    now - (last as i64) * 86400
}

fn require_index(db: &Path) -> Result<(), String> {
    if !db.is_file() {
        return Err("no index — run `mdmc index` first".to_string());
    }
    Ok(())
}

pub(crate) fn pick_one<T: fmt::Display>(prompt: &str, options: Vec<T>) -> Result<Vec<T>, String> {
    if options.is_empty() {
        return Err("no matches".to_string());
    }
    let selected = inquire::Select::new(prompt, options)
        .raw_prompt()
        .map(|op| op.value)
        .map_err(|e| format!("picker failed: {e}"))?;
    Ok(vec![selected])
}

pub(crate) fn pick_many<T: fmt::Display>(prompt: &str, options: Vec<T>) -> Result<Vec<T>, String> {
    if options.is_empty() {
        return Err("no matches".to_string());
    }
    let selected = inquire::MultiSelect::new(prompt, options)
        .raw_prompt()
        .map(|ops| ops.into_iter().map(|op| op.value).collect())
        .map_err(|e| format!("picker failed: {e}"))?;
    Ok(selected)
}

pub(crate) fn print_values(values: impl Iterator<Item = String>) -> Result<(), String> {
    let mut out = std::io::stdout().lock();
    for value in values {
        writeln!(out, "{value}").map_err(|e| format!("write failed: {e}"))?;
    }
    Ok(())
}

/// Print values comma-joined on one line by default (to compose with
/// `--to="$(mdmc pick addr --multi)"`), or one per line with `--lines`.
pub(crate) fn emit_values(values: Vec<String>, lines: bool) -> Result<(), String> {
    if values.is_empty() {
        return Ok(());
    }
    if lines {
        return print_values(values.into_iter());
    }
    println!("{}", values.join(", "));
    Ok(())
}

/// Load address rows from the index, most recent first.
fn load_addrs(last: u32, cfg: &Config) -> Result<Vec<Addr>, String> {
    let db = cfg.paths_index();
    require_index(&db)?;
    let rows = crate::index::addresses(&db, since(last))?;
    Ok(rows
        .into_iter()
        .map(|(address, name)| Addr { name, address })
        .collect())
}

/// Load message rows from the index, most recent first.
fn load_msgs(from: Option<&str>, last: u32, cfg: &Config) -> Result<Vec<Msg>, String> {
    let db = cfg.paths_index();
    require_index(&db)?;
    let rows = crate::index::messages(&db, since(last), from)?;
    Ok(build_msgs(rows))
}

/// `pick addr`: pick an address and print `Name <email>`.
pub fn pick_addr(last: u32, multi: bool, lines: bool, cfg: &Config) -> Result<(), String> {
    let addrs = load_addrs(last, cfg)?;
    let chosen = if multi {
        pick_many("address", addrs)?
    } else {
        pick_one("address", addrs)?
    };
    emit_values(chosen.into_iter().map(|a| a.name_addr()).collect(), lines)
}

/// `list addr`: print addresses (`Name <email>`), or a JSON array with `--json`.
pub fn list_addr(last: u32, json: bool, cfg: &Config) -> Result<(), String> {
    let addrs = load_addrs(last, cfg)?;
    if json {
        let out: Vec<serde_json::Value> = addrs
            .iter()
            .map(|a| serde_json::json!({"name": a.name, "address": a.address}))
            .collect();
        return println_json(&out);
    }
    print_values(addrs.iter().map(|a| a.name_addr()))
}

/// `pick msgs`: pick a message and print its message-id.
pub fn pick_msgs(
    from: Option<&str>,
    last: u32,
    multi: bool,
    lines: bool,
    cfg: &Config,
) -> Result<(), String> {
    let msgs = load_msgs(from, last, cfg)?;
    let chosen = if multi {
        pick_many("message", msgs)?
    } else {
        pick_one("message", msgs)?
    };
    emit_values(chosen.into_iter().map(|m| m.id).collect(), lines)
}

/// `list msgs`: print message-ids, or a JSON array with `--json`.
pub fn list_msgs(from: Option<&str>, last: u32, json: bool, cfg: &Config) -> Result<(), String> {
    let msgs = load_msgs(from, last, cfg)?;
    if json {
        let out: Vec<serde_json::Value> = msgs
            .iter()
            .map(|m| {
                serde_json::json!({
                    "message_id": m.id,
                    "from_name": m.from_name,
                    "from_addr": m.from_addr,
                    "subject": m.subject,
                })
            })
            .collect();
        return println_json(&out);
    }
    print_values(msgs.iter().map(|m| m.id.clone()))
}

pub(crate) fn println_json(value: &[serde_json::Value]) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string(value).map_err(|e| format!("json: {e}"))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, name: &str, addr: &str, subject: &str, date: i64) -> crate::index::MessageRow {
        crate::index::MessageRow {
            message_id: id.to_string(),
            from_name: name.to_string(),
            from_addr: addr.to_string(),
            subject: subject.to_string(),
            date,
        }
    }

    #[test]
    fn message_display_aligns_three_columns() {
        let msgs = build_msgs(vec![
            row("1", "Alice", "a@x", "Hi", 0),
            row("2", "Bob", "b@x", "", 0),
            row("3", "", "weekly@x", "subj", 0),
        ]);
        let rendered: Vec<String> = msgs.iter().map(|m| m.to_string()).collect();

        // The sender column holds the full "Name <email>" form.
        assert!(rendered[0].starts_with("Alice <a@x>"));
        assert!(rendered[1].starts_with("Bob <b@x>"));
        assert!(rendered[2].starts_with("weekly@x"));

        // The subject starts after sender (max 11) + 2 + datetime (16) + 2.
        let col = "Alice <a@x>".len() + 2 + 16 + 2;
        assert_eq!(&rendered[0][col..], "Hi");
        assert_eq!(&rendered[1][col..], "(no subject)");
        assert_eq!(&rendered[2][col..], "subj");
    }

    #[test]
    fn format_sender_combines_name_and_address() {
        assert_eq!(format_sender("Alice", "a@x"), "Alice <a@x>");
        assert_eq!(format_sender("", "a@x"), "a@x");
        assert_eq!(format_sender("Alice", ""), "Alice");
    }

    #[test]
    fn truncate_caps_at_max_with_ellipsis() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("abcdefgh", 5), "abcd…");
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn message_display_truncates_long_columns() {
        let long_subject = "x".repeat(100);
        let msgs = build_msgs(vec![row(
            "1",
            "A very long display name over here",
            "averylongemail@example.com",
            &long_subject,
            0,
        )]);
        let rendered = msgs[0].to_string();

        // Sender and subject are both truncated, so the row stays bounded.
        assert!(rendered.contains('…'));
        assert!(rendered.chars().count() <= MAX_SENDER + 2 + 16 + 2 + MAX_SUBJECT);
    }

    #[test]
    fn format_datetime_empty_when_absent() {
        assert_eq!(format_datetime(0), "");
        assert_eq!(format_datetime(-5), "");
    }
}

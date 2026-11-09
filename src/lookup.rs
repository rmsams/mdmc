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

/// A message row, rendered as `sender — subject`.
struct Msg {
    id: String,
    from_name: String,
    from_addr: String,
    subject: String,
}

impl Msg {
    fn sender(&self) -> &str {
        if self.from_name.is_empty() {
            &self.from_addr
        } else {
            &self.from_name
        }
    }
}

impl fmt::Display for Msg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let subject = if self.subject.is_empty() {
            "(no subject)"
        } else {
            &self.subject
        };
        write!(f, "{} — {subject}", self.sender())
    }
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

fn pick_one<T: fmt::Display>(prompt: &str, options: Vec<T>) -> Result<Vec<T>, String> {
    if options.is_empty() {
        return Err("no matches".to_string());
    }
    let selected = inquire::Select::new(prompt, options)
        .raw_prompt()
        .map(|op| op.value)
        .map_err(|e| format!("picker failed: {e}"))?;
    Ok(vec![selected])
}

fn pick_many<T: fmt::Display>(prompt: &str, options: Vec<T>) -> Result<Vec<T>, String> {
    if options.is_empty() {
        return Err("no matches".to_string());
    }
    let selected = inquire::MultiSelect::new(prompt, options)
        .raw_prompt()
        .map(|ops| ops.into_iter().map(|op| op.value).collect())
        .map_err(|e| format!("picker failed: {e}"))?;
    Ok(selected)
}

fn print_values(values: impl Iterator<Item = String>) -> Result<(), String> {
    let mut out = std::io::stdout().lock();
    for value in values {
        writeln!(out, "{value}").map_err(|e| format!("write failed: {e}"))?;
    }
    Ok(())
}

/// The `addr` command: pick an address (or print all matches) and write the
/// chosen `Name <email>` value(s) to stdout.
pub fn addr(last: u32, print: bool, multi: bool, json: bool, _cfg: &Config) -> Result<(), String> {
    let db = crate::index::db_path();
    require_index(&db)?;
    let rows = crate::index::addresses(&db, since(last))?;
    let addrs: Vec<Addr> = rows
        .into_iter()
        .map(|(address, name)| Addr { name, address })
        .collect();

    if json {
        let out: Vec<serde_json::Value> = addrs
            .iter()
            .map(|a| serde_json::json!({"name": a.name, "address": a.address}))
            .collect();
        return println_json(&out);
    }
    if print {
        return print_values(addrs.iter().map(|a| a.name_addr()));
    }
    let chosen = if multi {
        pick_many("address", addrs)?
    } else {
        pick_one("address", addrs)?
    };
    print_values(chosen.into_iter().map(|a| a.name_addr()))
}

/// The `mid` command: pick a message (or print all matches) and write the
/// chosen message-id(s) to stdout.
pub fn mid(
    from: Option<&str>,
    last: u32,
    print: bool,
    multi: bool,
    json: bool,
    _cfg: &Config,
) -> Result<(), String> {
    let db = crate::index::db_path();
    require_index(&db)?;
    let rows = crate::index::messages(&db, since(last), from)?;
    let msgs: Vec<Msg> = rows
        .into_iter()
        .map(|(id, from_name, from_addr, subject)| Msg {
            id,
            from_name,
            from_addr,
            subject,
        })
        .collect();

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
    if print {
        return print_values(msgs.iter().map(|m| m.id.clone()));
    }
    let chosen = if multi {
        pick_many("message", msgs)?
    } else {
        pick_one("message", msgs)?
    };
    print_values(chosen.into_iter().map(|m| m.id))
}

/// Pick a single message and return its message-id (used by the `view` and
/// `decompile` omit-input pickers).
pub fn pick_message_id(from: Option<&str>, last: u32, _cfg: &Config) -> Result<String, String> {
    let db = crate::index::db_path();
    require_index(&db)?;
    let rows = crate::index::messages(&db, since(last), from)?;
    let msgs: Vec<Msg> = rows
        .into_iter()
        .map(|(id, from_name, from_addr, subject)| Msg {
            id,
            from_name,
            from_addr,
            subject,
        })
        .collect();
    if msgs.is_empty() {
        return Err("no messages in index — run `mdmc index` first".to_string());
    }
    let chosen = pick_one("message", msgs)?;
    Ok(chosen.into_iter().next().unwrap().id)
}

fn println_json(value: &[serde_json::Value]) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string(value).map_err(|e| format!("json: {e}"))?
    );
    Ok(())
}

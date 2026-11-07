use std::process::Command;

use serde_json::Value;

fn notmuch_json(message_id: &str) -> Option<Value> {
    let out = Command::new("notmuch")
        .args(["show", "--format=json", &format!("id:{message_id}")])
        .output()
        .ok()?;
    if !out.status.success() || out.stdout.iter().all(|&b| b.is_ascii_whitespace()) {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

fn text_parts(parts: &[Value]) -> String {
    let mut result: Vec<String> = Vec::new();
    for p in parts {
        if !p.is_object() {
            continue;
        }
        let ct = p
            .get("content-type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        if ct == "text/plain" {
            if let Some(c) = p.get("content").and_then(|v| v.as_str()) {
                result.push(c.to_string());
            }
        } else if let Some(list) = p.get("content").and_then(|v| v.as_array()) {
            let s = text_parts(list);
            if !s.is_empty() {
                result.push(s);
            }
        }
    }
    result
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn find_message_node<'a>(nodes: &'a [Value], wanted_id: &str) -> Option<&'a Value> {
    fn walk<'a>(messages: &'a [Value], wanted_id: &str) -> Option<&'a Value> {
        for node in messages {
            if node.is_null() {
                continue;
            }
            let msg = match node.as_array() {
                Some(arr) => match arr.first() {
                    Some(m) => m,
                    None => continue,
                },
                None => node,
            };
            if msg.is_object() && msg.get("id").and_then(|v| v.as_str()) == Some(wanted_id) {
                return Some(msg);
            }
            if let Some(replies) = node
                .as_array()
                .and_then(|a| a.get(1))
                .and_then(|v| v.as_array())
            {
                if let Some(found) = walk(replies, wanted_id) {
                    return Some(found);
                }
            }
        }
        None
    }

    for thread in nodes {
        if let Some(msgs) = thread.as_array() {
            if let Some(found) = walk(msgs, wanted_id) {
                return Some(found);
            }
        }
    }
    None
}

/// Build the reply quote for a message-id, or an empty string when the
/// message cannot be found.
pub fn build_quote(in_reply_to: &str) -> String {
    let Some(data) = notmuch_json(in_reply_to) else {
        return String::new();
    };
    let Some(nodes) = data.as_array() else {
        return String::new();
    };
    let Some(msg) = find_message_node(nodes, in_reply_to) else {
        return String::new();
    };
    let headers = msg.get("headers").and_then(|h| h.as_object());
    let name = headers
        .and_then(|h| h.get("From"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let date = headers
        .and_then(|h| h.get("Date"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();

    let body = text_parts(
        msg.get("body")
            .and_then(|b| b.as_array())
            .map_or(&[], |v| v),
    );
    let body = body.trim();
    let quoted = body
        .split('\n')
        .map(|l| {
            if l.is_empty() {
                ">".to_string()
            } else {
                format!("> {l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let who = if name.is_empty() {
        "wrote:".to_string()
    } else {
        format!("{name} wrote:")
    };
    let when = if date.is_empty() {
        String::new()
    } else {
        format!("On {date}, ")
    };
    format!("\n\n{when}{who}\n\n{quoted}")
}

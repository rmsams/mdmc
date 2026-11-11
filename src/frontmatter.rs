use regex::Regex;
use serde::Deserialize;

static MAIL_DRAFT_SCHEMA: &str = include_str!("../schema/mail-draft.schema.json");

/// A YAML field that may be a single string or a list of strings.
#[derive(Debug, Clone, Default)]
pub struct StringList(pub Vec<String>);

impl<'de> Deserialize<'de> for StringList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            One(String),
            Many(Vec<String>),
        }
        Ok(match Repr::deserialize(deserializer)? {
            Repr::One(s) => StringList(vec![s]),
            Repr::Many(v) => StringList(v),
        })
    }
}

/// The frontmatter fields `compile` reads from a draft. Unknown fields
/// (title, type, status, message-id, …) are ignored.
#[derive(Debug, Default, Deserialize)]
pub struct Draft {
    pub title: Option<String>,
    pub date: Option<String>,
    pub account: Option<String>,
    #[serde(rename = "to", default)]
    pub to: StringList,
    #[serde(rename = "cc", default)]
    pub cc: StringList,
    #[serde(rename = "bcc", default)]
    pub bcc: StringList,
    pub subject: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(rename = "message-id", default)]
    pub message_id: Option<String>,
    #[serde(rename = "in-reply-to", default)]
    pub in_reply_to: Option<String>,
    #[serde(rename = "references", default)]
    pub references: StringList,
    #[serde(default)]
    pub quote: Option<String>,
    #[serde(default)]
    pub forward: Option<String>,
    #[serde(rename = "forward-attach", default)]
    pub forward_attach: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub css: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(rename = "lua-filter", default)]
    pub lua_filter: Option<String>,
    #[serde(rename = "link-format", default)]
    pub link_format: Option<String>,
    #[serde(rename = "pandoc-raw", default)]
    pub pandoc_raw: Option<bool>,
    #[serde(default)]
    pub sign: Option<bool>,
    #[serde(default)]
    pub encrypt: Option<bool>,
    #[serde(default)]
    pub attachments: Vec<String>,
}

impl Draft {
    pub fn format(&self) -> &str {
        self.format.as_deref().unwrap_or("text")
    }
}

/// Extract the `---`-delimited YAML frontmatter text and the body that follows
/// it. Returns `None` when the text has no `---` frontmatter.
fn extract(text: &str) -> Option<(&str, &str)> {
    if !text.starts_with("---") {
        return None;
    }
    let rest = &text[3..];
    match rest.chars().next() {
        Some(c) if c != '\n' && c != '\r' => return None,
        None => return None,
        _ => {}
    }

    let re = Regex::new(r"(?m)^---[ \t]*$").expect("static regex");
    let m = re.find(rest)?;

    let yaml_text = rest[..m.start()].trim_end_matches(['\n', '\r']);
    let body = &rest[m.end()..];
    Some((yaml_text, body))
}

/// Parse `---`-delimited YAML frontmatter.
///
/// Returns `(None, None, text)` when the text has no valid `---` frontmatter.
/// On success the first element is the typed `Draft`, and the second is the
/// full frontmatter as a JSON value (for schema validation; `None` if the YAML
/// failed to parse). A present-but-unparseable frontmatter yields a default
/// (empty) `Draft`, matching the Python tool's behaviour of failing later on
/// the missing required fields.
pub fn load(text: &str) -> (Option<Draft>, Option<serde_json::Value>, &str) {
    let Some((yaml_text, body)) = extract(text) else {
        return (None, None, text);
    };
    let draft = serde_yaml::from_str::<Draft>(yaml_text).unwrap_or_default();
    let json = serde_yaml::from_str::<serde_json::Value>(yaml_text).ok();
    (Some(draft), json, body)
}

/// Rewrite scalar frontmatter fields in place, preserving all other content
/// and formatting. A key already present has its value replaced; a missing key
/// is appended before the closing `---`. Values are written as plain scalars.
pub fn set_fields(text: &str, fields: &[(&str, &str)]) -> String {
    let Some(closing) = closing_line_index(text) else {
        return text.to_string();
    };
    let lines: Vec<&str> = text.split('\n').collect();

    let mut seen = vec![false; fields.len()];
    let mut body: Vec<String> = Vec::new();
    for line in &lines[1..closing] {
        let mut replaced = false;
        for (i, (key, value)) in fields.iter().enumerate() {
            if line.starts_with(&format!("{key}:")) {
                body.push(format!("{key}: {value}"));
                seen[i] = true;
                replaced = true;
                break;
            }
        }
        if !replaced {
            body.push((*line).to_string());
        }
    }
    for (i, (key, value)) in fields.iter().enumerate() {
        if !seen[i] {
            body.push(format!("{key}: {value}"));
        }
    }

    let mut out = String::from("---\n");
    out.push_str(&body.join("\n"));
    out.push('\n');
    out.push_str(&lines[closing..].join("\n"));
    out
}

fn closing_line_index(text: &str) -> Option<usize> {
    if !text.starts_with("---") {
        return None;
    }
    for (i, line) in text.split('\n').enumerate().skip(1) {
        if line.trim_end() == "---" {
            return Some(i);
        }
    }
    None
}

/// A single frontmatter field value: a scalar, a list of scalars, a boolean,
/// or a list of key/value objects (each an ordered pair list).
pub enum Field {
    Str(String),
    List(Vec<String>),
    Bool(bool),
    ObjectList(Vec<Vec<(String, String)>>),
}

/// Validate a frontmatter instance against the embedded `mail-draft` schema.
pub fn validate(fm: &serde_json::Value) -> Result<(), Vec<String>> {
    let schema: serde_json::Value =
        serde_json::from_str(MAIL_DRAFT_SCHEMA).expect("embedded mail-draft schema is valid JSON");
    let validator = jsonschema::validator_for(&schema).map_err(|e| vec![e.to_string()])?;
    let errors: Vec<String> = validator.iter_errors(fm).map(|e| e.to_string()).collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Render an ordered field list to a `serde_json::Value` for schema validation.
pub fn to_json(fields: &[(String, Field)]) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (key, value) in fields {
        let v = match value {
            Field::Str(s) => serde_json::Value::String(s.clone()),
            Field::List(items) => serde_json::Value::Array(
                items
                    .iter()
                    .map(|s| serde_json::Value::String(s.clone()))
                    .collect(),
            ),
            Field::ObjectList(items) => serde_json::Value::Array(
                items
                    .iter()
                    .map(|obj| {
                        let mut m = serde_json::Map::new();
                        for (k, v) in obj {
                            m.insert(k.clone(), serde_json::Value::String(v.clone()));
                        }
                        serde_json::Value::Object(m)
                    })
                    .collect(),
            ),
            Field::Bool(b) => serde_json::Value::Bool(*b),
        };
        map.insert(key.clone(), v);
    }
    serde_json::Value::Object(map)
}

/// Render a complete note: `---`-delimited frontmatter, a blank line, and the
/// (already right-trimmed) body. Field order is preserved.
pub fn render_note(fields: &[(String, Field)], body_text: &str) -> String {
    let mut yaml = String::new();
    for (key, value) in fields {
        match value {
            Field::Str(s) => {
                yaml.push_str(&format!("{key}: {}\n", scalar(s)));
            }
            Field::List(items) => {
                yaml.push_str(&format!("{key}:\n"));
                for item in items {
                    yaml.push_str(&format!("  - {}\n", scalar(item)));
                }
            }
            Field::ObjectList(items) => {
                yaml.push_str(&format!("{key}:\n"));
                for item in items {
                    for (i, (k, v)) in item.iter().enumerate() {
                        if i == 0 {
                            yaml.push_str(&format!("  - {k}: {}\n", scalar(v)));
                        } else {
                            yaml.push_str(&format!("    {k}: {}\n", scalar(v)));
                        }
                    }
                }
            }
            Field::Bool(b) => {
                yaml.push_str(&format!("{key}: {}\n", if *b { "true" } else { "false" }));
            }
        }
    }
    format!("---\n{yaml}---\n\n{body_text}\n")
}

/// Quote a scalar only when it would be misread as a plain YAML scalar.
fn scalar(s: &str) -> String {
    if needs_quotes(s) {
        format!("'{}'", s.replace('\'', "''"))
    } else {
        s.to_string()
    }
}

fn needs_quotes(s: &str) -> bool {
    if s.is_empty() || s != s.trim() || s.contains('\n') {
        return true;
    }
    if let Some(c) = s.chars().next() {
        if "-?:,[]{}#&*!|>'\"%@` ".contains(c) {
            return true;
        }
    }
    s.contains(": ")
        || s.contains(" #")
        || s.ends_with(':')
        || matches!(
            s.to_ascii_lowercase().as_str(),
            "null" | "~" | "true" | "false" | "yes" | "no" | "on" | "off"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_parses_frontmatter_and_body() {
        let text = "---\ntitle: 'Re: Test'\ntype: mail-draft\nstatus: draft\naccount: rmsams-gmail\nformat: text\nsubject: 'Re: Test'\nto:\n  - a@b.c\n---\n\nBody text\n";
        let (draft, json, body) = load(text);
        let draft = draft.unwrap();
        assert_eq!(draft.account.as_deref(), Some("rmsams-gmail"));
        assert_eq!(draft.to.0, vec!["a@b.c"]);
        assert_eq!(draft.subject.as_deref(), Some("Re: Test"));
        assert_eq!(draft.format(), "text");
        assert!(json.is_some());
        assert_eq!(body, "\n\nBody text\n");
    }

    #[test]
    fn load_returns_none_without_frontmatter() {
        let text = "just a body, no frontmatter";
        let (draft, json, body) = load(text);
        assert!(draft.is_none());
        assert!(json.is_none());
        assert_eq!(body, text);
    }

    #[test]
    fn load_tolerates_malformed_yaml() {
        let text = "---\n: : : not valid\n---\nbody\n";
        let (draft, json, _) = load(text);
        assert!(draft.is_some());
        assert!(json.is_none());
    }

    #[test]
    fn load_string_list_accepts_scalar() {
        let text = "---\ntype: mail-draft\nto: a@b.c\n---\n\n";
        let (draft, _, _) = load(text);
        assert_eq!(draft.unwrap().to.0, vec!["a@b.c"]);
    }

    #[test]
    fn set_fields_replaces_in_place() {
        let text = "---\ntitle: T\nstatus: draft\nmessage-id: old@x\n---\n\nbody\n";
        let out = set_fields(text, &[("status", "pending"), ("message-id", "new@x")]);
        assert!(out.contains("status: pending\n"));
        assert!(out.contains("message-id: new@x\n"));
        assert!(!out.contains("old@x"));
        assert!(out.contains("title: T\n"));
        assert!(out.ends_with("body\n"));
    }

    #[test]
    fn set_fields_appends_missing_key_before_closing_delimiter() {
        let text = "---\ntitle: T\nstatus: draft\n---\n\nbody\n";
        let out = set_fields(text, &[("message-id", "new@x")]);
        assert!(out.contains("message-id: new@x\n"));
        assert!(out.contains("title: T\n"));
        assert!(out.contains("status: draft\n"));
        assert!(out.ends_with("body\n"));
    }

    #[test]
    fn render_note_quotes_and_formats() {
        let fields = vec![
            (
                "title".to_string(),
                Field::Str("Re: CFSS-13158383".to_string()),
            ),
            ("type".to_string(), Field::Str("mail-draft".to_string())),
            ("to".to_string(), Field::List(vec!["a@b.c".to_string()])),
            ("sign".to_string(), Field::Bool(true)),
        ];
        let note = render_note(&fields, "hello");
        assert!(note.contains("title: 'Re: CFSS-13158383'\n"));
        assert!(note.contains("to:\n  - a@b.c\n"));
        assert!(note.contains("sign: true\n"));
        assert_eq!(note, "---\ntitle: 'Re: CFSS-13158383'\ntype: mail-draft\nto:\n  - a@b.c\nsign: true\n---\n\nhello\n");
    }

    #[test]
    fn validate_rejects_bad_type() {
        let json = serde_json::json!({"title": "T", "type": "not-mail-draft"});
        assert!(validate(&json).is_err());
    }

    #[test]
    fn validate_accepts_minimal_valid() {
        let json = serde_json::json!({"title": "T", "type": "mail-draft"});
        assert!(validate(&json).is_ok());
    }

    #[test]
    fn validate_rejects_bad_format() {
        let json = serde_json::json!({"title": "T", "type": "mail-draft", "format": "bogus"});
        assert!(validate(&json).is_err());
    }

    #[test]
    fn validate_accepts_quote_none() {
        let json = serde_json::json!({"title": "T", "type": "mail-draft", "quote": "none"});
        assert!(validate(&json).is_ok());
    }

    #[test]
    fn render_note_object_list_formats_entries() {
        let fields = vec![(
            "attachments".to_string(),
            Field::ObjectList(vec![vec![
                ("file".to_string(), "m1@x--a.pdf".to_string()),
                ("content-type".to_string(), "application/pdf".to_string()),
            ]]),
        )];
        let note = render_note(&fields, "body");
        assert_eq!(
            note,
            "---\nattachments:\n  - file: m1@x--a.pdf\n    content-type: application/pdf\n---\n\nbody\n"
        );
    }
}

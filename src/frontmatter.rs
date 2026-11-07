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

/// The frontmatter fields `write` reads from a draft note. Unknown fields
/// (title, type, status, message-id, …) are ignored.
#[derive(Debug, Default, Deserialize)]
pub struct Draft {
    pub account: Option<String>,
    #[serde(rename = "to", default)]
    pub to: StringList,
    #[serde(rename = "cc", default)]
    pub cc: StringList,
    #[serde(rename = "bcc", default)]
    pub bcc: StringList,
    pub subject: Option<String>,
    #[serde(rename = "message-id", default)]
    pub message_id: Option<String>,
    #[serde(rename = "in-reply-to", default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub quote: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub css: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(rename = "lua-filter", default)]
    pub lua_filter: Option<String>,
    #[serde(default)]
    pub attachments: Vec<String>,
}

impl Draft {
    pub fn format(&self) -> &str {
        self.format.as_deref().unwrap_or("text")
    }
}

/// Parse `---`-delimited YAML frontmatter.
///
/// Returns `(None, text)` when the text has no valid `---` frontmatter.
/// A present-but-unparseable frontmatter yields a default (empty) `Draft`
/// with the body, matching the Python tool's behaviour of failing later on
/// the missing required fields.
pub fn load(text: &str) -> (Option<Draft>, &str) {
    if !text.starts_with("---") {
        return (None, text);
    }
    let rest = &text[3..];
    match rest.chars().next() {
        Some(c) if c != '\n' && c != '\r' => return (None, text),
        None => return (None, text),
        _ => {}
    }

    let re = Regex::new(r"(?m)^---[ \t]*$").expect("static regex");
    let Some(m) = re.find(rest) else {
        return (None, text);
    };

    let yaml_text = rest[..m.start()].trim_end_matches(['\n', '\r']);
    let body = &rest[m.end()..];
    let draft = serde_yaml::from_str::<Draft>(yaml_text).unwrap_or_default();
    (Some(draft), body)
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

/// A single frontmatter field value: a scalar or a list of scalars.
pub enum Field {
    Str(String),
    List(Vec<String>),
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

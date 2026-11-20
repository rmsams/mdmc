use serde::Serialize;

use crate::config::Config;

/// A message-search hit, mirroring the notmuch `search --format=json` fields
/// the tool surfaces.
#[derive(Serialize)]
struct SearchHit {
    id: String,
    date: String,
    subject: String,
    from: String,
}

/// An address-book entry.
#[derive(Serialize)]
struct AddressHit {
    name: String,
    address: String,
}

/// `mdmc search`: search messages via notmuch and print their Message-IDs (or a
/// JSON array of hits). Never sends.
pub fn search(query: &str, limit: Option<usize>, json: bool) -> Result<String, String> {
    let threads = crate::notmuch::search(query, limit)?;
    let hits: Vec<SearchHit> = threads
        .iter()
        .map(|t| SearchHit {
            id: strip_id(
                t.query
                    .iter()
                    .flatten()
                    .next()
                    .map(String::as_str)
                    .unwrap_or(""),
            ),
            date: epoch_to_date(t.timestamp),
            subject: t.subject.clone(),
            from: t.authors.clone(),
        })
        .collect();
    if json {
        return format_json(&hits);
    }
    Ok(join_lines(hits.iter().map(|h| h.id.clone())))
}

/// `mdmc address`: look up addresses from the configured scope (sent mail by
/// default), filtered by an optional name/partial query. Never sends.
pub fn address(query: Option<&str>, json: bool, cfg: &Config) -> Result<String, String> {
    let scope = cfg
        .address_query()
        .map(str::to_string)
        .unwrap_or_else(|| default_scope(cfg));
    let entries = crate::notmuch::address(&scope)?;

    let words: Vec<String> = query
        .unwrap_or("")
        .to_lowercase()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let matched: Vec<&crate::notmuch::AddressEntry> = entries
        .iter()
        .filter(|e| {
            let name = e.name.to_lowercase();
            let addr = e.address.to_lowercase();
            words.iter().all(|w| name.contains(w) || addr.contains(w))
        })
        .collect();

    if json {
        let hits: Vec<AddressHit> = matched
            .iter()
            .map(|e| AddressHit {
                name: e.name.clone(),
                address: e.address.clone(),
            })
            .collect();
        return format_json(&hits);
    }
    Ok(join_lines(matched.iter().map(|e| {
        if e.name.is_empty() {
            e.address.clone()
        } else {
            format!("{} <{}>", e.name, e.address)
        }
    })))
}

/// The default notmuch scope for address lookup: messages sent from any
/// configured account (a `from:` OR-query), or `*` when no account has an
/// address.
fn default_scope(cfg: &Config) -> String {
    let froms: Vec<String> = cfg
        .accounts
        .values()
        .filter_map(|a| a.from.clone())
        .filter(|f| !f.is_empty())
        .collect();
    if froms.is_empty() {
        return "*".to_string();
    }
    froms
        .iter()
        .map(|f| format!("from:{f}"))
        .collect::<Vec<_>>()
        .join(" or ")
}

fn strip_id(raw: &str) -> String {
    raw.strip_prefix("id:").unwrap_or(raw).to_string()
}

fn join_lines(values: impl Iterator<Item = String>) -> String {
    let mut out = String::new();
    for value in values {
        out.push_str(&value);
        out.push('\n');
    }
    out
}

fn format_json<T: Serialize>(value: &[T]) -> Result<String, String> {
    Ok(format!(
        "{}\n",
        serde_json::to_string(value).map_err(|e| format!("json: {e}"))?
    ))
}

/// The calendar date (YYYY-MM-DD) of a unix timestamp.
fn epoch_to_date(secs: i64) -> String {
    let (y, m, d) = civil_from_days(secs.div_euclid(86400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to (year, month,
/// day).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_to_date_known_values() {
        assert_eq!(epoch_to_date(0), "1970-01-01");
        assert_eq!(epoch_to_date(86400), "1970-01-02");
        assert_eq!(epoch_to_date(951782400), "2000-02-29");
    }

    #[test]
    fn strip_id_removes_prefix() {
        assert_eq!(strip_id("id:m1@x"), "m1@x");
        assert_eq!(strip_id("m1@x"), "m1@x");
        assert_eq!(strip_id(""), "");
    }

    #[test]
    fn default_scope_joins_account_froms() {
        let cfg = Config {
            accounts: std::collections::BTreeMap::from([
                (
                    "a".to_string(),
                    crate::config::Account {
                        from: Some("a@x".to_string()),
                        ..Default::default()
                    },
                ),
                (
                    "b".to_string(),
                    crate::config::Account {
                        from: Some("b@x".to_string()),
                        ..Default::default()
                    },
                ),
            ]),
            ..Default::default()
        };
        assert_eq!(default_scope(&cfg), "from:a@x or from:b@x");
    }

    #[test]
    fn default_scope_falls_back_to_wildcard() {
        assert_eq!(default_scope(&Config::default()), "*");
    }
}

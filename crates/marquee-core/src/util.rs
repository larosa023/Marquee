//! Small helpers for reading loosely-typed JSON from third-party APIs.

use serde_json::Value;

pub fn s(v: &Value, key: &str) -> Option<String> {
    match &v[key] {
        Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub fn f(v: &Value, key: &str) -> Option<f64> {
    match &v[key] {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub fn i(v: &Value, key: &str) -> Option<i64> {
    match &v[key] {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub fn arr<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v[key].as_array().map(|a| a.as_slice()).unwrap_or(&[])
}

/// Some APIs (Last.fm) return an object instead of a one-element array.
pub fn list(v: &Value) -> Vec<&Value> {
    match v {
        Value::Array(a) => a.iter().collect(),
        Value::Null => vec![],
        other => vec![other],
    }
}

pub fn names(v: &Value, key: &str, field: &str) -> Vec<String> {
    arr(v, key).iter().filter_map(|x| s(x, field)).collect()
}

/// Accepts a full ISO date or datetime and returns YYYY-MM-DD.
pub fn date_only(d: &str) -> Option<String> {
    let d = d.trim();
    if d.len() >= 10 && d.as_bytes()[4] == b'-' && d.as_bytes()[7] == b'-' && d[..4].chars().all(|c| c.is_ascii_digit()) {
        Some(d[..10].to_string())
    } else {
        None
    }
}

/// MusicBrainz dates can be "2026", "2026-03" or "2026-03-14". Partial dates sort to the
/// end of their period so they don't show as already released.
pub fn partial_date(d: &str) -> Option<String> {
    let d = d.trim();
    match d.len() {
        4 if d.chars().all(|c| c.is_ascii_digit()) => Some(format!("{d}-12-31")),
        7 => Some(format!("{d}-28")),
        _ => date_only(d),
    }
}

pub fn unix_to_date(ts: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(ts, 0).map(|d| d.format("%Y-%m-%d").to_string())
}

pub fn unix_to_rfc3339(ts: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(ts, 0).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(date_only("2026-11-13T10:00:00Z").as_deref(), Some("2026-11-13"));
        assert_eq!(date_only("TBA"), None);
        assert_eq!(partial_date("2026").as_deref(), Some("2026-12-31"));
        assert_eq!(partial_date("2026-03").as_deref(), Some("2026-03-28"));
        assert_eq!(unix_to_date(1_700_000_000).as_deref(), Some("2023-11-14"));
    }
}

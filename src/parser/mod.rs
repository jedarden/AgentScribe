//! Parser implementations for different log formats
//!
//! Each format has a dedicated parser that normalizes events to the canonical schema.

mod aider_input;
mod json_array;
mod json_tree;
mod jsonl;
mod markdown;
mod sqlite;

pub use aider_input::{AiderInputEntry, AiderInputHistory};
pub use json_array::JsonArrayParser;
pub use json_tree::JsonTreeParser;
pub use jsonl::JsonlParser;
pub use markdown::MarkdownParser;
pub use sqlite::SqliteParser;

use crate::error::{AgentScribeError, Result};
use crate::event::Event;
use crate::plugin::Plugin;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::path::Path;

/// Context for parsing - contains info about the source file and session
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ParseContext {
    pub session_id: String,
    pub source_agent: String,
    pub source_file: String,
    pub project: Option<String>,
    pub model: Option<String>,
    pub line_number: usize,
}

impl ParseContext {
    pub fn new(session_id: String, source_agent: String, source_file: String) -> Self {
        ParseContext {
            session_id,
            source_agent,
            source_file,
            project: None,
            model: None,
            line_number: 0,
        }
    }

    #[allow(dead_code)]
    pub fn with_project(mut self, project: Option<String>) -> Self {
        self.project = project;
        self
    }

    #[allow(dead_code)]
    pub fn with_model(mut self, model: Option<String>) -> Self {
        self.model = model;
        self
    }

    #[allow(dead_code)]
    pub fn increment_line(&mut self) {
        self.line_number += 1;
    }
}

/// Check if a field name has a caret prefix (^)
///
/// Returns true if the field name starts with `^`, false otherwise.
/// Handles empty strings gracefully (returns false).
///
/// # Examples
/// ```
/// assert!(has_caret_prefix("^foo"));
/// assert!(!has_caret_prefix("foo"));
/// assert!(!has_caret_prefix("bar^baz"));
/// assert!(!has_caret_prefix(""));
/// ```
pub fn has_caret_prefix(field_name: &str) -> bool {
    field_name.starts_with('^')
}

/// Extract a nested field from JSON using dot notation
pub fn extract_field(value: &Value, path: &str) -> Option<Value> {
    if path.is_empty() {
        return None;
    }

    let mut current = value;
    for part in path.split('.') {
        // Handle array indexing like parts[0]
        if let Some(bracket_pos) = part.find('[') {
            let key = &part[..bracket_pos];
            let index_str = &part[bracket_pos + 1..part.len() - 1];
            let index: usize = index_str.parse().ok()?;

            current = current.get(key)?.get(index)?;
        } else {
            current = current.get(part)?;
        }
    }
    Some(current.clone())
}

/// Extract a string field from JSON
pub fn extract_string(value: &Value, path: &str) -> Option<String> {
    let field = extract_field(value, path)?;
    match field {
        Value::String(s) => Some(s),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => Some(String::new()),
        _ => None,
    }
}

/// Parse timestamp from various formats
pub fn parse_timestamp(value: &Value, path: &str) -> Result<DateTime<Utc>> {
    let ts_str = extract_string(value, path)
        .ok_or_else(|| AgentScribeError::Timestamp(format!("Field '{}' not found", path)))?;

    // Try ISO 8601 first
    if let Ok(dt) = DateTime::parse_from_rfc3339(&ts_str) {
        return Ok(dt.with_timezone(&Utc));
    }

    // Try parsing as Unix epoch (seconds)
    if let Ok(seconds) = ts_str.parse::<i64>() {
        let ts = if seconds > 1_000_000_000_000 {
            // Milliseconds
            DateTime::from_timestamp_millis(seconds)
        } else {
            // Seconds
            DateTime::from_timestamp(seconds, 0)
        };
        return ts.ok_or_else(|| AgentScribeError::Timestamp("Invalid timestamp".to_string()));
    }

    // Try parsing without timezone (assume UTC)
    if let Ok(dt) = ts_str.parse::<DateTime<Utc>>() {
        return Ok(dt);
    }

    Err(AgentScribeError::Timestamp(format!(
        "Cannot parse timestamp: {}",
        ts_str
    )))
}

/// Extract a field from either envelope or payload based on path prefix
///
/// **Envelope-first with payload fallback for caret-prefixed paths:**
/// - If path starts with `^`, try envelope first (remove `^` prefix)
/// - If envelope is None OR field not found in envelope, fallback to payload
/// - Otherwise, extract from payload only
///
/// **Resolution order for `^field`:**
/// 1. Try `envelope.field` (if envelope exists)
/// 2. Fallback to `payload.field` (if envelope missing or field not found)
/// 3. Return None (if both fail)
///
/// **Resolution order for `field` (no caret):**
/// - Extract from `payload.field` only
///
/// Supports dot notation for nested fields (e.g., `^outer.ts` or `user.role`)
/// and array indexing (e.g., `items[0].name`).
pub fn extract_with_envelope(
    path: &str,
    payload: &Value,
    envelope: Option<&Value>,
) -> Option<Value> {
    if has_caret_prefix(path) {
        // Strip the caret prefix
        if let Some(envelope_path) = path.strip_prefix('^') {
            // Try envelope first
            if let Some(env) = envelope {
                if let Some(value) = extract_field(env, envelope_path) {
                    return Some(value); // Found in envelope
                }
            }
            // Fallback to payload (using path without ^)
            extract_field(payload, envelope_path)
        } else {
            // Should not happen due to starts_with check, but handle defensively
            None
        }
    } else {
        // No caret prefix - extract from payload only
        extract_field(payload, path)
    }
}

/// Extract a string field from either envelope or payload based on path prefix
///
/// Wrapper around `extract_with_envelope` that converts the raw `Value` to a string,
/// following the same coercion rules as `extract_string` (String, Number, Bool, Null).
pub fn extract_string_with_envelope(
    path: &str,
    payload: &Value,
    envelope: Option<&Value>,
) -> Option<String> {
    let value = extract_with_envelope(path, payload, envelope)?;
    match value {
        Value::String(s) => Some(s),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => Some(String::new()),
        _ => None,
    }
}

/// Parse a timestamp from either envelope or payload based on path prefix
///
/// Wrapper around `extract_string_with_envelope` that parses the extracted string
/// as a timestamp. Supports ISO 8601, Unix epoch (seconds/milliseconds), and
/// UTC-naive formats.
pub fn parse_timestamp_with_envelope(
    path: &str,
    payload: &Value,
    envelope: Option<&Value>,
) -> Result<DateTime<Utc>> {
    let ts_str = extract_string_with_envelope(path, payload, envelope)
        .ok_or_else(|| AgentScribeError::Timestamp(format!("Field '{}' not found", path)))?;

    // Try ISO 8601 first
    if let Ok(dt) = DateTime::parse_from_rfc3339(&ts_str) {
        return Ok(dt.with_timezone(&Utc));
    }

    // Try parsing as Unix epoch (seconds)
    if let Ok(seconds) = ts_str.parse::<i64>() {
        let ts = if seconds > 1_000_000_000_000 {
            // Milliseconds
            DateTime::from_timestamp_millis(seconds)
        } else {
            // Seconds
            DateTime::from_timestamp(seconds, 0)
        };
        return ts.ok_or_else(|| AgentScribeError::Timestamp("Invalid timestamp".to_string()));
    }

    // Try parsing without timezone (assume UTC)
    if let Ok(dt) = ts_str.parse::<DateTime<Utc>>() {
        return Ok(dt);
    }

    Err(AgentScribeError::Timestamp(format!(
        "Cannot parse timestamp: {}",
        ts_str
    )))
}

/// Base trait for all format parsers
pub trait FormatParser {
    /// Parse events from the source
    fn parse(&self, source_path: &Path, plugin: &Plugin) -> Result<Vec<Event>>;

    /// Detect session boundaries in the source
    fn detect_sessions(&self, source_path: &Path, plugin: &Plugin) -> Result<Vec<SessionInfo>>;
}

/// Information about a detected session
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SessionInfo {
    pub session_id: String,
    pub start_offset: u64,
    pub end_offset: u64,
    pub metadata: Option<Value>,
    /// Parent session ID (populated for subagent sessions where this session
    /// is a child of another session)
    pub parent_session_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_field_simple() {
        let value = json!({"name": "test", "count": 42});
        assert_eq!(extract_string(&value, "name"), Some("test".to_string()));
        assert_eq!(extract_string(&value, "count"), Some("42".to_string()));
        assert_eq!(extract_string(&value, "missing"), None);
    }

    #[test]
    fn test_extract_field_nested() {
        let value = json!({"user": {"name": "alice", "age": 30}});
        assert_eq!(
            extract_string(&value, "user.name"),
            Some("alice".to_string())
        );
        assert_eq!(extract_string(&value, "user.age"), Some("30".to_string()));
    }

    #[test]
    fn test_extract_field_array() {
        let value = json!({"items": [{"name": "first"}, {"name": "second"}]});
        assert_eq!(
            extract_string(&value, "items[0].name"),
            Some("first".to_string())
        );
        assert_eq!(
            extract_string(&value, "items[1].name"),
            Some("second".to_string())
        );
    }

    #[test]
    fn test_parse_timestamp_iso() {
        let value = json!({"ts": "2026-03-16T12:00:00Z"});
        let result = parse_timestamp(&value, "ts");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_timestamp_epoch() {
        let value = json!({"ts": 1710590400});
        let result = parse_timestamp(&value, "ts");
        assert!(result.is_ok());
    }

    // extract_with_envelope tests

    #[test]
    fn test_extract_with_envelope_from_envelope() {
        let payload = json!({"role": "user", "content": "hello"});
        let envelope = json!({"timestamp": "2026-03-16T12:00:00Z", "session_id": "abc123"});

        // Extract from envelope using ^ prefix
        let result = extract_with_envelope("^timestamp", &payload, Some(&envelope));
        assert_eq!(result, Some(json!("2026-03-16T12:00:00Z")));
    }

    #[test]
    fn test_extract_with_envelope_from_payload() {
        let payload = json!({"role": "user", "content": "hello"});
        let envelope = json!({"timestamp": "2026-03-16T12:00:00Z"});

        // Extract from payload (no ^ prefix)
        let result = extract_with_envelope("role", &payload, Some(&envelope));
        assert_eq!(result, Some(json!("user")));
    }

    #[test]
    fn test_extract_with_envelope_dot_notation_from_envelope() {
        let payload = json!({"role": "user"});
        let envelope = json!({"outer": {"ts": "2026-03-16T12:00:00Z", "id": "xyz"}});

        // Extract nested field from envelope using ^ prefix with dot notation
        let result = extract_with_envelope("^outer.ts", &payload, Some(&envelope));
        assert_eq!(result, Some(json!("2026-03-16T12:00:00Z")));
    }

    #[test]
    fn test_extract_with_envelope_dot_notation_from_payload() {
        let payload = json!({"user": {"role": "admin", "name": "alice"}});
        let envelope = json!({"outer": {"ts": "2026-03-16T12:00:00Z"}});

        // Extract nested field from payload using dot notation
        let result = extract_with_envelope("user.role", &payload, Some(&envelope));
        assert_eq!(result, Some(json!("admin")));
    }

    #[test]
    fn test_extract_with_envelope_no_envelope_fallback_to_payload() {
        let payload = json!({"role": "user", "content": "hello"});

        // No envelope provided, should extract from payload
        let result = extract_with_envelope("role", &payload, None);
        assert_eq!(result, Some(json!("user")));
    }

    #[test]
    fn test_extract_with_envelope_caret_prefix_no_envelope_fallback_to_payload() {
        let payload =
            json!({"role": "user", "content": "hello", "timestamp": "2026-03-16T12:00:00Z"});

        // ^ prefix with no envelope should fallback to payload
        let result = extract_with_envelope("^timestamp", &payload, None);
        assert_eq!(result, Some(json!("2026-03-16T12:00:00Z")));
    }

    #[test]
    fn test_extract_with_envelope_missing_field_from_envelope_fallback_to_payload() {
        let payload = json!({"role": "user", "model": "gpt-4"});
        let envelope = json!({"timestamp": "2026-03-16T12:00:00Z"});

        // Missing field in envelope should fallback to payload
        let result = extract_with_envelope("^model", &payload, Some(&envelope));
        assert_eq!(result, Some(json!("gpt-4")));
    }

    #[test]
    fn test_extract_with_envelope_fallback_both_missing() {
        let payload = json!({"role": "user"});
        let envelope = json!({"timestamp": "2026-03-16T12:00:00Z"});

        // Field missing in both envelope and payload should return None
        let result = extract_with_envelope("^model", &payload, Some(&envelope));
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_with_envelope_missing_field_from_payload() {
        let payload = json!({"role": "user"});
        let envelope = json!({"timestamp": "2026-03-16T12:00:00Z"});

        // Missing field in payload should return None
        let result = extract_with_envelope("missing_field", &payload, Some(&envelope));
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_with_envelope_array_from_envelope() {
        let payload = json!({"role": "user"});
        let envelope = json!({"items": [{"name": "first"}, {"name": "second"}]});

        // Extract array element from envelope using ^ prefix
        let result = extract_with_envelope("^items[0].name", &payload, Some(&envelope));
        assert_eq!(result, Some(json!("first")));
    }

    #[test]
    fn test_extract_with_envelope_array_from_payload() {
        let payload = json!({"items": [{"name": "first"}, {"name": "second"}]});
        let envelope = json!({"timestamp": "2026-03-16T12:00:00Z"});

        // Extract array element from payload
        let result = extract_with_envelope("items[1].name", &payload, Some(&envelope));
        assert_eq!(result, Some(json!("second")));
    }

    #[test]
    fn test_extract_with_envelope_empty_path() {
        let payload = json!({"role": "user"});
        let envelope = json!({"timestamp": "2026-03-16T12:00:00Z"});

        // Empty path should return None
        let result = extract_with_envelope("", &payload, Some(&envelope));
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_with_envelope_only_caret_prefix() {
        let payload = json!({"role": "user"});
        let envelope = json!({"timestamp": "2026-03-16T12:00:00Z"});

        // Only ^ with no field name should return None (empty path after removing ^)
        let result = extract_with_envelope("^", &payload, Some(&envelope));
        assert_eq!(result, None);
    }

    // has_caret_prefix tests

    #[test]
    fn test_has_caret_prefix_with_caret() {
        assert!(has_caret_prefix("^foo"));
        assert!(has_caret_prefix("^timestamp"));
        assert!(has_caret_prefix("^outer.ts"));
        assert!(has_caret_prefix("^items[0].name"));
    }

    #[test]
    fn test_has_caret_prefix_without_caret() {
        assert!(!has_caret_prefix("foo"));
        assert!(!has_caret_prefix("timestamp"));
        assert!(!has_caret_prefix("bar^baz"));
        assert!(!has_caret_prefix("items[0].name"));
    }

    #[test]
    fn test_has_caret_prefix_empty_string() {
        assert!(!has_caret_prefix(""));
    }

    #[test]
    fn test_has_caret_prefix_only_caret() {
        assert!(has_caret_prefix("^"));
    }

    // Additional caret-prefix edge cases

    #[test]
    fn test_has_caret_prefix_various_formats() {
        // Unicode and special characters
        assert!(has_caret_prefix("^_field"));
        assert!(has_caret_prefix("^$field"));
        assert!(has_caret_prefix("^field-name"));
        assert!(has_caret_prefix("^field_name"));
        assert!(has_caret_prefix("^fieldName"));

        // Not a caret prefix
        assert!(!has_caret_prefix(" ^field")); // space before caret
        assert!(!has_caret_prefix("field^")); // caret after
        assert!(has_caret_prefix("^ ")); // caret then space - still starts with ^
        assert!(has_caret_prefix("^\t")); // caret then tab - still starts with ^
        assert!(has_caret_prefix("^0")); // caret then number - valid
        assert!(has_caret_prefix("^🔍")); // caret then emoji - valid Unicode
    }

    // extract_string_with_envelope tests

    #[test]
    fn test_extract_string_with_envelope_from_envelope() {
        let payload = json!({"role": "user"});
        let envelope = json!({"timestamp": "2026-03-16T12:00:00Z"});

        // Extract string from envelope using ^ prefix
        let result = extract_string_with_envelope("^timestamp", &payload, Some(&envelope));
        assert_eq!(result, Some("2026-03-16T12:00:00Z".to_string()));
    }

    #[test]
    fn test_extract_string_with_envelope_from_payload() {
        let payload = json!({"role": "user", "content": "hello"});
        let envelope = json!({"timestamp": "2026-03-16T12:00:00Z"});

        // Extract string from payload (no ^ prefix)
        let result = extract_string_with_envelope("role", &payload, Some(&envelope));
        assert_eq!(result, Some("user".to_string()));
    }

    #[test]
    fn test_extract_string_with_envelope_number_coercion() {
        let payload = json!({"count": 42});
        let envelope = json!({"timestamp": 1710590400});

        // Numbers should be coerced to strings
        let result1 = extract_string_with_envelope("count", &payload, None);
        assert_eq!(result1, Some("42".to_string()));

        let result2 = extract_string_with_envelope("^timestamp", &payload, Some(&envelope));
        assert_eq!(result2, Some("1710590400".to_string()));
    }

    #[test]
    fn test_extract_string_with_envelope_bool_coercion() {
        let payload = json!({"active": true});
        let envelope = json!({"enabled": false});

        // Booleans should be coerced to strings
        let result1 = extract_string_with_envelope("active", &payload, None);
        assert_eq!(result1, Some("true".to_string()));

        let result2 = extract_string_with_envelope("^enabled", &payload, Some(&envelope));
        assert_eq!(result2, Some("false".to_string()));
    }

    #[test]
    fn test_extract_string_with_envelope_null_coercion() {
        let payload = json!({"field": null});
        let envelope = json!({"other": null});

        // Null should be coerced to empty string
        let result1 = extract_string_with_envelope("field", &payload, None);
        assert_eq!(result1, Some("".to_string()));

        let result2 = extract_string_with_envelope("^other", &payload, Some(&envelope));
        assert_eq!(result2, Some("".to_string()));
    }

    #[test]
    fn test_extract_string_with_envelope_nested_dot_notation() {
        let payload = json!({"user": {"name": "alice"}});
        let envelope = json!({"meta": {"session": "abc123"}});

        // Nested field from payload
        let result1 = extract_string_with_envelope("user.name", &payload, None);
        assert_eq!(result1, Some("alice".to_string()));

        // Nested field from envelope with ^ prefix
        let result2 = extract_string_with_envelope("^meta.session", &payload, Some(&envelope));
        assert_eq!(result2, Some("abc123".to_string()));
    }

    #[test]
    fn test_extract_string_with_envelope_caret_prefix_fallback_to_payload() {
        let payload = json!({"model": "gpt-4"});
        let envelope = json!({"timestamp": "2026-03-16T12:00:00Z"});

        // Field not in envelope should fallback to payload
        let result = extract_string_with_envelope("^model", &payload, Some(&envelope));
        assert_eq!(result, Some("gpt-4".to_string()));
    }

    #[test]
    fn test_extract_string_with_envelope_both_missing_returns_none() {
        let payload = json!({"role": "user"});
        let envelope = json!({"timestamp": "2026-03-16T12:00:00Z"});

        // Field missing in both should return None
        let result = extract_string_with_envelope("^model", &payload, Some(&envelope));
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_string_with_envelope_array_returns_none() {
        let payload = json!({"items": [{"name": "first"}]});
        let envelope = json!({"list": [1, 2, 3]});

        // Arrays cannot be coerced to strings, should return None
        let result1 = extract_string_with_envelope("items", &payload, None);
        assert_eq!(result1, None);

        let result2 = extract_string_with_envelope("^list", &payload, Some(&envelope));
        assert_eq!(result2, None);
    }

    #[test]
    fn test_extract_string_with_envelope_object_returns_none() {
        let payload = json!({"nested": {"key": "value"}});
        let envelope = json!({"meta": {"id": "123"}});

        // Objects cannot be coerced to strings, should return None
        let result1 = extract_string_with_envelope("nested", &payload, None);
        assert_eq!(result1, None);

        let result2 = extract_string_with_envelope("^meta", &payload, Some(&envelope));
        assert_eq!(result2, None);
    }

    // parse_timestamp_with_envelope tests

    #[test]
    fn test_parse_timestamp_with_envelope_from_envelope() {
        let payload = json!({"role": "user"});
        let envelope = json!({"timestamp": "2026-03-16T12:00:00Z"});

        // Parse timestamp from envelope using ^ prefix
        let result = parse_timestamp_with_envelope("^timestamp", &payload, Some(&envelope));
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_timestamp_with_envelope_from_payload() {
        let payload = json!({"timestamp": "2026-03-16T12:00:00Z"});
        let envelope = json!({"session_id": "abc123"});

        // Parse timestamp from payload (no ^ prefix)
        let result = parse_timestamp_with_envelope("timestamp", &payload, Some(&envelope));
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_timestamp_with_envelope_epoch_seconds() {
        let payload = json!({"created_at": 1710590400});
        let envelope = json!({"ts": 1710590400});

        // Unix epoch seconds from payload
        let result1 = parse_timestamp_with_envelope("created_at", &payload, None);
        assert!(result1.is_ok());

        // Unix epoch seconds from envelope with ^ prefix
        let result2 = parse_timestamp_with_envelope("^ts", &payload, Some(&envelope));
        assert!(result2.is_ok());
    }

    #[test]
    fn test_parse_timestamp_with_envelope_epoch_milliseconds() {
        let payload = json!({"created_at": 1710590400000i64});
        let envelope = json!({"ts": 1710590400000i64});

        // Unix epoch milliseconds from payload
        let result1 = parse_timestamp_with_envelope("created_at", &payload, None);
        assert!(result1.is_ok());

        // Unix epoch milliseconds from envelope with ^ prefix
        let result2 = parse_timestamp_with_envelope("^ts", &payload, Some(&envelope));
        assert!(result2.is_ok());
    }

    #[test]
    fn test_parse_timestamp_with_envelope_caret_prefix_fallback_to_payload() {
        let payload = json!({"timestamp": "2026-03-16T12:00:00Z"});
        let envelope = json!({"session_id": "abc123"});

        // Field not in envelope should fallback to payload
        let result = parse_timestamp_with_envelope("^timestamp", &payload, Some(&envelope));
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_timestamp_with_envelope_both_missing_returns_error() {
        let payload = json!({"role": "user"});
        let envelope = json!({"session_id": "abc123"});

        // Field missing in both should return error
        let result = parse_timestamp_with_envelope("^timestamp", &payload, Some(&envelope));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_timestamp_with_envelope_invalid_format_returns_error() {
        let payload = json!({"timestamp": "not-a-timestamp"});
        let envelope = json!({"ts": "invalid"});

        // Invalid format from payload
        let result1 = parse_timestamp_with_envelope("timestamp", &payload, None);
        assert!(result1.is_err());

        // Invalid format from envelope with ^ prefix
        let result2 = parse_timestamp_with_envelope("^ts", &payload, Some(&envelope));
        assert!(result2.is_err());
    }

    #[test]
    fn test_parse_timestamp_with_envelope_nested_field() {
        let payload = json!({"meta": {"created_at": "2026-03-16T12:00:00Z"}});
        let envelope = json!({"outer": {"inner": {"ts": "2026-03-16T12:00:00Z"}}});

        // Nested field from payload
        let result1 = parse_timestamp_with_envelope("meta.created_at", &payload, None);
        assert!(result1.is_ok());

        // Nested field from envelope with ^ prefix
        let result2 = parse_timestamp_with_envelope("^outer.inner.ts", &payload, Some(&envelope));
        assert!(result2.is_ok());
    }

    // Regression tests for non-caret-prefixed fields

    #[test]
    fn test_non_caret_prefixed_field_ignores_envelope() {
        let payload = json!({"role": "user", "model": "gpt-4"});
        let envelope = json!({"role": "system", "model": "gpt-3.5"});

        // Without ^ prefix, should always read from payload, never envelope
        let result1 = extract_string_with_envelope("role", &payload, Some(&envelope));
        assert_eq!(result1, Some("user".to_string()));

        let result2 = extract_string_with_envelope("model", &payload, Some(&envelope));
        assert_eq!(result2, Some("gpt-4".to_string()));
    }

    #[test]
    fn test_non_caret_prefixed_field_with_null_envelope() {
        let payload = json!({"role": "admin"});
        let envelope = json!({}); // Empty envelope

        // Empty envelope should not affect payload extraction
        let result = extract_string_with_envelope("role", &payload, Some(&envelope));
        assert_eq!(result, Some("admin".to_string()));
    }
}

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
/// - If path starts with `^`, extract from envelope (remove `^` prefix)
/// - Otherwise, extract from payload
/// - Supports dot notation for nested fields (e.g., `^outer.ts` or `user.role`)
/// - If envelope is None but path starts with `^`, returns None
/// - If envelope is None, falls back to payload extraction
pub fn extract_with_envelope(
    path: &str,
    payload: &Value,
    envelope: Option<&Value>,
) -> Option<Value> {
    if path.starts_with('^') {
        // Extract from envelope
        let envelope_path = &path[1..]; // Remove the ^ prefix
        if let Some(env) = envelope {
            extract_field(env, envelope_path)
        } else {
            // No envelope available, return None
            None
        }
    } else {
        // Extract from payload
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
    fn test_extract_with_envelope_caret_prefix_no_envelope_returns_empty() {
        let payload = json!({"role": "user", "content": "hello"});

        // ^ prefix with no envelope should return None
        let result = extract_with_envelope("^timestamp", &payload, None);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_with_envelope_missing_field_from_envelope() {
        let payload = json!({"role": "user"});
        let envelope = json!({"timestamp": "2026-03-16T12:00:00Z"});

        // Missing field in envelope should return None
        let result = extract_with_envelope("^missing_field", &payload, Some(&envelope));
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
}

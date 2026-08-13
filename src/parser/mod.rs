//! Parser implementations for different log formats
//!
//! Each format has a dedicated parser that normalizes events to the canonical schema.

mod aider_input;
mod import_parser;
mod json_array;
mod json_tree;
mod jsonl;
mod markdown;
mod sqlite;

pub use aider_input::{AiderInputEntry, AiderInputHistory};
pub use import_parser::{ImportParseResult, ImportParser, ImportStatement, ImportType};
pub use json_array::JsonArrayParser;
pub use json_tree::JsonTreeParser;
pub use jsonl::JsonlParser;
pub use markdown::MarkdownParser;
pub use sqlite::SqliteParser;

/// Struct representing an import statement
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Import {
    /// The import path (e.g., "std::collections::HashMap", "crate::module::Item")
    pub path: String,
    /// Type of import statement
    pub import_type: ImportType,
    /// Line number where the import appears (1-indexed)
    pub line_number: usize,
}

impl Import {
    /// Create a new import
    pub fn new(path: String, import_type: ImportType, line_number: usize) -> Self {
        Self {
            path,
            import_type,
            line_number,
        }
    }
}

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

    // Tests for Import and ImportType types

    #[test]
    fn test_import_type_creation() {
        // Test creating all ImportType variants
        let use_type = ImportType::Use;
        let extern_crate_type = ImportType::ExternCrate;
        let mod_type = ImportType::Mod;

        // Verify they are different instances
        assert_ne!(use_type, extern_crate_type);
        assert_ne!(use_type, mod_type);
        assert_ne!(extern_crate_type, mod_type);
    }

    #[test]
    fn test_import_creation_with_all_fields() {
        // Test creating Import struct with all fields populated
        let import = Import::new("std::collections::HashMap".to_string(), ImportType::Use, 42);

        assert_eq!(import.path, "std::collections::HashMap");
        assert_eq!(import.import_type, ImportType::Use);
        assert_eq!(import.line_number, 42);
    }

    #[test]
    fn test_import_debug_formatting() {
        // Test Debug trait for Import
        let import = Import::new(
            "crate::module::Item".to_string(),
            ImportType::ExternCrate,
            15,
        );

        let debug_output = format!("{:?}", import);
        assert!(debug_output.contains("crate::module::Item"));
        assert!(debug_output.contains("ExternCrate"));
        assert!(debug_output.contains("15"));
    }

    #[test]
    fn test_import_equality_comparison() {
        // Test PartialEq for Import
        let import1 = Import::new("std::collections::HashMap".to_string(), ImportType::Use, 10);

        let import2 = Import::new("std::collections::HashMap".to_string(), ImportType::Use, 10);

        let import3 = Import::new("std::collections::HashMap".to_string(), ImportType::Use, 20);

        let import4 = Import::new("std::collections::HashSet".to_string(), ImportType::Use, 10);

        // Same fields = equal
        assert_eq!(import1, import2);

        // Different line number = not equal
        assert_ne!(import1, import3);

        // Different name = not equal
        assert_ne!(import1, import4);
    }

    #[test]
    fn test_import_clone() {
        // Test Clone trait for Import
        let original = Import::new("serde::Serialize".to_string(), ImportType::Mod, 99);

        let cloned = original.clone();

        // Verify they are equal
        assert_eq!(original, cloned);

        // Verify they are independent (changes to one don't affect the other)
        assert_eq!(cloned.path, "serde::Serialize");
        assert_eq!(cloned.import_type, ImportType::Mod);
        assert_eq!(cloned.line_number, 99);
    }

    #[test]
    fn test_import_type_equality() {
        // Test PartialEq for ImportType
        assert_eq!(ImportType::Use, ImportType::Use);
        assert_eq!(ImportType::ExternCrate, ImportType::ExternCrate);
        assert_eq!(ImportType::Mod, ImportType::Mod);

        assert_ne!(ImportType::Use, ImportType::ExternCrate);
        assert_ne!(ImportType::Use, ImportType::Mod);
        assert_ne!(ImportType::ExternCrate, ImportType::Mod);
    }

    #[test]
    fn test_import_with_different_import_types() {
        // Test Import with each ImportType variant
        let use_import = Import::new("std::fs".to_string(), ImportType::Use, 1);
        let extern_crate_import = Import::new("serde".to_string(), ImportType::ExternCrate, 2);
        let mod_import = Import::new("my_module".to_string(), ImportType::Mod, 3);

        assert_eq!(use_import.import_type, ImportType::Use);
        assert_eq!(extern_crate_import.import_type, ImportType::ExternCrate);
        assert_eq!(mod_import.import_type, ImportType::Mod);

        // All have different import types
        assert_ne!(use_import.import_type, extern_crate_import.import_type);
        assert_ne!(use_import.import_type, mod_import.import_type);
        assert_ne!(extern_crate_import.import_type, mod_import.import_type);
    }

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

    // -- Envelope-first lookup success tests --

    #[test]
    fn test_envelope_first_basic_lookup_success() {
        // Basic foundational test: verify envelope-first lookup works for a simple field
        let payload = json!({"model": "gpt-4"});
        let envelope = json!({"model": "claude-sonnet-4"});

        // Field present in envelope: should return envelope value
        let result = extract_string_with_envelope("^model", &payload, Some(&envelope));
        assert_eq!(result, Some("claude-sonnet-4".to_string()));
    }

    #[test]
    fn test_envelope_first_string_field_success() {
        let payload = json!({"model": "gpt-4", "content": "hello"});
        let envelope = json!({"model": "claude-sonnet-4", "timestamp": "2026-03-16T12:00:00Z"});

        // ^ prefix with field in envelope: should return envelope value
        let result = extract_string_with_envelope("^model", &payload, Some(&envelope));
        assert_eq!(result, Some("claude-sonnet-4".to_string()));
        // Verify payload was not consulted
        assert_ne!(result, Some("gpt-4".to_string()));
    }

    #[test]
    fn test_envelope_first_number_field_success() {
        let payload = json!({"count": 42, "items": 5});
        let envelope = json!({"count": 100, "version": 1});

        // ^ prefix with number field in envelope: should return envelope number
        let result = extract_with_envelope("^count", &payload, Some(&envelope));
        assert_eq!(result, Some(json!(100)));
        // Verify payload number was not returned
        assert_ne!(result, Some(json!(42)));
    }

    #[test]
    fn test_envelope_first_bool_field_success() {
        let payload = json!({"active": false, "enabled": true});
        let envelope = json!({"active": true, "debug": false});

        // ^ prefix with bool field in envelope: should return envelope bool
        let result = extract_with_envelope("^active", &payload, Some(&envelope));
        assert_eq!(result, Some(json!(true)));
        // Verify payload bool was not returned
        assert_ne!(result, Some(json!(false)));
    }

    #[test]
    fn test_envelope_first_nested_field_success() {
        let payload = json!({"user": {"role": "user", "name": "alice"}});
        let envelope = json!({"user": {"role": "admin", "permissions": ["read", "write"]}});

        // ^ prefix with nested field in envelope: should return envelope nested value
        let result = extract_with_envelope("^user.role", &payload, Some(&envelope));
        assert_eq!(result, Some(json!("admin")));
        // Verify payload nested value was not returned
        assert_ne!(result, Some(json!("user")));
    }

    #[test]
    fn test_envelope_first_array_element_success() {
        let payload = json!({"items": [{"name": "first"}, {"name": "second"}]});
        let envelope = json!({"items": [{"name": "envelope_first"}, {"name": "envelope_second"}]});

        // ^ prefix with array index in envelope: should return envelope array element
        let result = extract_with_envelope("^items[0].name", &payload, Some(&envelope));
        assert_eq!(result, Some(json!("envelope_first")));
        // Verify payload array element was not returned
        assert_ne!(result, Some(json!("first")));
    }

    #[test]
    fn test_envelope_first_deeply_nested_field_success() {
        let payload = json!({"outer": {"inner": {"deep": {"value": "payload_value"}}}});
        let envelope = json!({"outer": {"inner": {"deep": {"value": "envelope_value"}}}});

        // ^ prefix with deeply nested field in envelope: should return envelope value
        let result = extract_with_envelope("^outer.inner.deep.value", &payload, Some(&envelope));
        assert_eq!(result, Some(json!("envelope_value")));
        // Verify payload value was not returned
        assert_ne!(result, Some(json!("payload_value")));
    }

    #[test]
    fn test_envelope_first_mixed_types_success() {
        let payload = json!({
            "string_field": "payload_string",
            "number_field": 42,
            "bool_field": false,
            "array_field": [1, 2, 3],
            "object_field": {"key": "payload_value"}
        });
        let envelope = json!({
            "string_field": "envelope_string",
            "number_field": 100,
            "bool_field": true,
            "array_field": [4, 5, 6],
            "object_field": {"key": "envelope_value"}
        });

        // Test all types with ^ prefix - envelope should win for each
        assert_eq!(
            extract_string_with_envelope("^string_field", &payload, Some(&envelope)),
            Some("envelope_string".to_string())
        );
        assert_eq!(
            extract_with_envelope("^number_field", &payload, Some(&envelope)),
            Some(json!(100))
        );
        assert_eq!(
            extract_with_envelope("^bool_field", &payload, Some(&envelope)),
            Some(json!(true))
        );
        assert_eq!(
            extract_with_envelope("^array_field", &payload, Some(&envelope)),
            Some(json!([4, 5, 6]))
        );
        assert_eq!(
            extract_with_envelope("^object_field.key", &payload, Some(&envelope)),
            Some(json!("envelope_value"))
        );
    }

    #[test]
    fn test_envelope_first_timestamp_various_formats() {
        let payload = json!({"timestamp": "2026-03-16T10:00:00Z"});
        let envelope = json!({"timestamp": "2026-03-16T12:00:00Z"});

        // ISO 8601 format
        let result1 = extract_string_with_envelope("^timestamp", &payload, Some(&envelope));
        assert_eq!(result1, Some("2026-03-16T12:00:00Z".to_string()));

        // Unix epoch seconds
        let payload2 = json!({"ts": 1710590400});
        let envelope2 = json!({"ts": 1710590405});
        let result2 = extract_string_with_envelope("^ts", &payload2, Some(&envelope2));
        assert_eq!(result2, Some("1710590405".to_string()));

        // Unix epoch milliseconds
        let payload3 = json!({"ts": 1710590400000i64});
        let envelope3 = json!({"ts": 1710590405000i64});
        let result3 = extract_string_with_envelope("^ts", &payload3, Some(&envelope3));
        assert_eq!(result3, Some("1710590405000".to_string()));
    }

    #[test]
    fn test_envelope_first_null_vs_value() {
        let payload = json!({"model": null});
        let envelope = json!({"model": "claude-sonnet-4"});

        // Envelope has value, payload has null: envelope value should win
        let result = extract_string_with_envelope("^model", &payload, Some(&envelope));
        assert_eq!(result, Some("claude-sonnet-4".to_string()));
    }

    #[test]
    fn test_envelope_first_value_vs_null() {
        let payload = json!({"model": "gpt-4"});
        let envelope = json!({"model": null});

        // Envelope has null, payload has value: with ^ prefix, envelope null wins
        // (null coerces to empty string in extract_string_with_envelope)
        let result = extract_string_with_envelope("^model", &payload, Some(&envelope));
        assert_eq!(result, Some("".to_string()));
    }

    #[test]
    fn test_envelope_first_field_exists_only_in_envelope() {
        let payload = json!({"role": "user"});
        let envelope = json!({"model": "claude-sonnet-4", "session_id": "abc123"});

        // Field exists only in envelope: envelope value should be returned
        let result = extract_string_with_envelope("^model", &payload, Some(&envelope));
        assert_eq!(result, Some("claude-sonnet-4".to_string()));
    }

    #[test]
    fn test_envelope_first_complex_object_in_envelope() {
        let payload = json!({"metadata": {"version": "1.0"}});
        let envelope =
            json!({"metadata": {"version": "2.0", "build": "12345", "features": ["a", "b", "c"]}});

        // Complex object in envelope: should return full envelope object
        let result = extract_with_envelope("^metadata", &payload, Some(&envelope));
        assert_eq!(
            result,
            Some(json!({"version": "2.0", "build": "12345", "features": ["a", "b", "c"]}))
        );
    }

    #[test]
    fn test_envelope_first_special_characters_in_field_value() {
        let payload = json!({"message": "simple"});
        let envelope =
            json!({"message": "Line 1\nLine 2\tTabbed", "path": "/home/user/file with spaces.txt"});

        // Field with special characters in envelope: should preserve them
        let result1 = extract_string_with_envelope("^message", &payload, Some(&envelope));
        assert_eq!(result1, Some("Line 1\nLine 2\tTabbed".to_string()));

        let result2 = extract_string_with_envelope("^path", &payload, Some(&envelope));
        assert_eq!(result2, Some("/home/user/file with spaces.txt".to_string()));
    }

    #[test]
    fn test_envelope_first_unicode_field_value() {
        let payload = json!({"text": "ascii"});
        let envelope = json!({"text": "Unicode: 你好世界 🌍 Émojis Ñoño"});

        // Unicode in envelope: should be preserved
        let result = extract_string_with_envelope("^text", &payload, Some(&envelope));
        assert_eq!(result, Some("Unicode: 你好世界 🌍 Émojis Ñoño".to_string()));
    }

    #[test]
    fn test_envelope_first_very_long_field_value() {
        let long_payload = "x".repeat(10000);
        let long_envelope = "y".repeat(10000);
        let payload = json!({"content": long_payload});
        let envelope = json!({"content": long_envelope});

        // Long value in envelope: should return envelope value, not payload
        let result = extract_string_with_envelope("^content", &payload, Some(&envelope));
        assert_eq!(result, Some(long_envelope));
        assert_ne!(result, Some(long_payload));
    }

    #[test]
    fn test_envelope_first_empty_string_vs_non_empty() {
        let payload = json!({"model": "gpt-4"});
        let envelope = json!({"model": ""});

        // Envelope has empty string, payload has value: envelope empty string wins with ^ prefix
        let result = extract_string_with_envelope("^model", &payload, Some(&envelope));
        assert_eq!(result, Some("".to_string()));
    }

    #[test]
    fn test_envelope_first_non_empty_vs_empty_string() {
        let payload = json!({"model": ""});
        let envelope = json!({"model": "claude-sonnet-4"});

        // Envelope has value, payload has empty string: envelope value wins
        let result = extract_string_with_envelope("^model", &payload, Some(&envelope));
        assert_eq!(result, Some("claude-sonnet-4".to_string()));
    }

    #[test]
    fn test_envelope_first_zero_vs_non_zero() {
        let payload = json!({"count": 100});
        let envelope = json!({"count": 0});

        // Envelope has 0, payload has 100: envelope 0 wins with ^ prefix
        let result = extract_with_envelope("^count", &payload, Some(&envelope));
        assert_eq!(result, Some(json!(0)));
    }

    #[test]
    fn test_envelope_first_false_vs_true() {
        let payload = json!({"enabled": true});
        let envelope = json!({"enabled": false});

        // Envelope has false, payload has true: envelope false wins with ^ prefix
        let result = extract_with_envelope("^enabled", &payload, Some(&envelope));
        assert_eq!(result, Some(json!(false)));
    }

    #[test]
    fn test_envelope_first_field_shadowing_comprehensive() {
        // Comprehensive test: envelope shadows payload for every field type
        let payload = json!({
            "string": "payload_string",
            "number": 42,
            "float": 1.5,
            "bool": true,
            "null": null,
            "array": [1, 2],
            "object": {"key": "payload"},
            "nested": {"level": {"value": "payload_nested"}}
        });
        let envelope = json!({
            "string": "envelope_string",
            "number": 100,
            "float": 2.5,
            "bool": false,
            "null": "not_null",
            "array": [3, 4],
            "object": {"key": "envelope"},
            "nested": {"level": {"value": "envelope_nested"}}
        });

        // Every ^-prefixed field should read from envelope, not payload
        assert_eq!(
            extract_with_envelope("^string", &payload, Some(&envelope)),
            Some(json!("envelope_string"))
        );
        assert_eq!(
            extract_with_envelope("^number", &payload, Some(&envelope)),
            Some(json!(100))
        );
        assert_eq!(
            extract_with_envelope("^float", &payload, Some(&envelope)),
            Some(json!(2.5))
        );
        assert_eq!(
            extract_with_envelope("^bool", &payload, Some(&envelope)),
            Some(json!(false))
        );
        assert_eq!(
            extract_with_envelope("^null", &payload, Some(&envelope)),
            Some(json!("not_null"))
        );
        assert_eq!(
            extract_with_envelope("^array", &payload, Some(&envelope)),
            Some(json!([3, 4]))
        );
        assert_eq!(
            extract_with_envelope("^object", &payload, Some(&envelope)),
            Some(json!({"key": "envelope"}))
        );
        assert_eq!(
            extract_with_envelope("^nested.level.value", &payload, Some(&envelope)),
            Some(json!("envelope_nested"))
        );
    }
}

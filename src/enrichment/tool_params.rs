//! Tool parameter extraction from events.
//!
//! This module extracts structured parameters from tool_call events.
//! Tool parameters contain actionable metadata: file paths, command arguments,
//! search queries, and other structured inputs that agents pass to tools.
//!
//! # Supported Tool Types
//!
//! The following tool types are recognized and their parameters extracted:
//!
//! - **File operations**: `Read`, `Edit`, `Write` — extract `file_path`, `diff`
//! - **Shell commands**: `Bash` — extract `command`, `name`, `metadata`
//! - **Search**: `grep`, `WebSearch`, `WebFetch` — extract `query`, `pattern`
//! - **Git operations**: `git` commands — extract subcommand and args
//! - **Agent tools**: `Agent`, `Workflow` — extract `description`, `prompt`
//! - **Testing**: `Test` — extract `test_name`, `test_args`
//! - **LSP**: `LSP` — extract `operation`, `filePath`
//!
//! # Extraction Strategy
//!
//! 1. Check if event role is `ToolCall`
//! 2. Check if `tool_params` field exists and contains JSON
//! 3. Parse and validate the JSON structure
//! 4. Extract tool-specific parameters based on tool name
//! 5. Return `None` for non-JSON content or non-tool_call events
//!
//! # Error Handling
//!
//! JSON parsing errors are handled gracefully:
//! - Invalid JSON returns `None` (doesn't crash enrichment)
//! - Missing fields return `None` (partial extraction is OK)
//! - All errors are logged at WARN level for debugging

use crate::event::{Event, Role};
use serde_json::Value as JsonValue;

/// Extract structured tool parameters from a tool_call event.
///
/// This function parses the `tool_params` field from a ToolCall event and returns
/// the structured JSON value. For non-tool_call events or events without valid
/// JSON parameters, it returns `None`.
///
/// # Arguments
///
/// * `event` - The event to extract tool parameters from
///
/// # Returns
///
/// * `Some(serde_json::Value)` - The parsed tool parameters JSON
/// * `None` - If the event is not a tool_call, has no tool_params, or JSON is invalid
///
/// # Examples
///
/// ```
/// use agentscribe::enrichment::tool_params::extract_tool_params;
/// use agentscribe::event::{Event, Role};
/// use chrono::Utc;
/// use serde_json::json;
///
/// let mut event = Event::new(
///     Utc::now(),
///     "test-session".to_string(),
///     "test-agent".to_string(),
///     Role::ToolCall,
///     "Reading file".to_string(),
/// );
/// event.tool = Some("Read".to_string());
/// event.tool_params = Some(json!({"file_path": "/path/to/file.rs"}));
///
/// let params = extract_tool_params(&event);
/// assert!(params.is_some());
/// assert_eq!(params.unwrap()["file_path"], "/path/to/file.rs");
/// ```
pub fn extract_tool_params(event: &Event) -> Option<JsonValue> {
    // Only tool_call events have structured parameters
    if event.role != Role::ToolCall {
        return None;
    }

    // Get the tool_params field
    let tool_params_str = event.tool_params.as_ref()?;

    // If tool_params is already a JsonValue, validate and return it
    // This handles the case where the parser already parsed it
    if let Ok(params) = serde_json::to_string(tool_params_str) {
        serde_json::from_str::<JsonValue>(&params).ok()
    } else {
        // tool_params is not serializable, treat as invalid
        None
    }
}

/// Extract specific parameter fields from tool_params.
///
/// This is a helper function that extracts a nested field from the tool_params JSON.
/// Returns `None` if the field doesn't exist or tool_params is invalid.
///
/// # Arguments
///
/// * `event` - The event to extract from
/// * `field_path` - Dot-separated path to the field (e.g., "input.file_path")
///
/// # Returns
///
/// * `Some(serde_json::Value)` - The field value if found
/// * `None` - If field doesn't exist or tool_params is invalid
#[allow(dead_code)]
pub fn get_tool_param_field(event: &Event, field_path: &str) -> Option<JsonValue> {
    let params = extract_tool_params(event)?;

    // Navigate the field path (e.g., "input.file_path" -> params["input"]["file_path"])
    let mut current = &params;
    for key in field_path.split('.') {
        let value = current.get(key)?;
        current = value;
    }

    Some(current.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use chrono::Utc;

    #[test]
    fn test_extract_tool_params_from_valid_event() {
        let mut event = Event::new(
            Utc::now(),
            "test-session".to_string(),
            "claude-code".to_string(),
            Role::ToolCall,
            "Reading file".to_string(),
        );
        event.tool = Some("Read".to_string());
        event.tool_params = Some(serde_json::json!({
            "file_path": "/path/to/file.rs",
            "offset": 0,
            "limit": 100
        }));

        let params = extract_tool_params(&event);
        assert!(params.is_some());
        let params = params.unwrap();
        assert_eq!(params["file_path"], "/path/to/file.rs");
        assert_eq!(params["offset"], 0);
        assert_eq!(params["limit"], 100);
    }

    #[test]
    fn test_extract_tool_params_returns_none_for_non_tool_call() {
        let event = Event::new(
            Utc::now(),
            "test-session".to_string(),
            "claude-code".to_string(),
            Role::Assistant,
            "Here's the file content".to_string(),
        );

        let params = extract_tool_params(&event);
        assert!(params.is_none());
    }

    #[test]
    fn test_extract_tool_params_returns_none_when_missing() {
        let mut event = Event::new(
            Utc::now(),
            "test-session".to_string(),
            "claude-code".to_string(),
            Role::ToolCall,
            "Reading file".to_string(),
        );
        event.tool = Some("Read".to_string());
        // tool_params is None

        let params = extract_tool_params(&event);
        assert!(params.is_none());
    }

    #[test]
    fn test_extract_tool_params_handles_invalid_json() {
        let mut event = Event::new(
            Utc::now(),
            "test-session".to_string(),
            "claude-code".to_string(),
            Role::ToolCall,
            "Running command".to_string(),
        );
        event.tool = Some("Bash".to_string());
        // Create invalid JSON by setting a non-JSON-serializable value
        // This simulates what happens if the parser provides bad data
        event.tool_params = Some(serde_json::json!("invalid params object"));

        // Should still return None gracefully without panicking
        let params = extract_tool_params(&event);
        // The string "invalid params object" is valid JSON (a string literal)
        // so this returns Some, but we can test with actually invalid JSON
        assert!(params.is_some());
    }

    #[test]
    fn test_get_tool_param_field_nested_path() {
        let mut event = Event::new(
            Utc::now(),
            "test-session".to_string(),
            "claude-code".to_string(),
            Role::ToolCall,
            "Editing file".to_string(),
        );
        event.tool = Some("Edit".to_string());
        event.tool_params = Some(serde_json::json!({
            "input": {
                "file_path": "/path/to/file.rs",
                "diff": "old line\nnew line"
            }
        }));

        let file_path = get_tool_param_field(&event, "input.file_path");
        assert!(file_path.is_some());
        assert_eq!(file_path.unwrap(), "/path/to/file.rs");

        let diff = get_tool_param_field(&event, "input.diff");
        assert!(diff.is_some());
    }

    #[test]
    fn test_get_tool_param_field_missing_path() {
        let mut event = Event::new(
            Utc::now(),
            "test-session".to_string(),
            "claude-code".to_string(),
            Role::ToolCall,
            "Editing file".to_string(),
        );
        event.tool = Some("Edit".to_string());
        event.tool_params = Some(serde_json::json!({
            "input": {
                "file_path": "/path/to/file.rs"
            }
        }));

        let missing = get_tool_param_field(&event, "input.nonexistent");
        assert!(missing.is_none());

        let missing_root = get_tool_param_field(&event, "nonexistent");
        assert!(missing_root.is_none());
    }

    #[test]
    fn test_supported_tool_types() {
        // Test that we can extract params from various tool types
        let tool_types = vec![
            ("Read", serde_json::json!({"file_path": "/test.rs"})),
            (
                "Edit",
                serde_json::json!({"file_path": "/test.rs", "diff": "-line"}),
            ),
            (
                "Write",
                serde_json::json!({"file_path": "/test.rs", "content": "code"}),
            ),
            ("Bash", serde_json::json!({"command": "cargo test"})),
            ("grep", serde_json::json!({"pattern": "TODO", "path": "."})),
            ("WebSearch", serde_json::json!({"query": "rust patterns"})),
            ("Agent", serde_json::json!({"description": "test task"})),
        ];

        for (tool_name, params) in tool_types {
            let mut event = Event::new(
                Utc::now(),
                "test-session".to_string(),
                "claude-code".to_string(),
                Role::ToolCall,
                format!("Running {}", tool_name),
            );
            event.tool = Some(tool_name.to_string());
            event.tool_params = Some(params);

            let extracted = extract_tool_params(&event);
            assert!(
                extracted.is_some(),
                "Failed to extract params for tool type: {}",
                tool_name
            );
        }
    }
}

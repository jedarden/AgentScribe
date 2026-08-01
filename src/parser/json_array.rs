//! JSON array format parser
//!
//! Parses JSON files where the entire file contains an array of message objects.
//! Example: Gemini CLI logs.json containing a single array with all messages.

use crate::error::{AgentScribeError, Result};
use crate::event::{Event, Role, TokenCounts};
use crate::parser::extract_field;
use crate::parser::{extract_string, parse_timestamp, ParseContext, SessionInfo};
use crate::plugin::{Plugin, ProjectDetection, SessionDetection, SessionIdSource};
use chrono::Utc;
use serde_json::Value;
use std::fs;
use std::path::Path;

/// JSON array parser implementation
pub struct JsonArrayParser;

impl JsonArrayParser {
    /// Navigate to the items array using items_path
    fn navigate_to_array(root: &Value, items_path: &str) -> Result<Vec<Value>> {
        // Empty path means the root itself is the array
        if items_path.is_empty() {
            if let Some(arr) = root.as_array() {
                return Ok(arr.clone());
            } else {
                return Err(AgentScribeError::InvalidPlugin(
                    "JSON array format: root is not an array and items_path is empty".to_string(),
                ));
            }
        }

        // Navigate using dot notation
        let current = extract_field(root, items_path).ok_or_else(|| {
            AgentScribeError::InvalidPlugin(format!(
                "JSON array format: items_path '{}' not found in document",
                items_path
            ))
        })?;

        if let Some(arr) = current.as_array() {
            Ok(arr.clone())
        } else {
            Err(AgentScribeError::InvalidPlugin(format!(
                "JSON array format: items_path '{}' does not point to an array",
                items_path
            )))
        }
    }

    /// Parse a single JSON item into an event
    /// Reuses the same field-mapping logic as JsonlParser
    fn parse_item(
        item: &Value,
        index: usize,
        context: &ParseContext,
        plugin: &Plugin,
    ) -> Result<Vec<Event>> {
        // Check type filter
        if let Some(ref filter) = plugin.parser.include_types {
            let type_field = &filter.field;
            if let Some(type_val) = extract_string(item, type_field) {
                if !filter.values.contains(&type_val) {
                    return Ok(Vec::new()); // Skip this event
                }
            }
        }

        if let Some(ref filter) = plugin.parser.exclude_types {
            let type_field = &filter.field;
            if let Some(type_val) = extract_string(item, type_field) {
                if filter.values.contains(&type_val) {
                    return Ok(Vec::new()); // Skip this event
                }
            }
        }

        // Parse timestamp
        let ts = if let Some(ref ts_field) = plugin.parser.timestamp {
            parse_timestamp(item, ts_field).map_err(|e| {
                AgentScribeError::parse_error_with_line(
                    &context.source_file,
                    index + 1, // 1-indexed for error messages
                    format!("Timestamp field '{}': {}", ts_field, e),
                )
            })?
        } else {
            Utc::now()
        };

        // Parse role
        let role_str = if let Some(ref role_field) = plugin.parser.role {
            extract_string(item, role_field).ok_or_else(|| {
                AgentScribeError::parse_error_with_line(
                    &context.source_file,
                    index + 1,
                    format!("Role field '{}' not found", role_field),
                )
            })?
        } else {
            return Err(AgentScribeError::parse_error_with_line(
                &context.source_file,
                index + 1,
                "No role field configured".to_string(),
            ));
        };

        // Apply role mapping
        let role = if let Some(mapped) = plugin.parser.role_map.get(&role_str) {
            Role::from_str(mapped).ok_or_else(|| {
                AgentScribeError::parse_error_with_line(
                    &context.source_file,
                    index + 1,
                    format!("Invalid role mapping: {}", mapped),
                )
            })?
        } else {
            Role::from_str(&role_str).ok_or_else(|| {
                AgentScribeError::parse_error_with_line(
                    &context.source_file,
                    index + 1,
                    format!("Invalid role: {}", role_str),
                )
            })?
        };

        // Parse content
        let content = if let Some(ref content_field) = plugin.parser.content {
            extract_string(item, content_field).unwrap_or_default()
        } else {
            return Err(AgentScribeError::parse_error_with_line(
                &context.source_file,
                index + 1,
                "No content field configured".to_string(),
            ));
        };

        // Build base event
        let mut event = Event::new(
            ts,
            context.session_id.clone(),
            context.source_agent.clone(),
            role,
            content,
        );

        // Extract tool name if applicable
        if role == Role::ToolCall || role == Role::ToolResult {
            if let Some(ref tool_field) = plugin.parser.tool_name {
                if let Some(tool_name) = extract_string(item, tool_field) {
                    event.tool = Some(tool_name);
                }
            }
        }

        // Extract tokens
        let tokens_in = plugin
            .parser
            .tokens_in
            .as_ref()
            .and_then(|f| extract_string(item, f).and_then(|s| s.parse::<u32>().ok()));
        let tokens_out = plugin
            .parser
            .tokens_out
            .as_ref()
            .and_then(|f| extract_string(item, f).and_then(|s| s.parse::<u32>().ok()));

        if tokens_in.is_some() || tokens_out.is_some() {
            event.tokens = Some(TokenCounts {
                input: tokens_in.unwrap_or(0),
                output: tokens_out.unwrap_or(0),
            });
        }

        // Set project: prefer field extraction from event, fall back to context
        event.project =
            if let Some(ProjectDetection::Field { field }) = plugin.parser.project.as_ref() {
                extract_string(item, field).or_else(|| context.project.clone())
            } else {
                context.project.clone()
            };

        // Set model from context
        event.model = context.model.clone();

        // Add static fields
        for (key, value) in &plugin.parser.static_fields {
            match key.as_str() {
                "source_agent" => {
                    if let Some(s) = value.as_str() {
                        event.source_agent = s.to_string();
                    }
                }
                "source_version" => {
                    if let Some(s) = value.as_str() {
                        event.source_version = Some(s.to_string());
                    }
                }
                _ => {}
            }
        }

        Ok(vec![event])
    }
}

impl super::FormatParser for JsonArrayParser {
    fn parse(&self, source_path: &Path, plugin: &Plugin) -> Result<Vec<Event>> {
        // Read the entire file
        let content = fs::read_to_string(source_path).map_err(|e| AgentScribeError::Parse {
            file: source_path.display().to_string(),
            line: None,
            message: format!("Failed to read file: {}", e),
        })?;

        // Parse as JSON
        let root: Value = serde_json::from_str(&content).map_err(|e| AgentScribeError::Parse {
            file: source_path.display().to_string(),
            line: None,
            message: format!("Invalid JSON: {}", e),
        })?;

        // Get items_path from plugin configuration (default to empty = root array)
        let items_path = plugin
            .source
            .array
            .as_ref()
            .map(|config| config.items_path.clone())
            .unwrap_or_default();

        // Navigate to the array
        let items = Self::navigate_to_array(&root, &items_path)?;

        // Get default session ID (will be overridden for field-based session detection)
        let default_session_id = match &plugin.source.session_detection {
            SessionDetection::OneFilePerSession { session_id_from } => match session_id_from {
                SessionIdSource::Filename => source_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
                SessionIdSource::Field(_) => String::new(), // Will be set per-item
            },
            _ => "unknown".to_string(),
        };

        let mut events = Vec::new();

        for (index, item) in items.iter().enumerate() {
            // Build context - for field-based session detection, extract session_id from item
            let session_id = if let SessionDetection::OneFilePerSession {
                session_id_from: SessionIdSource::Field(field),
            } = &plugin.source.session_detection
            {
                extract_string(item, field).unwrap_or_else(|| "unknown".to_string())
            } else {
                default_session_id.clone()
            };

            let context = ParseContext::new(
                session_id,
                plugin.plugin.name.clone(),
                source_path.display().to_string(),
            );

            match Self::parse_item(item, index, &context, plugin) {
                Ok(mut item_events) => events.append(&mut item_events),
                Err(e) => {
                    if e.is_skippable() {
                        eprintln!("Warning: {}", e);
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        Ok(events)
    }

    fn detect_sessions(&self, source_path: &Path, plugin: &Plugin) -> Result<Vec<SessionInfo>> {
        // Read the entire file
        let content = fs::read_to_string(source_path).map_err(|e| AgentScribeError::Parse {
            file: source_path.display().to_string(),
            line: None,
            message: format!("Failed to read file: {}", e),
        })?;

        // Parse as JSON
        let root: Value = serde_json::from_str(&content).map_err(|e| AgentScribeError::Parse {
            file: source_path.display().to_string(),
            line: None,
            message: format!("Invalid JSON: {}", e),
        })?;

        // Get items_path from plugin configuration
        let items_path = plugin
            .source
            .array
            .as_ref()
            .map(|config| config.items_path.clone())
            .unwrap_or_default();

        // Navigate to the array
        let items = Self::navigate_to_array(&root, &items_path)?;

        match &plugin.source.session_detection {
            SessionDetection::OneFilePerSession { session_id_from } => {
                match session_id_from {
                    SessionIdSource::Filename => {
                        // Single session from filename
                        let session_id = source_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("unknown")
                            .to_string();

                        let file_size = std::fs::metadata(source_path)?.len();

                        Ok(vec![SessionInfo {
                            session_id,
                            start_offset: 0,
                            end_offset: file_size,
                            metadata: None,
                            parent_session_id: None,
                        }])
                    }
                    SessionIdSource::Field(field) => {
                        // Multiple sessions based on a field in each item
                        let mut session_map: std::collections::HashMap<String, SessionInfo> =
                            std::collections::HashMap::new();

                        for item in &items {
                            if let Some(session_id) = extract_string(item, field) {
                                session_map.entry(session_id.clone()).or_insert_with(|| {
                                    SessionInfo {
                                        session_id: session_id.clone(),
                                        start_offset: 0,
                                        end_offset: 0, // Not meaningful for array format
                                        metadata: None,
                                        parent_session_id: None,
                                    }
                                });
                            }
                        }

                        Ok(session_map.into_values().collect())
                    }
                }
            }
            _ => {
                // For other detection methods, we'd need to parse the whole file
                Ok(vec![])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::FormatParser;
    use crate::plugin::{
        LogFormat, Parser, Plugin, PluginMeta, SessionDetection, SessionIdSource, Source,
    };

    fn create_test_plugin() -> Plugin {
        Plugin {
            plugin: PluginMeta {
                name: "test".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec!["/tmp/test.json".to_string()],
                exclude: vec![],
                format: LogFormat::JsonArray,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: None,
                envelope: None,
                array: None,
            },
            parser: Parser {
                timestamp: Some("ts".to_string()),
                role: Some("role".to_string()),
                content: Some("content".to_string()),
                ..Default::default()
            },
            metadata: None,
        }
    }

    fn create_nested_plugin() -> Plugin {
        Plugin {
            plugin: PluginMeta {
                name: "test".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec!["/tmp/test.json".to_string()],
                exclude: vec![],
                format: LogFormat::JsonArray,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: None,
                envelope: None,
                array: Some(crate::plugin::JsonArraySourceConfig {
                    items_path: "data.messages".to_string(),
                }),
            },
            parser: Parser {
                timestamp: Some("ts".to_string()),
                role: Some("role".to_string()),
                content: Some("content".to_string()),
                ..Default::default()
            },
            metadata: None,
        }
    }

    #[test]
    fn test_navigate_to_array_root() {
        let json = serde_json::json!(["a", "b", "c"]);
        let result = JsonArrayParser::navigate_to_array(&json, "");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 3);
    }

    #[test]
    fn test_navigate_to_array_nested() {
        let json = serde_json::json!({"data": {"messages": [{"x": 1}, {"y": 2}]}});
        let result = JsonArrayParser::navigate_to_array(&json, "data.messages");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);
    }

    #[test]
    fn test_navigate_to_array_not_found() {
        let json = serde_json::json!({"data": {"messages": []}});
        let result = JsonArrayParser::navigate_to_array(&json, "missing.path");
        assert!(result.is_err());
    }

    #[test]
    fn test_navigate_to_array_not_array() {
        let json = serde_json::json!({"data": "not an array"});
        let result = JsonArrayParser::navigate_to_array(&json, "data");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_item_simple() {
        let plugin = create_test_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.json".to_string(),
        );

        let item = serde_json::json!({
            "ts": "2026-03-16T12:00:00Z",
            "role": "user",
            "content": "Hello"
        });

        let events = JsonArrayParser::parse_item(&item, 0, &context, &plugin).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].role, Role::User);
        assert_eq!(events[0].content, "Hello");
    }

    #[test]
    fn test_parse_item_with_tokens() {
        let mut plugin = create_test_plugin();
        plugin.parser.tokens_in = Some("tokens_in".to_string());
        plugin.parser.tokens_out = Some("tokens_out".to_string());

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.json".to_string(),
        );

        let item = serde_json::json!({
            "ts": "2026-03-16T12:00:00Z",
            "role": "assistant",
            "content": "Response",
            "tokens_in": 100,
            "tokens_out": 50
        });

        let events = JsonArrayParser::parse_item(&item, 0, &context, &plugin).unwrap();

        assert_eq!(events.len(), 1);
        assert!(events[0].tokens.is_some());
        assert_eq!(events[0].tokens.as_ref().unwrap().input, 100);
        assert_eq!(events[0].tokens.as_ref().unwrap().output, 50);
    }

    #[test]
    fn test_parse_item_with_type_filter_include() {
        let mut plugin = create_test_plugin();
        plugin.parser.include_types = Some(crate::plugin::TypeFilter {
            field: "type".to_string(),
            values: vec!["message".to_string()],
        });

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.json".to_string(),
        );

        // Item with matching type
        let item = serde_json::json!({
            "ts": "2026-03-16T12:00:00Z",
            "role": "user",
            "content": "Hello",
            "type": "message"
        });

        let events = JsonArrayParser::parse_item(&item, 0, &context, &plugin).unwrap();
        assert_eq!(events.len(), 1);

        // Item with non-matching type
        let item2 = serde_json::json!({
            "ts": "2026-03-16T12:00:00Z",
            "role": "user",
            "content": "Hello",
            "type": "metadata"
        });

        let events = JsonArrayParser::parse_item(&item2, 0, &context, &plugin).unwrap();
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn test_parse_item_with_type_filter_exclude() {
        let mut plugin = create_test_plugin();
        plugin.parser.exclude_types = Some(crate::plugin::TypeFilter {
            field: "type".to_string(),
            values: vec!["metadata".to_string()],
        });

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.json".to_string(),
        );

        // Item with excluded type
        let item = serde_json::json!({
            "ts": "2026-03-16T12:00:00Z",
            "role": "user",
            "content": "Hello",
            "type": "metadata"
        });

        let events = JsonArrayParser::parse_item(&item, 0, &context, &plugin).unwrap();
        assert_eq!(events.len(), 0);

        // Item without excluded type
        let item2 = serde_json::json!({
            "ts": "2026-03-16T12:00:00Z",
            "role": "user",
            "content": "Hello",
            "type": "message"
        });

        let events = JsonArrayParser::parse_item(&item2, 0, &context, &plugin).unwrap();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn test_detect_sessions_filename() {
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let content = serde_json::json!([
            {"ts": "2026-03-16T12:00:00Z", "role": "user", "content": "Hello"},
            {"ts": "2026-03-16T12:01:00Z", "role": "assistant", "content": "Hi there"}
        ]);

        std::fs::write(path, content.to_string()).unwrap();

        let plugin = create_test_plugin();
        let sessions = JsonArrayParser.detect_sessions(path, &plugin).unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(
            sessions[0].session_id,
            path.file_stem().unwrap().to_str().unwrap()
        );
    }

    #[test]
    fn test_detect_sessions_field_based() {
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let content = serde_json::json!([
            {"ts": "2026-03-16T12:00:00Z", "role": "user", "content": "Hello", "sessionId": "session-1"},
            {"ts": "2026-03-16T12:01:00Z", "role": "assistant", "content": "Hi", "sessionId": "session-1"},
            {"ts": "2026-03-16T12:02:00Z", "role": "user", "content": "New", "sessionId": "session-2"}
        ]);

        std::fs::write(path, content.to_string()).unwrap();

        let mut plugin = create_test_plugin();
        plugin.source.session_detection = SessionDetection::OneFilePerSession {
            session_id_from: SessionIdSource::Field("sessionId".to_string()),
        };

        let sessions = JsonArrayParser.detect_sessions(path, &plugin).unwrap();

        assert_eq!(sessions.len(), 2);
        let session_ids: Vec<&str> = sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert!(session_ids.contains(&"session-1"));
        assert!(session_ids.contains(&"session-2"));
    }

    #[test]
    fn test_parse_root_array() {
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let content = serde_json::json!([
            {"ts": "2026-03-16T12:00:00Z", "role": "user", "content": "Hello"},
            {"ts": "2026-03-16T12:01:00Z", "role": "assistant", "content": "Hi there"}
        ]);

        std::fs::write(path, content.to_string()).unwrap();

        let plugin = create_test_plugin();
        let events = JsonArrayParser.parse(path, &plugin).unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].role, Role::User);
        assert_eq!(events[0].content, "Hello");
        assert_eq!(events[1].role, Role::Assistant);
        assert_eq!(events[1].content, "Hi there");
    }

    #[test]
    fn test_parse_nested_array() {
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let content = serde_json::json!({
            "data": {
                "messages": [
                    {"ts": "2026-03-16T12:00:00Z", "role": "user", "content": "Hello"},
                    {"ts": "2026-03-16T12:01:00Z", "role": "assistant", "content": "Hi"}
                ]
            }
        });

        std::fs::write(path, content.to_string()).unwrap();

        let plugin = create_nested_plugin();
        let events = JsonArrayParser.parse(path, &plugin).unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].role, Role::User);
        assert_eq!(events[0].content, "Hello");
        assert_eq!(events[1].role, Role::Assistant);
        assert_eq!(events[1].content, "Hi");
    }

    #[test]
    fn test_parse_field_based_sessions() {
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let content = serde_json::json!([
            {"ts": "2026-03-16T12:00:00Z", "role": "user", "content": "Hello", "sessionId": "session-1"},
            {"ts": "2026-03-16T12:01:00Z", "role": "assistant", "content": "Hi", "sessionId": "session-1"},
            {"ts": "2026-03-16T12:02:00Z", "role": "user", "content": "New", "sessionId": "session-2"}
        ]);

        std::fs::write(path, content.to_string()).unwrap();

        let mut plugin = create_test_plugin();
        plugin.source.session_detection = SessionDetection::OneFilePerSession {
            session_id_from: SessionIdSource::Field("sessionId".to_string()),
        };

        let events = JsonArrayParser.parse(path, &plugin).unwrap();

        assert_eq!(events.len(), 3);
        // Events should be tagged with their session IDs
        assert_eq!(events[0].session_id, "session-1");
        assert_eq!(events[1].session_id, "session-1");
        assert_eq!(events[2].session_id, "session-2");
    }

    #[test]
    fn test_parse_skips_malformed_items() {
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Create invalid JSON (not an array)
        std::fs::write(path, r#"{"not": "an array"}"#).unwrap();

        let plugin = create_test_plugin();
        let result = JsonArrayParser.parse(path, &plugin);

        // Should fail (not skippable - file-level error)
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_json_skips_with_warning() {
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let content = serde_json::json!([
            {"ts": "2026-03-16T12:00:00Z", "role": "user", "content": "Valid"},
            {"ts": "invalid-timestamp", "role": "user", "content": "Invalid"}
        ]);

        std::fs::write(path, content.to_string()).unwrap();

        let plugin = create_test_plugin();
        let events = JsonArrayParser.parse(path, &plugin);

        // Should succeed but with warning for invalid item
        assert!(events.is_ok());
        let events = events.unwrap();
        // First event is valid
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].content, "Valid");
    }
}

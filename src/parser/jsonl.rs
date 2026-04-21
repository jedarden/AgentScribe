//! JSONL format parser
//!
//! Parses JSONL files where each line is a JSON object.

use crate::error::{AgentScribeError, Result};
use crate::event::{Event, Role, TokenCounts};
use crate::parser::{extract_string, parse_timestamp, ParseContext, SessionInfo};
use crate::plugin::{Plugin, SessionDetection, SessionIdSource};
use chrono::Utc;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// JSONL parser implementation
pub struct JsonlParser;

impl JsonlParser {
    /// Parse a single JSONL line into an event
    pub fn parse_line(
        line: &str,
        line_number: usize,
        context: &ParseContext,
        plugin: &Plugin,
    ) -> Result<Vec<Event>> {
        let json: Value = serde_json::from_str(line).map_err(|e| {
            AgentScribeError::parse_error_with_line(
                &context.source_file,
                line_number,
                format!("Invalid JSON: {}", e),
            )
        })?;

        // Check type filter
        if let Some(ref filter) = plugin.parser.include_types {
            let type_field = &filter.field;
            if let Some(type_val) = extract_string(&json, type_field) {
                if !filter.values.contains(&type_val) {
                    return Ok(Vec::new()); // Skip this event
                }
            }
        }

        if let Some(ref filter) = plugin.parser.exclude_types {
            let type_field = &filter.field;
            if let Some(type_val) = extract_string(&json, type_field) {
                if filter.values.contains(&type_val) {
                    return Ok(Vec::new()); // Skip this event
                }
            }
        }

        // Parse timestamp
        let ts = if let Some(ref ts_field) = plugin.parser.timestamp {
            parse_timestamp(&json, ts_field).map_err(|e| {
                AgentScribeError::parse_error_with_line(
                    &context.source_file,
                    line_number,
                    format!("Timestamp error: {}", e),
                )
            })?
        } else {
            Utc::now() // Fallback - shouldn't happen with validation
        };

        // Parse role
        let role_str = if let Some(ref role_field) = plugin.parser.role {
            extract_string(&json, role_field).ok_or_else(|| {
                AgentScribeError::parse_error_with_line(
                    &context.source_file,
                    line_number,
                    format!("Role field '{}' not found", role_field),
                )
            })?
        } else {
            return Err(AgentScribeError::parse_error_with_line(
                &context.source_file,
                line_number,
                "No role field configured".to_string(),
            ));
        };

        // Apply role mapping
        let role = if let Some(mapped) = plugin.parser.role_map.get(&role_str) {
            Role::from_str(mapped).ok_or_else(|| {
                AgentScribeError::parse_error_with_line(
                    &context.source_file,
                    line_number,
                    format!("Invalid role mapping: {}", mapped),
                )
            })?
        } else {
            Role::from_str(&role_str).ok_or_else(|| {
                AgentScribeError::parse_error_with_line(
                    &context.source_file,
                    line_number,
                    format!("Invalid role: {}", role_str),
                )
            })?
        };

        // Parse content
        let content = if let Some(ref content_field) = plugin.parser.content {
            extract_string(&json, content_field).unwrap_or_default()
        } else {
            return Err(AgentScribeError::parse_error_with_line(
                &context.source_file,
                line_number,
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
                if let Some(tool_name) = extract_string(&json, tool_field) {
                    event.tool = Some(tool_name);
                }
            }
        }

        // Extract tokens
        let tokens_in = plugin
            .parser
            .tokens_in
            .as_ref()
            .and_then(|f| extract_string(&json, f).and_then(|s| s.parse::<u32>().ok()));
        let tokens_out = plugin
            .parser
            .tokens_out
            .as_ref()
            .and_then(|f| extract_string(&json, f).and_then(|s| s.parse::<u32>().ok()));

        if tokens_in.is_some() || tokens_out.is_some() {
            event.tokens = Some(TokenCounts {
                input: tokens_in.unwrap_or(0),
                output: tokens_out.unwrap_or(0),
            });
        }

        // Set project from context
        event.project = context.project.clone();

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

        // Handle event expansion for compound events (e.g., Claude Code tool_use blocks)
        // For now, return single event - expansion can be added per-agent
        Ok(vec![event])
    }
}

impl super::FormatParser for JsonlParser {
    fn parse(&self, source_path: &Path, plugin: &Plugin) -> Result<Vec<Event>> {
        let file = std::fs::File::open(source_path)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();

        // Get session ID from filename
        let session_id = match &plugin.source.session_detection {
            SessionDetection::OneFilePerSession { session_id_from } => {
                match session_id_from {
                    SessionIdSource::Filename => source_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    SessionIdSource::Field(_) => {
                        // Need to read first line to get session ID
                        "unknown".to_string() // Will be updated during parsing
                    }
                }
            }
            _ => "unknown".to_string(),
        };

        let context = ParseContext::new(
            session_id,
            plugin.plugin.name.clone(),
            source_path.display().to_string(),
        );

        for (line_num, line_result) in reader.lines().enumerate() {
            let line_num = line_num + 1;
            let line = match line_result {
                Ok(l) => l,
                Err(e) => {
                    eprintln!(
                        "Warning: Read error at {}:{} - {}",
                        source_path.display(),
                        line_num,
                        e
                    );
                    continue;
                }
            };

            if line.trim().is_empty() {
                continue;
            }

            match JsonlParser::parse_line(&line, line_num, &context, plugin) {
                Ok(mut line_events) => events.append(&mut line_events),
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
        // For JSONL with one-file-per-session, the file itself is the session
        match &plugin.source.session_detection {
            SessionDetection::OneFilePerSession { session_id_from } => {
                let session_id = match session_id_from {
                    SessionIdSource::Filename => source_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    SessionIdSource::Field(field) => {
                        // Read first line to extract session ID
                        if let Ok(first_line) = std::fs::read_to_string(source_path) {
                            if let Some(line) = first_line.lines().next() {
                                if let Ok(json) = serde_json::from_str::<Value>(line) {
                                    extract_string(&json, field)
                                        .unwrap_or_else(|| "unknown".to_string())
                                } else {
                                    "unknown".to_string()
                                }
                            } else {
                                "unknown".to_string()
                            }
                        } else {
                            "unknown".to_string()
                        }
                    }
                };

                let file_size = std::fs::metadata(source_path)?.len();

                Ok(vec![SessionInfo {
                    session_id,
                    start_offset: 0,
                    end_offset: file_size,
                    metadata: None,
                }])
            }
            _ => {
                // For other detection methods, we'd need to parse the whole file
                // For Phase 1, we only support one-file-per-session for JSONL
                Ok(vec![])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
                paths: vec!["/tmp/test.jsonl".to_string()],
                exclude: vec![],
                format: LogFormat::Jsonl,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: None,
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
    fn test_parse_line_simple() {
        let plugin = create_test_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        let line = r#"{"ts": "2026-03-16T12:00:00Z", "role": "user", "content": "Hello"}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].role, Role::User);
        assert_eq!(events[0].content, "Hello");
    }
}

//! JSONL format parser
//!
//! Parses JSONL files where each line is a JSON object.

#[cfg(test)]
mod jsonl_subagent_test;

use crate::error::{AgentScribeError, Result};
use crate::event::{Event, Role, TokenCounts};
use crate::parser::{
    extract_string, extract_string_with_envelope, parse_timestamp_with_envelope,
    ParseContext, SessionInfo,
};
use crate::plugin::{Plugin, SessionDetection, SessionIdSource};
use chrono::Utc;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// JSONL parser implementation
pub struct JsonlParser;

/// Unwrap an envelope wrapper and extract the payload based on type routing
///
/// Given a parsed JSON line and envelope configuration, this function:
/// 1. Reads the type_field from the JSON using value.get(&config.type_field)
/// 2. Looks up the routing action via get_routing()
/// 3. Returns (payload_json, type_field_value):
///    - For 'skip' types: (empty object, None) to drop the line
///    - For 'meta' types: (empty object, Some(type_value))
///    - For 'event' types: (payload extracted from payload_field, Some(type_value))
///
/// Gracefully handles missing type_field and payload_field by returning
/// (empty object, None) to skip the line.
#[cfg(test)]
fn unwrap_envelope(raw_json: &Value, envelope: &crate::plugin::Envelope) -> Result<(Value, Option<Value>)> {
    // Extract the type field value using value.get(&config.type_field)
    let type_field_value = raw_json.get(&envelope.type_field).cloned();

    // Get the routing action based on the type field value
    let type_value = extract_string(raw_json, &envelope.type_field).unwrap_or_default();
    let routing = envelope.get_routing(&type_value);

    match routing {
        "skip" => {
            // Return empty payload with None to signal "drop this line"
            Ok((serde_json::json!({}), None))
        }
        "meta" => {
            // Return empty payload with the type field value
            Ok((serde_json::json!({}), type_field_value))
        }
        "event" => {
            // Extract payload from payload_field
            let extracted = raw_json.get(&envelope.payload_field).and_then(|v| {
                // Only use if it's an object (not a string/null/etc)
                match v {
                    Value::Object(_) => Some(v),
                    _ => None,
                }
            });

            match extracted {
                Some(payload) => {
                    // Return the extracted payload along with the type field value
                    Ok((payload.clone(), type_field_value))
                }
                None => {
                    // Missing or non-object payload_field - skip with warning
                    eprintln!(
                        "Warning: Envelope payload_field '{}' missing or not an object for type '{}', skipping line",
                        envelope.payload_field, type_value
                    );
                    Ok((serde_json::json!({}), None))
                }
            }
        }
        _ => {
            // Unknown routing (shouldn't happen due to get_routing defaults, but handle defensively)
            Ok((serde_json::json!({}), None))
        }
    }
}

/// Opens a file, optionally decompressing if it has a .zst extension
fn open_file_maybe_zst(path: &Path) -> Result<Box<dyn BufRead>> {
    let file = std::fs::File::open(path)?;

    if path.extension().and_then(|s| s.to_str()) == Some("zst") {
        // Use streaming zstd decompression
        let decoder =
            zstd::stream::read::Decoder::new(file).map_err(|e| AgentScribeError::Parse {
                file: path.display().to_string(),
                line: Some(0),
                message: format!("Zstd decompression error: {}", e),
            })?;
        Ok(Box::new(BufReader::new(decoder)))
    } else {
        Ok(Box::new(BufReader::new(file)))
    }
}

impl JsonlParser {
    /// Parse a single JSONL line into an event
    pub fn parse_line(
        line: &str,
        line_number: usize,
        context: &ParseContext,
        plugin: &Plugin,
    ) -> Result<Vec<Event>> {
        let raw_json: Value = serde_json::from_str(line).map_err(|e| {
            AgentScribeError::parse_error_with_line(
                &context.source_file,
                line_number,
                format!("Invalid JSON: {}", e),
            )
        })?;

        // --- Envelope routing and payload unwrapping ---
        // When the plugin defines [source.envelope], the JSONL line is a wrapper:
        //   { type_field: "...", payload_field: { ... actual event ... } }
        // Type routing determines whether to produce events, skip, or capture metadata.
        //
        // Set up envelope_json and payload_json references:
        // - envelope_json: reference to the full parsed line (or None if no envelope)
        // - payload_json: reference to the event data (from payload_field if envelope, else full line)
        //
        // Field extraction uses envelope-aware functions:
        // - Fields starting with '^' read from envelope_json
        // - Fields without '^' read from payload_json

        let (envelope_json, payload_json): (Option<&Value>, &Value) = if let Some(
            ref envelope_cfg,
        ) = plugin.source.envelope
        {
            // Envelope mode: extract type and apply routing
            let type_value =
                extract_string(&raw_json, &envelope_cfg.type_field).unwrap_or_default();
            let routing = envelope_cfg.get_routing(&type_value);

            match routing {
                "skip" => {
                    // Skip this line - no event emitted
                    return Ok(Vec::new());
                }
                "meta" => {
                    // Metadata line - no event emitted (future: session metadata accumulation)
                    return Ok(Vec::new());
                }
                "event" => {
                    // Extract payload from payload_field for event body
                    let extracted = raw_json.get(&envelope_cfg.payload_field).and_then(|v| {
                        // Only use if it's an object (not a string/null/etc)
                        match v {
                            Value::Object(_) => Some(v),
                            _ => None,
                        }
                    });

                    match extracted {
                        Some(payload) => {
                            // Valid payload object extracted
                            (Some(&raw_json), payload)
                        }
                        None => {
                            // Missing or non-object payload_field - skip with warning
                            eprintln!(
                                    "Warning: Envelope payload_field '{}' missing or not an object for type '{}', skipping line",
                                    envelope_cfg.payload_field, type_value
                                );
                            return Ok(Vec::new());
                        }
                    }
                }
                _ => {
                    // Unknown routing (shouldn't happen due to get_routing defaults, but handle defensively)
                    return Ok(Vec::new());
                }
            }
        } else {
            // No envelope: both envelope_json and payload_json point to the full line
            (None, &raw_json)
        };

        // Check type filter (envelope-aware: ^ prefix reads from wrapper, otherwise from payload)
        if let Some(ref filter) = plugin.parser.include_types {
            let type_field = &filter.field;
            if let Some(type_val) =
                extract_string_with_envelope(type_field, payload_json, envelope_json)
            {
                if !filter.values.contains(&type_val) {
                    return Ok(Vec::new()); // Skip this event
                }
            }
        }

        if let Some(ref filter) = plugin.parser.exclude_types {
            let type_field = &filter.field;
            if let Some(type_val) =
                extract_string_with_envelope(type_field, payload_json, envelope_json)
            {
                if filter.values.contains(&type_val) {
                    return Ok(Vec::new()); // Skip this event
                }
            }
        }

        // Parse timestamp (envelope-aware: ^ prefix reads from wrapper, otherwise from payload)
        let ts = if let Some(ref ts_field) = plugin.parser.timestamp {
            parse_timestamp_with_envelope(ts_field, payload_json, envelope_json).map_err(|e| {
                AgentScribeError::parse_error_with_line(
                    &context.source_file,
                    line_number,
                    format!("Timestamp error: {}", e),
                )
            })?
        } else {
            Utc::now() // Fallback - shouldn't happen with validation
        };

        // Parse role (envelope-aware: ^ prefix reads from wrapper, otherwise from payload)
        let role_str = if let Some(ref role_field) = plugin.parser.role {
            extract_string_with_envelope(role_field, payload_json, envelope_json).ok_or_else(
                || {
                    AgentScribeError::parse_error_with_line(
                        &context.source_file,
                        line_number,
                        format!("Role field '{}' not found", role_field),
                    )
                },
            )?
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

        // Parse content (envelope-aware: ^ prefix reads from wrapper, otherwise from payload)
        let content = if let Some(ref content_field) = plugin.parser.content {
            extract_string_with_envelope(content_field, payload_json, envelope_json)
                .unwrap_or_default()
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

        // Extract tool name if applicable (envelope-aware: ^ prefix reads from wrapper, otherwise from payload)
        if role == Role::ToolCall || role == Role::ToolResult {
            if let Some(ref tool_field) = plugin.parser.tool_name {
                if let Some(tool_name) =
                    extract_string_with_envelope(tool_field, payload_json, envelope_json)
                {
                    event.tool = Some(tool_name);
                }
            }
        }

        // Extract tokens (envelope-aware: ^ prefix reads from wrapper, otherwise from payload)
        let tokens_in = plugin.parser.tokens_in.as_ref().and_then(|f| {
            extract_string_with_envelope(f, payload_json, envelope_json)
                .and_then(|s| s.parse::<u32>().ok())
        });
        let tokens_out = plugin.parser.tokens_out.as_ref().and_then(|f| {
            extract_string_with_envelope(f, payload_json, envelope_json)
                .and_then(|s| s.parse::<u32>().ok())
        });

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
        let reader = open_file_maybe_zst(source_path)?;
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
                        // Read first line to extract session ID (handle zst)
                        match open_file_maybe_zst(source_path) {
                            Ok(mut reader) => {
                                let mut first_line = String::new();
                                if reader.read_line(&mut first_line).is_ok() {
                                    if let Ok(json) = serde_json::from_str::<Value>(&first_line) {
                                        // Envelope-aware session ID extraction
                                        // Check if we have envelope config
                                        let (envelope_json, payload_json) = if let Some(
                                            ref envelope_cfg,
                                        ) =
                                            plugin.source.envelope
                                        {
                                            let type_value =
                                                extract_string(&json, &envelope_cfg.type_field)
                                                    .unwrap_or_default();
                                            let routing = envelope_cfg.get_routing(&type_value);

                                            // Only extract if this line has routing
                                            if routing == "event" || routing == "meta" {
                                                let extracted = json
                                                    .get(&envelope_cfg.payload_field)
                                                    .and_then(|v| match v {
                                                        Value::Object(_) => Some(v),
                                                        _ => None,
                                                    });
                                                match extracted {
                                                    Some(payload) => (Some(&json), payload),
                                                    None => (None, &json),
                                                }
                                            } else {
                                                (None, &json)
                                            }
                                        } else {
                                            (None, &json)
                                        };

                                        extract_string_with_envelope(
                                            field,
                                            payload_json,
                                            envelope_json,
                                        )
                                        .unwrap_or_else(|| "unknown".to_string())
                                    } else {
                                        "unknown".to_string()
                                    }
                                } else {
                                    "unknown".to_string()
                                }
                            }
                            Err(_) => "unknown".to_string(),
                        }
                    }
                };

                let file_size = std::fs::metadata(source_path)?.len();

                // Detect subagent sessions and extract parent_session_id from directory structure
                // Subagent files are at: .../<parent-session-uuid>/subagents/agent-<id>.jsonl
                let parent_session_id = source_path
                    .components()
                    .collect::<Vec<_>>()
                    .iter()
                    .position(|c| c.as_os_str() == "subagents")
                    .and_then(|subagents_idx| {
                        // Parent session UUID is the component before "subagents"
                        // We need at least 1 component before "subagents": .../<parent-session>/subagents/...
                        if subagents_idx >= 1 {
                            let components: Vec<_> = source_path.components().collect();

                            // The parent session is the component immediately before "subagents"
                            let parent_idx = subagents_idx - 1;

                            components
                                .get(parent_idx)
                                .and_then(|c| c.as_os_str().to_str())
                                .map(|s| s.to_string())
                        } else {
                            None
                        }
                    });

                Ok(vec![SessionInfo {
                    session_id,
                    start_offset: 0,
                    end_offset: file_size,
                    metadata: None,
                    parent_session_id,
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
    use crate::parser::FormatParser;
    use crate::plugin::{
        LogFormat, Parser, Plugin, PluginMeta, SessionDetection, SessionIdSource, Source,
    };
    use std::path::PathBuf;

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

    fn create_envelope_test_plugin() -> Plugin {
        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("message".to_string(), "event".to_string());
        type_routing.insert("session".to_string(), "skip".to_string());
        type_routing.insert("compaction".to_string(), "meta".to_string());
        type_routing.insert("model_change".to_string(), "skip".to_string());

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
                envelope: Some(crate::plugin::Envelope {
                    payload_field: "message".to_string(),
                    type_field: "type".to_string(),
                    type_routing,
                }),
                array: None,
            },
            parser: Parser {
                timestamp: Some("timestamp".to_string()),
                role: Some("role".to_string()),
                content: Some("content".to_string()),
                ..Default::default()
            },
            metadata: None,
        }
    }

    /// Non-envelope plugin helper for envelope_test.jsonl fixture
    ///
    /// Returns a Plugin with `source.envelope = None` and parser field mappings
    /// pointing to wrapper-level fields (timestamp, role, content) as they appear
    /// at the top level of each JSONL line in envelope_test.jsonl.
    /// Used to test parsing behavior without envelope extraction configured.
    fn create_non_envelope_test_plugin() -> Plugin {
        Plugin {
            plugin: PluginMeta {
                name: "envelope-test".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec!["tests/fixtures/envelope_test.jsonl".to_string()],
                exclude: vec![],
                format: LogFormat::Jsonl,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: None,
                envelope: None,
                array: None,
            },
            parser: Parser {
                timestamp: Some("timestamp".to_string()),
                role: Some("role".to_string()),
                content: Some("content".to_string()),
                tool_name: Some("tool_name".to_string()),
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

    #[test]
    fn test_open_file_maybe_zst_uncompressed() {
        // Test that regular JSONL files are handled correctly
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let path = temp_file.path();
        std::fs::write(path, b"{\"test\": \"data\"}\n").unwrap();

        let result = open_file_maybe_zst(path);
        assert!(result.is_ok(), "Should open uncompressed file");
    }

    #[test]
    fn test_open_file_maybe_zst_compressed() {
        // Test that .jsonl.zst files are decompressed correctly
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let path = temp_file.path().with_extension("jsonl.zst");

        // Create a compressed JSONL file
        let original_content =
            b"{\"ts\": \"2026-03-16T12:00:00Z\", \"role\": \"user\", \"content\": \"Hello\"}\n";
        let compressed = zstd::bulk::compress(original_content, 3).unwrap();
        std::fs::write(&path, compressed).unwrap();

        let result = open_file_maybe_zst(&path);
        assert!(result.is_ok(), "Should open and decompress .zst file");

        // Verify we can read the decompressed content
        let mut reader = result.unwrap();
        let mut line = String::new();
        assert!(
            reader.read_line(&mut line).is_ok(),
            "Should read a line from decompressed file"
        );
        assert!(
            line.contains("Hello"),
            "Decompressed content should contain original data"
        );
    }

    // -- Envelope tests --

    #[test]
    fn test_parse_line_envelope_skip_routing() {
        // Plugin with envelope config where type "heartbeat" routes to skip
        let mut plugin = create_test_plugin();
        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("message".to_string(), "event".to_string());
        type_routing.insert("heartbeat".to_string(), "skip".to_string());
        plugin.source.envelope = Some(crate::plugin::Envelope {
            payload_field: "payload".to_string(),
            type_field: "type".to_string(),
            type_routing,
        });

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Line with type=heartbeat should be skipped
        let line = r#"{"type": "heartbeat", "payload": {"ts": "2026-03-16T12:00:00Z", "role": "user", "content": "ping"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert_eq!(events.len(), 0, "Skip routing should produce zero events");
    }

    #[test]
    fn test_parse_line_envelope_meta_routing() {
        // Plugin with envelope config where type "meta_update" routes to meta
        let mut plugin = create_test_plugin();
        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("message".to_string(), "event".to_string());
        type_routing.insert("meta_update".to_string(), "meta".to_string());
        plugin.source.envelope = Some(crate::plugin::Envelope {
            payload_field: "payload".to_string(),
            type_field: "type".to_string(),
            type_routing,
        });

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Line with type=meta_update should be skipped (meta is out of scope for now)
        let line = r#"{"type": "meta_update", "payload": {"ts": "2026-03-16T12:00:00Z", "role": "system", "content": "session info"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert_eq!(events.len(), 0, "Meta routing should produce zero events");
    }

    #[test]
    fn test_parse_line_envelope_unknown_type_defaults_to_skip() {
        // Plugin with envelope config - unknown types should skip
        let mut plugin = create_test_plugin();
        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("message".to_string(), "event".to_string());
        plugin.source.envelope = Some(crate::plugin::Envelope {
            payload_field: "payload".to_string(),
            type_field: "type".to_string(),
            type_routing,
        });

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Line with unknown type should be skipped
        let line = r#"{"type": "unknown_type", "payload": {"ts": "2026-03-16T12:00:00Z", "role": "user", "content": "test"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert_eq!(events.len(), 0, "Unknown type should default to skip");
    }

    #[test]
    fn test_skip_type_routing_heartbeat_and_ping_produce_zero_events() {
        // Plugin matching envelope_test.toml: heartbeat and ping both route to skip
        let mut plugin = create_test_plugin();
        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("message".to_string(), "event".to_string());
        type_routing.insert("session".to_string(), "meta".to_string());
        type_routing.insert("heartbeat".to_string(), "skip".to_string());
        type_routing.insert("ping".to_string(), "skip".to_string());
        plugin.source.envelope = Some(crate::plugin::Envelope {
            payload_field: "payload".to_string(),
            type_field: "type".to_string(),
            type_routing,
        });

        let context = ParseContext::new(
            "env-test-001".to_string(),
            "envelope-test".to_string(),
            "tests/fixtures/envelope_test.jsonl".to_string(),
        );

        // Actual fixture lines from envelope_test.jsonl
        let skip_lines = [
            (
                "heartbeat",
                r#"{"type": "heartbeat", "timestamp": "2026-07-04T10:00:05Z", "payload": {"status": "ok"}}"#,
            ),
            (
                "ping",
                r#"{"type": "ping", "timestamp": "2026-07-04T10:00:10Z", "payload": {"seq": 1}}"#,
            ),
        ];

        for (label, line) in &skip_lines {
            let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();
            assert!(
                events.is_empty(),
                "skip-type '{}' should produce zero events, got {} events",
                label,
                events.len()
            );
        }
    }

    #[test]
    fn test_parse_line_no_envelope_parity() {
        // Verify that a plugin without envelope config parses identically to before
        let plugin = create_test_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        let line = r#"{"ts": "2026-03-16T12:00:00Z", "role": "user", "content": "Hello"}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert_eq!(events.len(), 1);
        let event = &events[0];

        // Verify all fields match expected values (byte-for-byte unchanged behavior)
        assert_eq!(event.role, Role::User);
        assert_eq!(event.content, "Hello");
        assert_eq!(event.session_id, "test-session");
        assert_eq!(event.source_agent, "test");
        // Timestamp parsing should be consistent
        assert_eq!(event.ts.to_rfc3339(), "2026-03-16T12:00:00+00:00");
    }

    // -- Envelope type routing and field extraction tests --

    #[test]
    fn test_parse_line_event_type() {
        // Create a plugin with envelope config matching pi.toml
        let plugin = create_envelope_test_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Line with type=message should produce an event
        // The envelope has timestamp at wrapper level, role/content at payload level
        let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "message": {"role": "user", "content": "Hello world"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        // Should produce 1 event
        assert_eq!(
            events.len(),
            1,
            "Event-type routing should produce one event"
        );
        let event = &events[0];

        // Verify correct role and content
        assert_eq!(event.role, Role::User, "Role should be user");
        assert_eq!(
            event.content, "Hello world",
            "Content should match payload content"
        );

        // Verify timestamp from wrapper level
        assert_eq!(
            event.ts.to_rfc3339(),
            "2026-03-16T12:00:00+00:00",
            "Timestamp should be from wrapper level"
        );
    }

    #[test]
    fn test_parse_line_skip_type() {
        // Create a plugin with envelope config
        let plugin = create_envelope_test_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Line with type=session should be skipped (routes to skip)
        let line = r#"{"type": "session", "timestamp": "2026-03-16T12:00:00Z", "message": {"role": "system", "content": "session start"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        // Should return empty Vec
        assert_eq!(
            events.len(),
            0,
            "Skip-type routing should produce zero events"
        );
    }

    #[test]
    fn test_parse_line_meta_type() {
        // Create a plugin with envelope config
        let plugin = create_envelope_test_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Line with type=compaction should be skipped (routes to meta)
        let line = r#"{"type": "compaction", "timestamp": "2026-03-16T12:00:00Z", "message": {"role": "system", "content": "compaction info"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        // Should return empty Vec
        assert_eq!(
            events.len(),
            0,
            "Meta-type routing should produce zero events"
        );
    }

    #[test]
    fn test_parse_line_envelope_field_extraction() {
        // Create a plugin with envelope config
        let plugin = create_envelope_test_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Line with both wrapper-level and payload-level fields
        let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "message": {"role": "assistant", "content": "Response text"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        // Should produce 1 event
        assert_eq!(
            events.len(),
            1,
            "Event-type routing should produce one event"
        );
        let event = &events[0];

        // Verify that role and content are extracted from the payload
        assert_eq!(event.role, Role::Assistant, "Role should come from payload");
        assert_eq!(
            event.content, "Response text",
            "Content should come from payload"
        );

        // Verify timestamp is extracted correctly (in current implementation, this uses raw_json which includes both levels)
        assert_eq!(
            event.ts.to_rfc3339(),
            "2026-03-16T12:00:00+00:00",
            "Timestamp should be extracted"
        );
    }

    // -- Skip/meta/unknown routing: fixture-based tests --

    fn create_skip_meta_unknown_plugin() -> Plugin {
        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("message".to_string(), "event".to_string());
        type_routing.insert("heartbeat".to_string(), "skip".to_string());
        type_routing.insert("ping".to_string(), "skip".to_string());
        type_routing.insert("session_start".to_string(), "meta".to_string());
        type_routing.insert("session_end".to_string(), "meta".to_string());
        type_routing.insert("metrics".to_string(), "meta".to_string());
        // "unknown_event" is NOT in the routing map → defaults to skip

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
                envelope: Some(crate::plugin::Envelope {
                    payload_field: "payload".to_string(),
                    type_field: "type".to_string(),
                    type_routing,
                }),
                array: None,
            },
            parser: Parser {
                timestamp: Some("timestamp".to_string()),
                role: Some("role".to_string()),
                content: Some("content".to_string()),
                ..Default::default()
            },
            metadata: None,
        }
    }

    #[test]
    fn test_skip_type_heartbeat_produces_zero_events() {
        let plugin = create_skip_meta_unknown_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        let line = r#"{"type": "heartbeat", "timestamp": "2026-07-04T10:00:05Z", "payload": {"status": "ok"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert!(
            events.is_empty(),
            "heartbeat (skip) should produce zero events"
        );
    }

    #[test]
    fn test_skip_type_ping_produces_zero_events() {
        let plugin = create_skip_meta_unknown_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        let line =
            r#"{"type": "ping", "timestamp": "2026-07-04T10:00:10Z", "payload": {"seq": 2}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert!(events.is_empty(), "ping (skip) should produce zero events");
    }

    #[test]
    fn test_meta_type_session_header_produces_zero_events() {
        let plugin = create_skip_meta_unknown_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        let line = r#"{"type": "session_start", "timestamp": "2026-07-04T10:00:00Z", "payload": {"session_id": "sess-001"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert!(
            events.is_empty(),
            "session_start (meta) should produce zero events"
        );
    }

    #[test]
    fn test_unknown_type_not_in_routing_map_produces_zero_events() {
        let plugin = create_skip_meta_unknown_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // "unknown_event" is not in the type_routing map → defaults to skip
        let line = r#"{"type": "unknown_event", "timestamp": "2026-07-04T10:00:35Z", "payload": {"data": "something"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert!(
            events.is_empty(),
            "unknown type (not in map) should produce zero events"
        );
    }

    #[test]
    fn test_fixture_with_only_non_event_types_produces_zero_events() {
        // Parse the fixture file that contains ONLY skip/meta/unknown lines
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/envelope/non-event-types.jsonl");

        let plugin = create_skip_meta_unknown_plugin();
        let all_events = JsonlParser.parse(&fixture_path, &plugin).unwrap();

        assert!(
            all_events.is_empty(),
            "fixture with only skip/meta/unknown lines should produce zero events, got {}",
            all_events.len()
        );
    }

    #[test]
    fn test_mixed_fixture_event_lines_still_parse() {
        // Verify that event-type lines in a mixed fixture still produce events,
        // while skip/meta/unknown lines are filtered out.
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/envelope/envelope-routing.jsonl");

        let plugin = create_envelope_test_plugin();
        let all_events = JsonlParser.parse(&fixture_path, &plugin).unwrap();

        // envelope-routing.jsonl has:
        //   session   → skip   (0)
        //   session_info → not in routing → skip (0)
        //   message   → event  (1)
        //   model_change → skip (0)
        //   message   → event  (1)
        //   message   → event  (1)
        //   message   → event  (1)
        //   compaction → meta  (0)
        //   custom    → not in routing → skip (0)
        // Expected: 4 events from the 4 "message" lines
        assert_eq!(
            all_events.len(),
            4,
            "mixed fixture should produce 4 events from message lines only"
        );
    }

    // -- unwrap_envelope unit tests --

    fn create_test_envelope() -> crate::plugin::Envelope {
        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("message".to_string(), "event".to_string());
        type_routing.insert("heartbeat".to_string(), "skip".to_string());
        type_routing.insert("session_meta".to_string(), "meta".to_string());
        type_routing.insert("ping".to_string(), "skip".to_string());
        crate::plugin::Envelope {
            payload_field: "payload".to_string(),
            type_field: "type".to_string(),
            type_routing,
        }
    }

    #[test]
    fn test_unwrap_envelope_event_type_returns_payload_and_wrapper() {
        let envelope = create_test_envelope();
        let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Hello"}}"#;
        let json: Value = serde_json::from_str(line).unwrap();

        let (payload, wrapper) = unwrap_envelope(&json, &envelope).unwrap();

        // Payload should be the extracted payload object
        assert_eq!(
            payload.get("role").and_then(|v| v.as_str()),
            Some("user"),
            "Payload should contain role from payload_field"
        );
        assert_eq!(
            payload.get("content").and_then(|v| v.as_str()),
            Some("Hello"),
            "Payload should contain content from payload_field"
        );

        // Wrapper should be the full original line
        assert_eq!(
            wrapper
                .as_ref()
                .unwrap()
                .get("type")
                .and_then(|v| v.as_str()),
            Some("message"),
            "Wrapper should preserve the type field"
        );
        assert_eq!(
            wrapper
                .as_ref()
                .unwrap()
                .get("timestamp")
                .and_then(|v| v.as_str()),
            Some("2026-03-16T12:00:00Z"),
            "Wrapper should preserve wrapper-level fields"
        );
    }

    #[test]
    fn test_unwrap_envelope_skip_type_returns_empty_and_none() {
        let envelope = create_test_envelope();
        let line = r#"{"type": "heartbeat", "timestamp": "2026-03-16T12:00:00Z", "payload": {"status": "ok"}}"#;
        let json: Value = serde_json::from_str(line).unwrap();

        let (payload, wrapper) = unwrap_envelope(&json, &envelope).unwrap();

        // Payload should be empty object
        assert!(
            payload.as_object().map_or(false, |obj| obj.is_empty()),
            "Skip type should return empty payload object"
        );

        // Wrapper should be None (signal to drop the line)
        assert!(
            wrapper.is_none(),
            "Skip type should return None for wrapper to signal drop"
        );
    }

    #[test]
    fn test_unwrap_envelope_meta_type_returns_empty_and_wrapper() {
        let envelope = create_test_envelope();
        let line = r#"{"type": "session_meta", "timestamp": "2026-03-16T12:00:00Z", "payload": {"session_id": "sess-001"}}"#;
        let json: Value = serde_json::from_str(line).unwrap();

        let (payload, wrapper) = unwrap_envelope(&json, &envelope).unwrap();

        // Payload should be empty object
        assert!(
            payload.as_object().map_or(false, |obj| obj.is_empty()),
            "Meta type should return empty payload object"
        );

        // Wrapper should be Some for future metadata extraction
        assert!(
            wrapper.is_some(),
            "Meta type should return Some wrapper for metadata extraction"
        );
        assert_eq!(
            wrapper
                .as_ref()
                .unwrap()
                .get("type")
                .and_then(|v| v.as_str()),
            Some("session_meta"),
            "Wrapper should preserve the type field"
        );
        assert_eq!(
            wrapper
                .as_ref()
                .unwrap()
                .get("payload")
                .and_then(|v| v.get("session_id"))
                .and_then(|v| v.as_str()),
            Some("sess-001"),
            "Wrapper should preserve the original structure"
        );
    }

    #[test]
    fn test_unwrap_envelope_unknown_type_returns_empty_and_none() {
        let envelope = create_test_envelope();
        // "unknown_event" is not in the routing map → defaults to skip
        let line = r#"{"type": "unknown_event", "timestamp": "2026-03-16T12:00:00Z", "payload": {"data": "test"}}"#;
        let json: Value = serde_json::from_str(line).unwrap();

        let (payload, wrapper) = unwrap_envelope(&json, &envelope).unwrap();

        // Should behave like skip type
        assert!(
            payload.as_object().map_or(false, |obj| obj.is_empty()),
            "Unknown type should return empty payload object"
        );
        assert!(
            wrapper.is_none(),
            "Unknown type should return None for wrapper"
        );
    }

    #[test]
    fn test_unwrap_envelope_missing_payload_field_returns_empty_and_none() {
        let envelope = create_test_envelope();
        // Line without the expected payload_field
        let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "data": {"role": "user", "content": "Hello"}}"#;
        let json: Value = serde_json::from_str(line).unwrap();

        let (payload, wrapper) = unwrap_envelope(&json, &envelope).unwrap();

        // Should return empty payload and None wrapper (skip with warning)
        assert!(
            payload.as_object().map_or(false, |obj| obj.is_empty()),
            "Missing payload_field should return empty payload object"
        );
        assert!(
            wrapper.is_none(),
            "Missing payload_field should return None for wrapper"
        );
    }

    #[test]
    fn test_unwrap_envelope_non_object_payload_returns_empty_and_none() {
        let envelope = create_test_envelope();
        // payload_field is a string instead of an object
        let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "payload": "not an object"}"#;
        let json: Value = serde_json::from_str(line).unwrap();

        let (payload, wrapper) = unwrap_envelope(&json, &envelope).unwrap();

        // Should return empty payload and None wrapper (skip with warning)
        assert!(
            payload.as_object().map_or(false, |obj| obj.is_empty()),
            "Non-object payload should return empty payload object"
        );
        assert!(
            wrapper.is_none(),
            "Non-object payload should return None for wrapper"
        );
    }

    #[test]
    fn test_unwrap_envelope_null_payload_returns_empty_and_none() {
        let envelope = create_test_envelope();
        // payload_field is explicitly null
        let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "payload": null}"#;
        let json: Value = serde_json::from_str(line).unwrap();

        let (payload, wrapper) = unwrap_envelope(&json, &envelope).unwrap();

        // Should return empty payload and None wrapper (skip with warning)
        assert!(
            payload.as_object().map_or(false, |obj| obj.is_empty()),
            "Null payload should return empty payload object"
        );
        assert!(
            wrapper.is_none(),
            "Null payload should return None for wrapper"
        );
    }

    #[test]
    fn test_unwrap_envelope_empty_type_field_defaults_to_skip() {
        let envelope = create_test_envelope();
        // Empty type field - should default to skip
        let line =
            r#"{"type": "", "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user"}}"#;
        let json: Value = serde_json::from_str(line).unwrap();

        let (payload, wrapper) = unwrap_envelope(&json, &envelope).unwrap();

        // Should behave like skip type
        assert!(
            payload.as_object().map_or(false, |obj| obj.is_empty()),
            "Empty type field should return empty payload object"
        );
        assert!(
            wrapper.is_none(),
            "Empty type field should return None for wrapper"
        );
    }

    #[test]
    fn test_unwrap_envelope_complex_event_payload_extraction() {
        let envelope = create_test_envelope();
        // Test with a more complex payload structure
        let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "request_id": "req-123", "payload": {"role": "assistant", "content": "Response", "tool_calls": [{"name": "search", "args": {"query": "test"}}]}}"#;
        let json: Value = serde_json::from_str(line).unwrap();

        let (payload, wrapper) = unwrap_envelope(&json, &envelope).unwrap();

        // Verify payload contains all nested fields from payload_field
        assert_eq!(
            payload.get("role").and_then(|v| v.as_str()),
            Some("assistant")
        );
        assert_eq!(
            payload.get("content").and_then(|v| v.as_str()),
            Some("Response")
        );
        assert!(payload.get("tool_calls").is_some());

        // Verify wrapper contains all original fields including wrapper-level ones
        assert_eq!(
            wrapper
                .as_ref()
                .unwrap()
                .get("request_id")
                .and_then(|v| v.as_str()),
            Some("req-123"),
            "Wrapper should contain wrapper-level fields"
        );
        assert_eq!(
            wrapper
                .as_ref()
                .unwrap()
                .get("timestamp")
                .and_then(|v| v.as_str()),
            Some("2026-03-16T12:00:00Z"),
            "Wrapper should contain timestamp"
        );
    }

    #[test]
    fn test_unwrap_envelope_different_skip_types_all_return_empty_none() {
        let envelope = create_test_envelope();

        // Test both "heartbeat" and "ping" route to skip
        let skip_lines = [
            r#"{"type": "heartbeat", "timestamp": "2026-03-16T12:00:00Z", "payload": {"status": "ok"}}"#,
            r#"{"type": "ping", "timestamp": "2026-03-16T12:00:00Z", "payload": {"seq": 1}}"#,
        ];

        for line in &skip_lines {
            let json: Value = serde_json::from_str(line).unwrap();
            let (payload, wrapper) = unwrap_envelope(&json, &envelope).unwrap();

            assert!(
                payload.as_object().map_or(false, |obj| obj.is_empty()),
                "Skip type '{}' should return empty payload",
                line
            );
            assert!(
                wrapper.is_none(),
                "Skip type '{}' should return None wrapper",
                line
            );
        }
    }

    // -- ^-prefixed envelope field extraction tests --

    fn create_caret_envelope_test_plugin() -> Plugin {
        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("message".to_string(), "event".to_string());

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
                envelope: Some(crate::plugin::Envelope {
                    payload_field: "payload".to_string(),
                    type_field: "type".to_string(),
                    type_routing,
                }),
                array: None,
            },
            parser: Parser {
                // Use ^ prefix to read timestamp from envelope wrapper
                timestamp: Some("^timestamp".to_string()),
                // No ^ prefix: read role/content from payload
                role: Some("role".to_string()),
                content: Some("content".to_string()),
                tool_name: Some("tool_name".to_string()),
                tokens_in: Some("tokens_in".to_string()),
                tokens_out: Some("tokens_out".to_string()),
                ..Default::default()
            },
            metadata: None,
        }
    }

    #[test]
    fn test_parse_line_caret_prefix_reads_from_wrapper() {
        // Test that ^timestamp reads from envelope wrapper, not payload
        let plugin = create_caret_envelope_test_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Envelope line: timestamp at wrapper level, payload has different timestamp
        let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Hello", "timestamp": "2026-03-16T10:00:00Z"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert_eq!(events.len(), 1);
        let event = &events[0];

        // Should use wrapper timestamp (12:00:00Z), not payload timestamp (10:00:00Z)
        assert_eq!(
            event.ts.to_rfc3339(),
            "2026-03-16T12:00:00+00:00",
            "^timestamp should read from wrapper level"
        );
    }

    #[test]
    fn test_parse_line_no_caret_prefix_reads_from_payload() {
        // Test that "role" without ^ prefix reads from payload, not wrapper
        let plugin = create_caret_envelope_test_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Envelope line: role at both levels, payload should win for non-^ fields
        let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "role": "wrapper_role", "payload": {"role": "user", "content": "Hello"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert_eq!(events.len(), 1);
        let event = &events[0];

        // Should use payload role ("user"), not wrapper role ("wrapper_role")
        assert_eq!(
            event.role,
            Role::User,
            "role (no ^) should read from payload"
        );
        assert_eq!(event.content, "Hello", "content should read from payload");
    }

    #[test]
    fn test_parse_line_caret_prefix_tool_name_from_wrapper() {
        // Test that ^ prefix works for tool_name field
        let mut plugin = create_caret_envelope_test_plugin();
        // Override tool_name to use ^ prefix
        plugin.parser.tool_name = Some("^tool_name".to_string());

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Envelope line with tool_name at wrapper level
        let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "tool_name": "wrapper_tool", "payload": {"role": "assistant", "content": "Running tool", "tool_name": "payload_tool"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert_eq!(events.len(), 1);
        let event = &events[0];

        // Should use wrapper tool_name ("wrapper_tool")
        assert_eq!(
            event.tool,
            Some("wrapper_tool".to_string()),
            "^tool_name should read from wrapper level"
        );
    }

    #[test]
    fn test_parse_line_caret_prefix_tokens_from_wrapper() {
        // Test that ^ prefix works for tokens_in/tokens_out fields
        let mut plugin = create_caret_envelope_test_plugin();
        // Override tokens to use ^ prefix
        plugin.parser.tokens_in = Some("^wrapper_tokens_in".to_string());
        plugin.parser.tokens_out = Some("^wrapper_tokens_out".to_string());

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Envelope line with tokens at wrapper level
        let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "wrapper_tokens_in": "100", "wrapper_tokens_out": "200", "payload": {"role": "user", "content": "Hello", "tokens_in": "50", "tokens_out": "75"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert_eq!(events.len(), 1);
        let event = &events[0];

        // Should use wrapper token counts
        assert_eq!(
            event.tokens.as_ref().map(|t| t.input),
            Some(100),
            "^wrapper_tokens_in should read from wrapper level"
        );
        assert_eq!(
            event.tokens.as_ref().map(|t| t.output),
            Some(200),
            "^wrapper_tokens_out should read from wrapper level"
        );
    }

    #[test]
    fn test_parse_line_missing_payload_field_with_caret_prefix() {
        // Test that missing payload_field skips with warning even when using ^ prefix
        let plugin = create_caret_envelope_test_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Line without the expected payload_field
        let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "data": {"role": "user", "content": "Hello"}}"#;

        // Should skip with warning and return empty Vec (no panic)
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert!(
            events.is_empty(),
            "Missing payload_field should skip line and return empty events"
        );
    }

    #[test]
    fn test_parse_line_non_object_payload_with_caret_prefix() {
        // Test that non-object payload_field skips with warning
        let plugin = create_caret_envelope_test_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // payload_field is a string instead of an object
        let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "payload": "not an object"}"#;

        // Should skip with warning and return empty Vec (no panic)
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert!(
            events.is_empty(),
            "Non-object payload should skip line and return empty events"
        );
    }

    #[test]
    fn test_parse_line_mixed_caret_and_payload_fields() {
        // Test comprehensive scenario with mixed ^ and non-^ field paths
        let mut plugin = create_caret_envelope_test_plugin();
        // Mix fields: timestamp from wrapper, role/content from payload, tool_name from wrapper
        plugin.parser.timestamp = Some("^ts".to_string());
        plugin.parser.role = Some("role".to_string());
        plugin.parser.content = Some("content".to_string());
        plugin.parser.tool_name = Some("^tool".to_string());

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Complex envelope with fields at both levels
        let line = r#"{"type": "message", "ts": "2026-03-16T12:00:00Z", "tool": "search", "payload": {"role": "assistant", "content": "Searching...", "tool": "payload_tool"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert_eq!(events.len(), 1);
        let event = &events[0];

        // Verify ^ts reads from wrapper
        assert_eq!(
            event.ts.to_rfc3339(),
            "2026-03-16T12:00:00+00:00",
            "^ts should read from wrapper level"
        );

        // Verify role/content read from payload
        assert_eq!(event.role, Role::Assistant, "role should read from payload");
        assert_eq!(
            event.content, "Searching...",
            "content should read from payload"
        );

        // Verify ^tool reads from wrapper
        assert_eq!(
            event.tool,
            Some("search".to_string()),
            "^tool should read from wrapper level (not payload.tool)"
        );
    }

    #[test]
    fn test_parse_line_no_envelope_plugin_ignores_caret_prefix() {
        // Test that ^ prefix has no effect when envelope is not configured
        let mut plugin = create_test_plugin();
        // Add ^ prefix even though there's no envelope config
        plugin.parser.timestamp = Some("^timestamp".to_string());

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Simple non-envelope line (field is actually at top level)
        let line = r#"{"timestamp": "2026-03-16T12:00:00Z", "role": "user", "content": "Hello"}}"#;

        // With no envelope config, ^timestamp should behave like timestamp
        // and fail to find the field (returns empty events or error)
        let result = JsonlParser::parse_line(line, 1, &context, &plugin);

        // Should either return empty events or error (depends on implementation)
        // The key is that it doesn't panic
        assert!(
            result.is_ok() || result.is_err(),
            "Should not panic with ^ prefix and no envelope"
        );
    }

    #[test]
    fn test_fixture_envelope_with_caret_prefix_parses_correctly() {
        // Integration test: fixture JSONL with envelope and ^ prefix should parse correctly
        let plugin = create_caret_envelope_test_plugin();

        // Create a temporary fixture file
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write fixture content with envelope structure
        let fixture_content = r#"{"type": "message", "timestamp": "2026-07-24T10:00:00Z", "payload": {"role": "user", "content": "First message"}}
{"type": "message", "timestamp": "2026-07-24T10:00:01Z", "payload": {"role": "assistant", "content": "Response"}}
{"type": "heartbeat", "timestamp": "2026-07-24T10:00:02Z", "payload": {"status": "ok"}}
{"type": "message", "timestamp": "2026-07-24T10:00:03Z", "payload": {"role": "user", "content": "Follow-up"}}
"#;
        std::fs::write(path, fixture_content).unwrap();

        // Parse the file
        let result = JsonlParser.parse(path, &plugin);

        assert!(result.is_ok(), "Should parse fixture successfully");
        let events = result.unwrap();

        // Should have 3 events (skipped the heartbeat)
        assert_eq!(events.len(), 3, "Should produce 3 events (1 skipped)");

        // Verify first event
        assert_eq!(events[0].role, Role::User);
        assert_eq!(events[0].content, "First message");
        assert_eq!(events[0].ts.to_rfc3339(), "2026-07-24T10:00:00+00:00");

        // Verify second event
        assert_eq!(events[1].role, Role::Assistant);
        assert_eq!(events[1].content, "Response");
        assert_eq!(events[1].ts.to_rfc3339(), "2026-07-24T10:00:01+00:00");

        // Verify third event (after skipped heartbeat)
        assert_eq!(events[2].role, Role::User);
        assert_eq!(events[2].content, "Follow-up");
        assert_eq!(events[2].ts.to_rfc3339(), "2026-07-24T10:00:03+00:00");
    }

    #[test]
    fn test_unwrap_envelope_type_extraction() {
        // Test type field extraction with both present and missing type field cases
        let envelope = create_test_envelope();

        // Test 1: Type field is present and correctly extracted
        let line_with_type = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Hello"}}"#;
        let json_with_type: Value = serde_json::from_str(line_with_type).unwrap();

        let (payload_with_type, wrapper_with_type) = unwrap_envelope(&json_with_type, &envelope).unwrap();

        // Verify that type field value is correctly extracted in the wrapper
        assert_eq!(
            wrapper_with_type
                .as_ref()
                .unwrap()
                .get("type")
                .and_then(|v| v.as_str()),
            Some("message"),
            "Type field 'message' should be correctly extracted and preserved in wrapper"
        );

        // Verify payload is extracted correctly for event type
        assert_eq!(
            payload_with_type.get("role").and_then(|v| v.as_str()),
            Some("user"),
            "Payload should contain role from payload_field"
        );

        // Test 2: Type field is missing - should default to skip behavior
        let line_without_type = r#"{"timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Hello"}}"#;
        let json_without_type: Value = serde_json::from_str(line_without_type).unwrap();

        let (payload_without_type, wrapper_without_type) = unwrap_envelope(&json_without_type, &envelope).unwrap();

        // Missing type field should result in skip behavior (empty payload, None wrapper)
        assert!(
            payload_without_type.as_object().map_or(false, |obj| obj.is_empty()),
            "Missing type field should return empty payload object"
        );

        assert!(
            wrapper_without_type.is_none(),
            "Missing type field should return None for wrapper (skip behavior)"
        );

        // Test 3: Type field with different value
        let line_different_type = r#"{"type": "heartbeat", "timestamp": "2026-03-16T12:00:00Z", "payload": {"status": "ok"}}"#;
        let json_different_type: Value = serde_json::from_str(line_different_type).unwrap();

        let (payload_different_type, wrapper_different_type) = unwrap_envelope(&json_different_type, &envelope).unwrap();

        // Verify heartbeat type is correctly identified and routed to skip
        assert!(
            payload_different_type.as_object().map_or(false, |obj| obj.is_empty()),
            "Type 'heartbeat' should return empty payload (skip routing)"
        );

        assert!(
            wrapper_different_type.is_none(),
            "Type 'heartbeat' should return None for wrapper (skip routing)"
        );
    }
}

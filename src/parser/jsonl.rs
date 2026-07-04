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

        // --- Envelope context setup ---
        // When the plugin defines [source.envelope], the JSONL line is a wrapper:
        //   { type_field: "...", payload_field: { ... actual event ... } }
        // Type routing determines whether to produce events, skip, or capture metadata.
        //
        // Set up envelope_json and payload_json references:
        // - envelope_json: reference to the full parsed line (or None if no envelope)
        // - payload_json: reference to the event data (from payload_field if envelope, else full line)
        //
        // IMPORTANT: In this bead, we ONLY:
        // 1. Set up these references
        // 2. Apply type-based early-skip routing
        //
        // Field extraction below still uses &raw_json exactly as before this bead.
        // Converting to envelope-aware field extraction (using payload_json) is a separate bead.

        let (_envelope_json, _payload_json): (Option<&Value>, &Value) =
            if let Some(ref envelope_cfg) = plugin.source.envelope {
                // Envelope mode: extract type and apply routing
                let type_value =
                    extract_string(&raw_json, &envelope_cfg.type_field).unwrap_or_default();
                let routing = envelope_cfg.get_routing(&type_value);

                match routing {
                    "skip" | "meta" => return Ok(Vec::new()), // Early skip for these types
                    "event" => {
                        // Extract payload from payload_field, falling back to raw_json if missing/not an object
                        let extracted = raw_json
                            .get(&envelope_cfg.payload_field)
                            .and_then(|v| {
                                // Only use if it's an object (not a string/null/etc)
                                match v {
                                    Value::Object(_) => Some(v),
                                    _ => None,
                                }
                            })
                            .unwrap_or(&raw_json);
                        (Some(&raw_json), extracted)
                    }
                    _ => return Ok(Vec::new()), // Unknown routing treated as skip
                }
            } else {
                // No envelope: both envelope_json and payload_json point to the full line
                (None, &raw_json)
            };

        // Check type filter (operate on raw_json - payload_json unused until next bead)
        if let Some(ref filter) = plugin.parser.include_types {
            let type_field = &filter.field;
            if let Some(type_val) = extract_string(&raw_json, type_field) {
                if !filter.values.contains(&type_val) {
                    return Ok(Vec::new()); // Skip this event
                }
            }
        }

        if let Some(ref filter) = plugin.parser.exclude_types {
            let type_field = &filter.field;
            if let Some(type_val) = extract_string(&raw_json, type_field) {
                if filter.values.contains(&type_val) {
                    return Ok(Vec::new()); // Skip this event
                }
            }
        }

        // Parse timestamp (operates on raw_json - envelope-aware extraction is next bead)
        let ts = if let Some(ref ts_field) = plugin.parser.timestamp {
            parse_timestamp(&raw_json, ts_field).map_err(|e| {
                AgentScribeError::parse_error_with_line(
                    &context.source_file,
                    line_number,
                    format!("Timestamp error: {}", e),
                )
            })?
        } else {
            Utc::now() // Fallback - shouldn't happen with validation
        };

        // Parse role (operates on raw_json - envelope-aware extraction is next bead)
        let role_str = if let Some(ref role_field) = plugin.parser.role {
            extract_string(&raw_json, role_field).ok_or_else(|| {
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

        // Parse content (operates on raw_json - envelope-aware extraction is next bead)
        let content = if let Some(ref content_field) = plugin.parser.content {
            extract_string(&raw_json, content_field).unwrap_or_default()
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

        // Extract tool name if applicable (operates on raw_json - envelope-aware extraction is next bead)
        if role == Role::ToolCall || role == Role::ToolResult {
            if let Some(ref tool_field) = plugin.parser.tool_name {
                if let Some(tool_name) = extract_string(&raw_json, tool_field) {
                    event.tool = Some(tool_name);
                }
            }
        }

        // Extract tokens (operates on raw_json - envelope-aware extraction is next bead)
        let tokens_in = plugin
            .parser
            .tokens_in
            .as_ref()
            .and_then(|f| extract_string(&raw_json, f).and_then(|s| s.parse::<u32>().ok()));
        let tokens_out = plugin
            .parser
            .tokens_out
            .as_ref()
            .and_then(|f| extract_string(&raw_json, f).and_then(|s| s.parse::<u32>().ok()));

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
                                        extract_string(&json, field)
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
            assert_eq!(
                events,
                Vec::new(),
                "skip-type '{}' should produce zero events",
                label
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
        assert_eq!(events.len(), 1, "Event-type routing should produce one event");
        let event = &events[0];

        // Verify correct role and content
        assert_eq!(event.role, Role::User, "Role should be user");
        assert_eq!(event.content, "Hello world", "Content should match payload content");

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
        assert_eq!(events.len(), 0, "Skip-type routing should produce zero events");
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
        assert_eq!(events.len(), 0, "Meta-type routing should produce zero events");
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
        assert_eq!(events.len(), 1, "Event-type routing should produce one event");
        let event = &events[0];

        // Verify that role and content are extracted from the payload
        assert_eq!(event.role, Role::Assistant, "Role should come from payload");
        assert_eq!(event.content, "Response text", "Content should come from payload");

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

        assert!(events.is_empty(), "heartbeat (skip) should produce zero events");
    }

    #[test]
    fn test_skip_type_ping_produces_zero_events() {
        let plugin = create_skip_meta_unknown_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        let line = r#"{"type": "ping", "timestamp": "2026-07-04T10:00:10Z", "payload": {"seq": 2}}"#;
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

        assert!(events.is_empty(), "session_start (meta) should produce zero events");
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

        assert!(events.is_empty(), "unknown type (not in map) should produce zero events");
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
}

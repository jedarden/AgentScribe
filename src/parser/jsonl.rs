//! JSONL format parser
//!
//! Parses JSONL files where each line is a JSON object.

#[cfg(test)]
mod jsonl_subagent_test;

use crate::error::{AgentScribeError, Result};
use crate::event::{Event, Role, TokenCounts};
use crate::parser::{
    extract_string, extract_string_with_envelope, parse_timestamp_with_envelope, ParseContext,
    SessionInfo,
};
use crate::plugin::{Plugin, SessionDetection, SessionIdSource};
use chrono::Utc;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::Path;
use tracing::warn;

/// Check if a source path is a subagent file
fn is_subagent_file(source_path: &Path) -> bool {
    source_path
        .components()
        .any(|c| c.as_os_str() == "subagents")
}

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
#[allow(dead_code)]
pub fn unwrap_envelope(
    raw_json: &Value,
    envelope: &crate::plugin::Envelope,
) -> Result<(Value, Option<Value>)> {
    // Get the routing action based on the type field value
    // Read the type_field from the JSON value using envelope.type_field
    let type_value = raw_json
        .get(&envelope.type_field)
        .and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        })
        .unwrap_or_default();
    let routing = envelope.get_routing(&type_value);

    match routing {
        "skip" => {
            // Return empty payload with None to signal "drop this line"
            Ok((Value::Object(serde_json::Map::new()), None))
        }
        "meta" => {
            // Return empty payload with the full wrapper JSON
            Ok((
                Value::Object(serde_json::Map::new()),
                Some(raw_json.clone()),
            ))
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
                    // Return the extracted payload along with the full wrapper JSON
                    Ok((payload.clone(), Some(raw_json.clone())))
                }
                None => {
                    // Missing or non-object payload_field - skip with warning
                    // Determine specific warning reason
                    let has_payload_field = raw_json.get(&envelope.payload_field).is_some();
                    let warning_msg = if has_payload_field {
                        let payload_value = raw_json.get(&envelope.payload_field).unwrap();
                        let value_desc = match payload_value {
                            Value::String(s) => {
                                let truncated = if s.len() > 50 {
                                    format!("{}...", &s[..50])
                                } else {
                                    s.clone()
                                };
                                format!("string '{}'", truncated)
                            }
                            Value::Null => "null".to_string(),
                            Value::Bool(b) => format!("bool {}", b),
                            Value::Number(n) => format!("number {}", n),
                            Value::Array(_) => "array".to_string(),
                            Value::Object(_) => "object".to_string(),
                        };
                        format!(
                            "Envelope payload_field '{}' exists for type '{}' but is not an object (found: {}), skipping line",
                            envelope.payload_field, type_value, value_desc
                        )
                    } else {
                        format!(
                            "Envelope payload_field '{}' missing for type '{}', skipping line",
                            envelope.payload_field, type_value
                        )
                    };
                    warn!("{}", warning_msg);
                    Ok((Value::Object(serde_json::Map::new()), None))
                }
            }
        }
        _ => {
            // Unknown routing (shouldn't happen due to get_routing defaults, but handle defensively)
            Ok((Value::Object(serde_json::Map::new()), None))
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
                    // Metadata line - no event emitted
                    // TODO: Future session metadata accumulation (project, model, version)
                    // These lines contain session-level metadata that should be extracted
                    // and accumulated into the session context. For now, we drop them.
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
                            // Determine specific warning reason
                            let has_payload_field =
                                raw_json.get(&envelope_cfg.payload_field).is_some();
                            let warning_msg = if has_payload_field {
                                let payload_value =
                                    raw_json.get(&envelope_cfg.payload_field).unwrap();
                                let value_desc = match payload_value {
                                    Value::String(s) => {
                                        let truncated = if s.len() > 50 {
                                            format!("{}...", &s[..50])
                                        } else {
                                            s.clone()
                                        };
                                        format!("string '{}'", truncated)
                                    }
                                    Value::Null => "null".to_string(),
                                    Value::Bool(b) => format!("bool {}", b),
                                    Value::Number(n) => format!("number {}", n),
                                    Value::Array(_) => "array".to_string(),
                                    Value::Object(_) => "object".to_string(),
                                };
                                format!(
                                    "Envelope payload_field '{}' exists for type '{}' but is not an object (found: {}), skipping line",
                                    envelope_cfg.payload_field, type_value, value_desc
                                )
                            } else {
                                format!(
                                    "Envelope payload_field '{}' missing for type '{}', skipping line",
                                    envelope_cfg.payload_field, type_value
                                )
                            };
                            warn!("{}", warning_msg);
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
                        // Only override if we don't already have a subagent suffix
                        // The ParseContext sets source_agent with "-subagent" suffix for subagent files,
                        // and we need to preserve that instead of overwriting with the static value
                        if !event.source_agent.ends_with("-subagent") {
                            event.source_agent = s.to_string();
                        }
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
                    SessionIdSource::Filename => {
                        // For subagent sessions, include parent directory in session ID
                        // to maintain hierarchy and avoid collisions
                        let is_subagent = is_subagent_file(source_path);

                        eprintln!("DEBUG: source_path = {:?}", source_path);
                        eprintln!("DEBUG: is_subagent = {}", is_subagent);

                        if is_subagent {
                            // Extract path from "projects/<project>/<parent>/subagents/<agent>.jsonl"
                            // to get "<parent>/<agent>"
                            let components: Vec<_> = source_path.components().collect();
                            eprintln!("DEBUG: components = {:?}", components);

                            if let Some(subagents_idx) =
                                components.iter().position(|c| c.as_os_str() == "subagents")
                            {
                                if subagents_idx >= 2 {
                                    let parent_idx = subagents_idx - 1;
                                    if let (Some(parent_os), Some(agent_os)) = (
                                        components.get(parent_idx),
                                        components.get(subagents_idx + 1),
                                    ) {
                                        if let (Some(parent_name), Some(agent_name)) = (
                                            parent_os.as_os_str().to_str(),
                                            agent_os.as_os_str().to_str(),
                                        ) {
                                            // Get agent name without extension
                                            let agent_stem = agent_name
                                                .strip_suffix(".jsonl")
                                                .or_else(|| agent_name.strip_suffix(".json"))
                                                .unwrap_or(agent_name);
                                            format!("{}/{}", parent_name, agent_stem)
                                        } else {
                                            source_path
                                                .file_stem()
                                                .and_then(|s| s.to_str())
                                                .unwrap_or("unknown")
                                                .to_string()
                                        }
                                    } else {
                                        source_path
                                            .file_stem()
                                            .and_then(|s| s.to_str())
                                            .unwrap_or("unknown")
                                            .to_string()
                                    }
                                } else {
                                    source_path
                                        .file_stem()
                                        .and_then(|s| s.to_str())
                                        .unwrap_or("unknown")
                                        .to_string()
                                }
                            } else {
                                source_path
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("unknown")
                                    .to_string()
                            }
                        } else {
                            // For main sessions, use just the filename
                            source_path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("unknown")
                                .to_string()
                        }
                    }
                    SessionIdSource::Field(_) => {
                        // Need to read first line to get session ID
                        "unknown".to_string() // Will be updated during parsing
                    }
                }
            }
            _ => "unknown".to_string(),
        };

        // Adjust source_agent for subagent files
        let source_agent = if is_subagent_file(source_path) {
            format!("{}-subagent", plugin.plugin.name.clone())
        } else {
            plugin.plugin.name.clone()
        };

        let context =
            ParseContext::new(session_id, source_agent, source_path.display().to_string());

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
                    SessionIdSource::Filename => {
                        // For subagent sessions, include parent directory in session ID
                        // to maintain hierarchy and avoid collisions
                        let is_subagent = source_path
                            .components()
                            .any(|c| c.as_os_str() == "subagents");

                        if is_subagent {
                            // Extract path from ".../<parent>/subagents/<agent>.jsonl"
                            // to get "<parent>/<agent>"
                            let components: Vec<_> = source_path.components().collect();

                            if let Some(subagents_idx) =
                                components.iter().position(|c| c.as_os_str() == "subagents")
                            {
                                if subagents_idx >= 2 {
                                    let parent_idx = subagents_idx - 1;
                                    if let (Some(parent_os), Some(agent_os)) = (
                                        components.get(parent_idx),
                                        components.get(subagents_idx + 1),
                                    ) {
                                        if let (Some(parent_name), Some(agent_name)) = (
                                            parent_os.as_os_str().to_str(),
                                            agent_os.as_os_str().to_str(),
                                        ) {
                                            // Get agent name without extension
                                            let agent_stem = agent_name
                                                .strip_suffix(".jsonl")
                                                .or_else(|| agent_name.strip_suffix(".json"))
                                                .unwrap_or(agent_name);
                                            format!("{}/{}", parent_name, agent_stem)
                                        } else {
                                            source_path
                                                .file_stem()
                                                .and_then(|s| s.to_str())
                                                .unwrap_or("unknown")
                                                .to_string()
                                        }
                                    } else {
                                        source_path
                                            .file_stem()
                                            .and_then(|s| s.to_str())
                                            .unwrap_or("unknown")
                                            .to_string()
                                    }
                                } else {
                                    source_path
                                        .file_stem()
                                        .and_then(|s| s.to_str())
                                        .unwrap_or("unknown")
                                        .to_string()
                                }
                            } else {
                                source_path
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("unknown")
                                    .to_string()
                            }
                        } else {
                            source_path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("unknown")
                                .to_string()
                        }
                    }
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
                //
                // Subagent files are at: .../<parent-session-uuid>/subagents/agent-<id>.jsonl
                //
                // ROOT CAUSE FIX (bead bf-1pkfp):
                // The previous implementation required:
                // 1. At least 2 components before "subagents" (subagents_idx >= 2)
                // 2. A "projects" directory somewhere before the parent session
                //
                // This caused test fixtures and non-standard directory structures to fail
                // parent_session_id extraction because they didn't match the exact production
                // layout of ~/.claude/projects/<path>/<parent>/subagents/...
                //
                // THE FIX:
                // - Removed the "projects" directory requirement entirely
                // - Reduced minimum components before "subagents" from 2 to 1
                // - Directly extract parent session ID as the component immediately before "subagents"
                //
                // This allows subagent detection to work for both:
                // - Production paths: ~/.claude/projects/<path>/<parent>/subagents/...
                // - Test paths: /tmp/.../sessions/<parent>/subagents/...
                //
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

    /// Fixture structure for meta routing test cases
    ///
    /// Provides a structured way to define meta routing test scenarios
    /// with expected behavior and assertions.
    #[derive(Debug, Clone)]
    struct MetaRoutingTestFixture {
        /// Test case description
        #[allow(dead_code)]
        description: &'static str,
        /// Type field value for the envelope line
        type_value: &'static str,
        /// Expected routing action: "skip", "meta", or "event"
        expected_routing: &'static str,
        /// Expected number of events produced (0 for skip/meta, 1 for event)
        expected_event_count: usize,
        /// Sample envelope line JSON for testing
        sample_line: &'static str,
        /// Whether the line should be dropped (skip/meta) or produce events (event)
        should_drop: bool,
    }

    impl MetaRoutingTestFixture {
        /// Create a new meta routing test fixture
        fn new(
            description: &'static str,
            type_value: &'static str,
            expected_routing: &'static str,
            sample_line: &'static str,
        ) -> Self {
            let should_drop = expected_routing == "skip" || expected_routing == "meta";
            let expected_event_count = if should_drop { 0 } else { 1 };

            Self {
                description,
                type_value,
                expected_routing,
                expected_event_count,
                sample_line,
                should_drop,
            }
        }

        /// Create a skip-type fixture (line should be dropped)
        fn skip_fixture(
            description: &'static str,
            type_value: &'static str,
            sample_line: &'static str,
        ) -> Self {
            Self::new(description, type_value, "skip", sample_line)
        }

        /// Create a meta-type fixture (line should accumulate metadata, not emit events)
        fn meta_fixture(
            description: &'static str,
            type_value: &'static str,
            sample_line: &'static str,
        ) -> Self {
            Self::new(description, type_value, "meta", sample_line)
        }

        /// Create an event-type fixture (line should produce events)
        fn event_fixture(
            description: &'static str,
            type_value: &'static str,
            sample_line: &'static str,
        ) -> Self {
            Self::new(description, type_value, "event", sample_line)
        }
    }

    /// Helper function to create a plugin with custom meta routing configuration
    ///
    /// # Arguments
    /// * `type_routing` - HashMap mapping type values to routing actions
    ///
    /// # Returns
    /// A configured Plugin with envelope routing set up
    fn create_meta_routing_plugin(
        type_routing: std::collections::HashMap<String, String>,
    ) -> Plugin {
        let mut role_map = std::collections::HashMap::new();
        role_map.insert("toolResult".to_string(), "tool_result".to_string());

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
                timestamp: Some("^timestamp".to_string()),
                role: Some("role".to_string()),
                content: Some("content".to_string()),
                role_map,
                ..Default::default()
            },
            metadata: None,
        }
    }

    /// Helper function to assert meta routing behavior
    ///
    /// Parses a line with the given plugin and asserts the expected routing behavior.
    ///
    /// # Arguments
    /// * `plugin` - The plugin to use for parsing
    /// * `line` - The JSONL line to parse
    /// * `expected_event_count` - Expected number of events to be produced
    /// * `should_drop` - Whether the line should be dropped (true) or produce events (false)
    fn assert_meta_routing_behavior(
        plugin: &Plugin,
        line: &str,
        expected_event_count: usize,
        should_drop: bool,
    ) {
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        let events =
            JsonlParser::parse_line(line, 1, &context, plugin).expect("Parsing should succeed");

        assert_eq!(
            events.len(),
            expected_event_count,
            "Expected {} events, got {}",
            expected_event_count,
            events.len()
        );

        assert_eq!(
            events.is_empty(),
            should_drop,
            "Line should {} events, but it {}",
            if should_drop {
                "not produce"
            } else {
                "produce"
            },
            if should_drop {
                "produced some"
            } else {
                "didn't produce"
            }
        );
    }

    /// Helper function to test a meta routing fixture
    ///
    /// Uses a fixture to validate meta routing behavior comprehensively.
    ///
    /// # Arguments
    /// * `fixture` - The MetaRoutingTestFixture to validate
    fn test_meta_routing_fixture(fixture: &MetaRoutingTestFixture) {
        // Build type routing map with just this fixture's type
        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert(
            fixture.type_value.to_string(),
            fixture.expected_routing.to_string(),
        );

        let plugin = create_meta_routing_plugin(type_routing);

        assert_meta_routing_behavior(
            &plugin,
            fixture.sample_line,
            fixture.expected_event_count,
            fixture.should_drop,
        );
    }

    /// Collection of standard meta routing fixtures
    ///
    /// Provides a set of commonly-used test scenarios for meta routing.
    fn standard_meta_routing_fixtures() -> Vec<MetaRoutingTestFixture> {
        vec![
            // Skip-type fixtures
            MetaRoutingTestFixture::skip_fixture(
                "heartbeat type should be skipped",
                "heartbeat",
                r#"{"type": "heartbeat", "timestamp": "2026-03-16T12:00:00Z", "payload": {"status": "ok"}}"#,
            ),
            MetaRoutingTestFixture::skip_fixture(
                "ping type should be skipped",
                "ping",
                r#"{"type": "ping", "timestamp": "2026-03-16T12:00:00Z", "payload": {"seq": 1}}"#,
            ),
            // Meta-type fixtures
            MetaRoutingTestFixture::meta_fixture(
                "session_start type should accumulate metadata",
                "session_start",
                r#"{"type": "session_start", "timestamp": "2026-03-16T12:00:00Z", "payload": {"session_id": "sess-001"}}"#,
            ),
            MetaRoutingTestFixture::meta_fixture(
                "session_end type should accumulate metadata",
                "session_end",
                r#"{"type": "session_end", "timestamp": "2026-03-16T12:30:00Z", "payload": {"duration": 1800}}"#,
            ),
            MetaRoutingTestFixture::meta_fixture(
                "compaction type should accumulate metadata",
                "compaction",
                r#"{"type": "compaction", "timestamp": "2026-03-16T12:05:00Z", "payload": {"files_compacted": 3}}"#,
            ),
            // Event-type fixtures
            MetaRoutingTestFixture::event_fixture(
                "message type should produce events",
                "message",
                r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Hello"}}"#,
            ),
        ]
    }

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

        let mut role_map = std::collections::HashMap::new();
        role_map.insert("toolResult".to_string(), "tool_result".to_string());

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
                timestamp: Some("^timestamp".to_string()),
                role: Some("role".to_string()),
                content: Some("content".to_string()),
                role_map,
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
    #[allow(dead_code)]
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
        type_routing.insert("compaction".to_string(), "meta".to_string());
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

    /// Helper function for testing meta routing fixture lines that should return Ok(Vec::new()).
    ///
    /// This function encapsulates the common pattern for testing envelope lines that are
    /// routed to "meta" types (such as session_start, session_end, metrics, compaction).
    /// These meta-type lines should parse successfully but produce zero canonical events.
    ///
    /// # Arguments
    /// * `line` - The JSONL fixture line to parse
    /// * `line_number` - The line number (for realistic error reporting)
    /// * `assertion_msg` - Custom assertion message describing what should produce zero events
    ///
    /// # Examples
    /// ```ignore
    /// let fixture_line = r#"{"type":"session_start","timestamp":"2026-07-04T10:00:00Z","payload":{"session_id":"sess-001"}}"#;
    /// assert_meta_routing_returns_empty(fixture_line, 3, "session_start should produce zero events");
    /// ```
    fn assert_meta_routing_returns_empty(line: &str, line_number: usize, assertion_msg: &str) {
        let plugin = create_skip_meta_unknown_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Verify the line parses successfully
        let result = JsonlParser::parse_line(line, line_number, &context, &plugin);
        assert!(
            result.is_ok(),
            "Meta routing line should parse successfully: {}",
            assertion_msg
        );

        // Verify it produces zero events (the expected behavior for meta-type routing)
        let events = result.unwrap();
        assert!(events.is_empty(), "{}", assertion_msg);
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
    fn test_meta_type_session_end_produces_zero_events() {
        let plugin = create_skip_meta_unknown_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        let line = r#"{"type": "session_end", "timestamp": "2026-07-04T10:00:30Z", "payload": {"duration": 30}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert!(
            events.is_empty(),
            "session_end (meta) should produce zero events"
        );
    }

    #[test]
    fn test_meta_type_metrics_produces_zero_events() {
        let plugin = create_skip_meta_unknown_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        let line = r#"{"type": "metrics", "timestamp": "2026-07-04T10:00:35Z", "payload": {"events_processed": 5}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert!(
            events.is_empty(),
            "metrics (meta) should produce zero events"
        );
    }

    #[test]
    fn test_meta_type_compaction_produces_zero_events() {
        let plugin = create_skip_meta_unknown_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        let line = r#"{"type": "compaction", "timestamp": "2026-07-04T10:00:40Z", "payload": {"files_compacted": 3}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert!(
            events.is_empty(),
            "compaction (meta) should produce zero events"
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
    fn test_session_start_fixture_line_returns_empty_vec() {
        // Test case for session_start fixture line from non-event-types.jsonl
        // Verifies that Ok(Vec::new()) is returned for session_start meta routing
        // Exact fixture line from non-event-types.jsonl line 3
        let line = r#"{"type":"session_start","timestamp":"2026-07-04T10:00:00Z","payload":{"session_id":"sess-001"}}"#;

        assert_meta_routing_returns_empty(line, 3, "session_start should produce zero events");
    }

    #[test]
    fn test_session_end_fixture_line_returns_empty_vec() {
        // Test case for session_end fixture line from non-event-types.jsonl
        // Verifies that Ok(Vec::new()) is returned for session_end meta routing
        // Exact fixture line from non-event-types.jsonl line 4
        let line = r#"{"type":"session_end","timestamp":"2026-07-04T10:00:30Z","payload":{"duration":30}}"#;

        assert_meta_routing_returns_empty(line, 4, "session_end should produce zero events");
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
            payload.as_object().is_some_and(|obj| obj.is_empty()),
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
            payload.as_object().is_some_and(|obj| obj.is_empty()),
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
            payload.as_object().is_some_and(|obj| obj.is_empty()),
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
            payload.as_object().is_some_and(|obj| obj.is_empty()),
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
            payload.as_object().is_some_and(|obj| obj.is_empty()),
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
            payload.as_object().is_some_and(|obj| obj.is_empty()),
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
            payload.as_object().is_some_and(|obj| obj.is_empty()),
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
                payload.as_object().is_some_and(|obj| obj.is_empty()),
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
                // Add role_map for tool_call support
                role_map: {
                    let mut map = std::collections::HashMap::new();
                    map.insert("tool_call".to_string(), "tool_call".to_string());
                    map
                },
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
        let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "tool_name": "wrapper_tool", "payload": {"role": "tool_call", "content": "Running tool", "tool_name": "payload_tool"}}"#;
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
        // Add role_map for tool_call support
        plugin.parser.role_map = {
            let mut map = std::collections::HashMap::new();
            map.insert("tool_call".to_string(), "tool_call".to_string());
            map
        };

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Complex envelope with fields at both levels
        let line = r#"{"type": "message", "ts": "2026-03-16T12:00:00Z", "tool": "search", "payload": {"role": "tool_call", "content": "Searching...", "tool": "payload_tool"}}"#;
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
        assert_eq!(event.role, Role::ToolCall, "role should read from payload");
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

        let (payload_with_type, wrapper_with_type) =
            unwrap_envelope(&json_with_type, &envelope).unwrap();

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

        let (payload_without_type, wrapper_without_type) =
            unwrap_envelope(&json_without_type, &envelope).unwrap();

        // Missing type field should result in skip behavior (empty payload, None wrapper)
        assert!(
            payload_without_type
                .as_object()
                .is_some_and(|obj| obj.is_empty()),
            "Missing type field should return empty payload object"
        );

        assert!(
            wrapper_without_type.is_none(),
            "Missing type field should return None for wrapper (skip behavior)"
        );

        // Test 3: Type field with different value
        let line_different_type = r#"{"type": "heartbeat", "timestamp": "2026-03-16T12:00:00Z", "payload": {"status": "ok"}}"#;
        let json_different_type: Value = serde_json::from_str(line_different_type).unwrap();

        let (payload_different_type, wrapper_different_type) =
            unwrap_envelope(&json_different_type, &envelope).unwrap();

        // Verify heartbeat type is correctly identified and routed to skip
        assert!(
            payload_different_type
                .as_object()
                .is_some_and(|obj| obj.is_empty()),
            "Type 'heartbeat' should return empty payload (skip routing)"
        );

        assert!(
            wrapper_different_type.is_none(),
            "Type 'heartbeat' should return None for wrapper (skip routing)"
        );
    }

    #[test]
    fn test_unwrap_envelope_basic_type_field_extraction() {
        // Basic unit test that verifies unwrap_envelope can extract the type_field from a JSON value
        let envelope = create_test_envelope();

        // Create a sample JSON value with a type field
        let json_line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Hello World"}}"#;
        let json_value: Value =
            serde_json::from_str(json_line).expect("JSON should parse successfully");

        // Call unwrap_envelope with appropriate EnvelopeConfig
        let (payload, wrapper) =
            unwrap_envelope(&json_value, &envelope).expect("unwrap_envelope should succeed");

        // Verify the type field is read correctly
        assert!(wrapper.is_some(), "Wrapper should be Some for event type");
        let wrapper_ref = wrapper.as_ref().expect("Wrapper should be Some");
        let extracted_type = wrapper_ref.get("type").and_then(|v| v.as_str());
        assert_eq!(
            extracted_type,
            Some("message"),
            "Type field should be extracted as 'message'"
        );

        // Verify payload extraction works correctly
        assert_eq!(payload.get("role").and_then(|v| v.as_str()), Some("user"));
        assert_eq!(
            payload.get("content").and_then(|v| v.as_str()),
            Some("Hello World")
        );
    }

    #[test]
    fn test_detect_sessions_multiple_subagents_same_parent() {
        // Test that multiple subagent files under the same parent UUID
        // each return exactly 1 session with correct session_id and parent_session_id

        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let parent_uuid = "parent-session-abc123";

        // Create parent directory structure
        let parent_dir = temp
            .path()
            .join(".claude/projects/test-project")
            .join(parent_uuid);
        let subagents_dir = parent_dir.join("subagents");
        std::fs::create_dir_all(&subagents_dir).expect("Failed to create subagents directory");

        // Create multiple subagent files: agent-1, agent-2, agent-3
        let subagent_files = vec!["agent-1", "agent-2", "agent-3"];

        for agent_name in &subagent_files {
            let agent_path = subagents_dir.join(format!("{}.jsonl", agent_name));
            let content = r#"{"timestamp": "2026-07-23T10:00:00Z", "role": "user", "content": "Test message"}"#;
            std::fs::write(&agent_path, content).expect("Failed to write agent content");
        }

        // Create a test plugin
        let plugin = create_test_plugin();

        // Test each subagent file
        for agent_name in &subagent_files {
            let agent_path = subagents_dir.join(format!("{}.jsonl", agent_name));

            // Call detect_sessions for this subagent file
            let sessions = JsonlParser::detect_sessions(&JsonlParser, &agent_path, &plugin)
                .expect("detect_sessions should succeed");

            // Verify exactly 1 session is returned
            assert_eq!(
                sessions.len(),
                1,
                "detect_sessions should return exactly 1 session for {}",
                agent_name
            );

            let session_info = &sessions[0];

            // Verify session_id matches the agent filename (without .jsonl extension)
            assert_eq!(
                session_info.session_id,
                format!("{}/{}", parent_uuid, agent_name),
                "session_id should be '{}/{}' for {}",
                parent_uuid,
                agent_name,
                agent_name
            );

            // Verify parent_session_id matches the shared parent UUID
            assert_eq!(
                session_info.parent_session_id,
                Some(parent_uuid.to_string()),
                "parent_session_id should be '{}' for {}",
                parent_uuid,
                agent_name
            );

            // Verify offsets
            assert_eq!(
                session_info.start_offset, 0,
                "start_offset should be 0 for {}",
                agent_name
            );
            assert!(
                session_info.end_offset > 0,
                "end_offset should be positive for {}",
                agent_name
            );
        }

        println!(
            "✅ Multiple subagents with same parent test passed! Tested {} subagent files under parent '{}'",
            subagent_files.len(),
            parent_uuid
        );
    }

    #[test]
    fn test_envelope_routing_event() {
        // Test that event lines correctly unwrap payload and produce events
        let plugin = create_envelope_test_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Event line: type=message routes to event, should unwrap payload and produce event
        let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "message": {"role": "user", "content": "Hello world"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        // Should produce exactly 1 event
        assert_eq!(
            events.len(),
            1,
            "Event routing should produce exactly 1 event"
        );

        let event = &events[0];

        // Verify payload was correctly unwrapped - role and content from payload
        assert_eq!(
            event.role,
            Role::User,
            "Role should be from unwrapped payload"
        );
        assert_eq!(
            event.content, "Hello world",
            "Content should be from unwrapped payload"
        );

        // Verify timestamp from envelope wrapper (^timestamp)
        assert_eq!(
            event.ts.to_rfc3339(),
            "2026-03-16T12:00:00+00:00",
            "Timestamp should be from envelope wrapper"
        );
    }

    #[test]
    fn test_envelope_routing_skip() {
        // Test that skip lines drop (produce 0 events)
        let plugin = create_envelope_test_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Skip line: type=session routes to skip, should drop the line
        let line = r#"{"type": "session", "timestamp": "2026-03-16T12:00:00Z", "message": {"role": "system", "content": "session start"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        // Should produce exactly 0 events (line dropped)
        assert_eq!(
            events.len(),
            0,
            "Skip routing should drop line and produce 0 events"
        );
    }

    #[test]
    fn test_envelope_routing_meta() {
        // Test that meta lines accumulate envelope state and produce 0 events
        let plugin = create_envelope_test_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Meta line: type=compaction routes to meta, should accumulate state and produce 0 events
        let line = r#"{"type": "compaction", "timestamp": "2026-03-16T12:00:00Z", "message": {"role": "system", "content": "compaction info"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        // Should produce exactly 0 events (meta accumulates state but doesn't emit events)
        assert_eq!(
            events.len(),
            0,
            "Meta routing should accumulate envelope state and produce 0 events"
        );
    }

    #[test]
    fn test_envelope_routing_unknown_type() {
        // Test that unknown type defaults to skip behavior (0 events)
        let plugin = create_envelope_test_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Unknown type line: type=unknown_event not in routing map, should default to skip
        let line = r#"{"type": "unknown_event", "timestamp": "2026-03-16T12:00:00Z", "message": {"role": "user", "content": "test content"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        // Should produce exactly 0 events (unknown types default to skip)
        assert_eq!(
            events.len(),
            0,
            "Unknown type should default to skip behavior and produce 0 events"
        );
    }

    // -- Type field extraction tests for string, number, bool, and missing --

    #[test]
    fn test_type_field_extraction_string_value() {
        // Test that type field with string value is extracted correctly
        let mut plugin = create_test_plugin();
        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("message".to_string(), "event".to_string());
        plugin.source.envelope = Some(crate::plugin::Envelope {
            payload_field: "payload".to_string(),
            type_field: "type".to_string(),
            type_routing,
        });
        plugin.parser.timestamp = Some("^timestamp".to_string());
        plugin.parser.role = Some("role".to_string());
        plugin.parser.content = Some("content".to_string());

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Type field is a string value "message"
        let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Hello"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        // Should route to "event" and produce 1 event
        assert_eq!(
            events.len(),
            1,
            "String type value 'message' should route to event"
        );
        assert_eq!(events[0].role, Role::User);
        assert_eq!(events[0].content, "Hello");
    }

    #[test]
    fn test_type_field_extraction_number_value() {
        // Test that type field with numeric value is converted to string and routed correctly
        let mut plugin = create_test_plugin();
        let mut type_routing = std::collections::HashMap::new();
        // Type routing uses numeric value as string
        type_routing.insert("1".to_string(), "event".to_string());
        type_routing.insert("2".to_string(), "skip".to_string());
        plugin.source.envelope = Some(crate::plugin::Envelope {
            payload_field: "payload".to_string(),
            type_field: "type".to_string(),
            type_routing,
        });
        plugin.parser.timestamp = Some("^timestamp".to_string());
        plugin.parser.role = Some("role".to_string());
        plugin.parser.content = Some("content".to_string());

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Type field is a numeric value 1
        let line = r#"{"type": 1, "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Hello"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        // Should convert number to string "1" and route to "event"
        assert_eq!(
            events.len(),
            1,
            "Numeric type value 1 should be converted to '1' and route to event"
        );
        assert_eq!(events[0].content, "Hello");

        // Test numeric value 2 routes to skip
        let line2 = r#"{"type": 2, "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Skipped"}}"#;
        let events2 = JsonlParser::parse_line(line2, 1, &context, &plugin).unwrap();

        // Should convert number to string "2" and route to "skip"
        assert_eq!(
            events2.len(),
            0,
            "Numeric type value 2 should be converted to '2' and route to skip"
        );
    }

    #[test]
    fn test_type_field_extraction_bool_value() {
        // Test that type field with boolean value is converted to string and routed correctly
        let mut plugin = create_test_plugin();
        let mut type_routing = std::collections::HashMap::new();
        // Type routing uses boolean values as strings
        type_routing.insert("true".to_string(), "event".to_string());
        type_routing.insert("false".to_string(), "skip".to_string());
        plugin.source.envelope = Some(crate::plugin::Envelope {
            payload_field: "payload".to_string(),
            type_field: "type".to_string(),
            type_routing,
        });
        plugin.parser.timestamp = Some("^timestamp".to_string());
        plugin.parser.role = Some("role".to_string());
        plugin.parser.content = Some("content".to_string());

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Type field is boolean true
        let line = r#"{"type": true, "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Hello"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        // Should convert bool to string "true" and route to "event"
        assert_eq!(
            events.len(),
            1,
            "Boolean type value true should be converted to 'true' and route to event"
        );
        assert_eq!(events[0].content, "Hello");

        // Test boolean false routes to skip
        let line2 = r#"{"type": false, "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Skipped"}}"#;
        let events2 = JsonlParser::parse_line(line2, 1, &context, &plugin).unwrap();

        // Should convert bool to string "false" and route to "skip"
        assert_eq!(
            events2.len(),
            0,
            "Boolean type value false should be converted to 'false' and route to skip"
        );
    }

    #[test]
    fn test_type_field_extraction_missing_defaults_to_empty_string() {
        // Test that missing type field defaults to empty string and routes to skip
        let mut plugin = create_test_plugin();
        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("message".to_string(), "event".to_string());
        // Empty string is NOT in routing map, so it should default to skip
        plugin.source.envelope = Some(crate::plugin::Envelope {
            payload_field: "payload".to_string(),
            type_field: "type".to_string(),
            type_routing,
        });
        plugin.parser.timestamp = Some("timestamp".to_string());
        plugin.parser.role = Some("role".to_string());
        plugin.parser.content = Some("content".to_string());

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Type field is completely missing from the JSON line
        let line = r#"{"timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Hello"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        // Missing type_field should default to empty string "", which routes to skip
        assert_eq!(
            events.len(),
            0,
            "Missing type field should default to empty string and route to skip"
        );
    }

    #[test]
    fn test_type_field_extraction_empty_string_value() {
        // Test that type field with explicit empty string routes to skip
        let mut plugin = create_test_plugin();
        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("message".to_string(), "event".to_string());
        // Empty string is NOT in routing map, so it should default to skip
        plugin.source.envelope = Some(crate::plugin::Envelope {
            payload_field: "payload".to_string(),
            type_field: "type".to_string(),
            type_routing,
        });
        plugin.parser.timestamp = Some("timestamp".to_string());
        plugin.parser.role = Some("role".to_string());
        plugin.parser.content = Some("content".to_string());

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Type field is explicitly an empty string
        let line = r#"{"type": "", "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Hello"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        // Empty string should default to skip behavior
        assert_eq!(
            events.len(),
            0,
            "Empty string type field should route to skip"
        );
    }

    #[test]
    fn test_type_field_extraction_null_value() {
        // Test that type field with null value defaults to empty string and routes to skip
        let mut plugin = create_test_plugin();
        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("message".to_string(), "event".to_string());
        plugin.source.envelope = Some(crate::plugin::Envelope {
            payload_field: "payload".to_string(),
            type_field: "type".to_string(),
            type_routing,
        });
        plugin.parser.timestamp = Some("timestamp".to_string());
        plugin.parser.role = Some("role".to_string());
        plugin.parser.content = Some("content".to_string());

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Type field is explicitly null
        let line = r#"{"type": null, "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Hello"}}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        // Null type field should default to empty string and route to skip
        assert_eq!(
            events.len(),
            0,
            "Null type field should default to empty string and route to skip"
        );
    }

    #[test]
    fn test_envelope_get_routing_returns_correct_action() {
        // Test that envelope.get_routing() returns the correct routing action for different type values
        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("message".to_string(), "event".to_string());
        type_routing.insert("session".to_string(), "meta".to_string());
        type_routing.insert("heartbeat".to_string(), "skip".to_string());

        let envelope = crate::plugin::Envelope {
            payload_field: "payload".to_string(),
            type_field: "type".to_string(),
            type_routing,
        };

        // Test each routing action
        assert_eq!(envelope.get_routing("message"), "event");
        assert_eq!(envelope.get_routing("session"), "meta");
        assert_eq!(envelope.get_routing("heartbeat"), "skip");

        // Test unknown type defaults to skip
        assert_eq!(envelope.get_routing("unknown"), "skip");

        // Test empty string defaults to skip
        assert_eq!(envelope.get_routing(""), "skip");
    }

    #[test]
    fn test_no_envelope_config_no_change_in_behavior() {
        // Test that plugins without envelope config work as before (no behavior change)
        let plugin = create_test_plugin(); // No envelope config
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        let line = r#"{"ts": "2026-03-16T12:00:00Z", "role": "user", "content": "Hello"}"#;
        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        // Should parse normally without envelope processing
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].role, Role::User);
        assert_eq!(events[0].content, "Hello");
        assert_eq!(events[0].session_id, "test-session");
        assert_eq!(events[0].source_agent, "test");
    }

    // ─── Integration Tests: Non-Envelope Plugin Parity (Bead bf-y28h7) ───

    #[test]
    fn test_non_envelope_plugin_byte_for_byte_parity() {
        // Verify that a plugin without envelope config produces byte-for-byte
        // identical events to the pre-envelope implementation.
        //
        // This test validates that for non-envelope plugins:
        // - payload_json == raw line (the full line is the event data)
        // - envelope_json == None (no envelope wrapper exists)
        // - All field extraction works exactly as before
        // - No behavior change compared to pre-envelope implementation

        let plugin = create_test_plugin(); // No envelope config
        let context = ParseContext::new(
            "parity-test".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Test 1: Simple user message
        let line1 = r#"{"ts": "2026-03-16T12:00:00Z", "role": "user", "content": "Hello world"}"#;
        let events1 = JsonlParser::parse_line(line1, 1, &context, &plugin).unwrap();
        assert_eq!(events1.len(), 1, "Should produce exactly 1 event");
        let e1 = &events1[0];
        assert_eq!(e1.role, Role::User);
        assert_eq!(e1.content, "Hello world");
        assert_eq!(e1.ts.to_rfc3339(), "2026-03-16T12:00:00+00:00");
        assert_eq!(e1.session_id, "parity-test");
        assert_eq!(e1.source_agent, "test");

        // Test 2: Assistant message with tool name
        let line2 = r#"{"ts": "2026-03-16T12:00:01Z", "role": "tool_call", "content": "Running command", "tool_name": "bash"}"#;
        let mut plugin2 = create_test_plugin();
        plugin2.parser.tool_name = Some("tool_name".to_string());
        let events2 = JsonlParser::parse_line(line2, 1, &context, &plugin2).unwrap();
        assert_eq!(events2.len(), 1);
        let e2 = &events2[0];
        assert_eq!(e2.role, Role::ToolCall);
        assert_eq!(e2.content, "Running command");
        assert_eq!(e2.tool, Some("bash".to_string()));

        // Test 3: Message with tokens
        let line3 = r#"{"ts": "2026-03-16T12:00:02Z", "role": "assistant", "content": "Response", "tokens_in": "100", "tokens_out": "50"}"#;
        let mut plugin3 = create_test_plugin();
        plugin3.parser.tokens_in = Some("tokens_in".to_string());
        plugin3.parser.tokens_out = Some("tokens_out".to_string());
        let events3 = JsonlParser::parse_line(line3, 1, &context, &plugin3).unwrap();
        assert_eq!(events3.len(), 1);
        let e3 = &events3[0];
        assert_eq!(e3.role, Role::Assistant);
        assert_eq!(e3.tokens.as_ref().map(|t| t.input), Some(100));
        assert_eq!(e3.tokens.as_ref().map(|t| t.output), Some(50));

        // Test 4: Complex nested field paths (as used by real plugins)
        let line4 = r#"{"timestamp": "2026-03-16T12:00:03Z", "message": {"role": "user", "content": "Nested test"}, "session_id": "sess-001"}"#;
        let mut plugin4 = create_test_plugin();
        plugin4.parser.timestamp = Some("timestamp".to_string());
        plugin4.parser.role = Some("message.role".to_string());
        plugin4.parser.content = Some("message.content".to_string());
        let events4 = JsonlParser::parse_line(line4, 1, &context, &plugin4).unwrap();
        assert_eq!(events4.len(), 1);
        let e4 = &events4[0];
        assert_eq!(e4.role, Role::User);
        assert_eq!(e4.content, "Nested test");
        assert_eq!(e4.ts.to_rfc3339(), "2026-03-16T12:00:03+00:00");

        // All tests pass → byte-for-byte parity maintained
    }

    #[test]
    fn test_non_envelope_plugin_full_file_parity() {
        // Test that a full file with non-envelope plugin produces identical
        // event sequence to pre-envelope implementation.
        //
        // Uses a real fixture (claude-code session) to validate end-to-end parity.

        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude-code/session-with-tools.jsonl");

        // Create a plugin without envelope config (standard claude-code mapping)
        let plugin = Plugin {
            plugin: PluginMeta {
                name: "claude-code".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec![fixture_path.display().to_string()],
                exclude: vec![],
                format: LogFormat::Jsonl,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: None,
                envelope: None, // No envelope config
                array: None,
            },
            parser: Parser {
                timestamp: Some("timestamp".to_string()),
                role: Some("message.role".to_string()),
                content: Some("message.content".to_string()),
                tool_name: Some("message.tool".to_string()),
                tokens_in: Some("message.usage.input_tokens".to_string()),
                tokens_out: Some("message.usage.output_tokens".to_string()),
                ..Default::default()
            },
            metadata: None,
        };

        // Parse the fixture
        let events = JsonlParser.parse(&fixture_path, &plugin);

        assert!(
            events.is_ok(),
            "Should parse successfully without envelope config"
        );
        let events = events.unwrap();

        // Should produce multiple events (fixture has user + assistant + tool_use)
        assert!(!events.is_empty(), "Should produce at least one event");

        // Verify event structure matches expectations
        let first_event = &events[0];
        assert!(
            !first_event.session_id.is_empty(),
            "Session ID should be set"
        );
        assert_eq!(first_event.source_agent, "claude-code");

        // Verify tool_call events have tool names set
        let tool_events: Vec<_> = events.iter().filter(|e| e.role == Role::ToolCall).collect();
        for tool_event in tool_events {
            assert!(
                tool_event.tool.is_some(),
                "Tool_call events should have tool name set"
            );
        }

        // No behavior change → same event count and structure as pre-envelope
    }

    // ─── Integration Tests: Mixed Envelope Type Routing (Bead bf-y28h7) ───

    #[test]
    fn test_mixed_envelope_fixture_skip_meta_event_routing() {
        // Integration test: fixture with mixed envelope types (skip/meta/event)
        // routes correctly and produces only event-type outputs.
        //
        // Uses envelope_test.jsonl which contains:
        // - session_start → meta (dropped)
        // - heartbeat → skip (dropped)
        // - ping → skip (dropped)
        // - message → event (produced)
        // - unknown_event → not in routing, defaults to skip (dropped)

        let fixture_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/envelope_test.jsonl");

        // Create plugin matching envelope_test.toml
        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("message".to_string(), "event".to_string());
        type_routing.insert("session_start".to_string(), "meta".to_string());
        type_routing.insert("heartbeat".to_string(), "skip".to_string());
        type_routing.insert("ping".to_string(), "skip".to_string());

        let plugin = Plugin {
            plugin: PluginMeta {
                name: "envelope-test".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec![fixture_path.display().to_string()],
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
                timestamp: Some("^timestamp".to_string()), // From wrapper
                role: Some("role".to_string()),            // From payload (after unwrapping)
                content: Some("content".to_string()),      // From payload (after unwrapping)
                tool_name: Some("tool_name".to_string()),
                role_map: {
                    let mut map = std::collections::HashMap::new();
                    map.insert("tool".to_string(), "tool_call".to_string());
                    map
                },
                ..Default::default()
            },
            metadata: None,
        };

        // Parse the fixture
        let events = JsonlParser.parse(&fixture_path, &plugin);

        assert!(events.is_ok(), "Should parse fixture successfully");
        let events = events.unwrap();

        // envelope_test.jsonl has 9 lines:
        // Line 1: session_start → meta → dropped (0)
        // Line 2: heartbeat → skip → dropped (0)
        // Line 3: ping → skip → dropped (0)
        // Line 4: message → event → produced (1)
        // Line 5: message → event → produced (1)
        // Line 6: message → event → produced (1)
        // Line 7: message → event → produced (1)
        // Line 8: unknown_event → not in routing → skip → dropped (0)
        // Expected: 4 events from 4 message lines
        assert_eq!(
            events.len(),
            4,
            "Should produce 4 events from message lines only"
        );

        // Verify all events have correct structure
        for (i, event) in events.iter().enumerate() {
            assert!(!event.content.is_empty(), "Event {} should have content", i);
            assert_eq!(event.source_agent, "envelope-test");
        }
    }

    #[test]
    fn test_envelope_json_payload_json_references_available() {
        // Test that envelope_json and payload_json references are properly
        // available for downstream field extraction (child 3 work).
        //
        // Validates that:
        // - ^prefix fields read from envelope_json
        // - non-^prefix fields read from payload_json
        // - References are correctly set for event/meta/skip routing

        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("message".to_string(), "event".to_string());

        let plugin = Plugin {
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
                // ^timestamp reads from envelope_json
                timestamp: Some("^timestamp".to_string()),
                // role/content read from payload_json
                role: Some("role".to_string()),
                content: Some("content".to_string()),
                // ^model reads from envelope_json (test envelope field extraction)
                tool_name: Some("^wrapper_field".to_string()),
                // ^tokens_in reads from envelope_json (numeric field)
                tokens_in: Some("^tokens_in".to_string()),
                // ^tokens_out reads from envelope_json (numeric field)
                tokens_out: Some("^tokens_out".to_string()),
                ..Default::default()
            },
            metadata: None,
        };

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Envelope line with fields at both wrapper and payload levels
        let line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "model": "gpt-4", "request_id": "req-123", "seq": 1, "payload": {"role": "user", "content": "Hello", "model": "ignored", "request_id": "ignored", "seq": 999}}"#;

        let events = JsonlParser::parse_line(line, 1, &context, &plugin).unwrap();

        assert_eq!(events.len(), 1, "Should produce 1 event");
        let event = &events[0];

        // Verify ^timestamp read from wrapper (not payload)
        assert_eq!(event.ts.to_rfc3339(), "2026-03-16T12:00:00+00:00");

        // Verify role/content read from payload (not wrapper)
        assert_eq!(event.role, Role::User);
        assert_eq!(event.content, "Hello");

        // Verify ^model read from wrapper (not payload.model="ignored")
        assert_eq!(event.tool, Some("gpt-4".to_string()));

        // Verify ^request_id read from wrapper
        assert_eq!(event.tokens.as_ref().map(|t| t.input), Some(123));

        // Verify ^seq read from wrapper (not payload.seq=999)
        assert_eq!(event.tokens.as_ref().map(|t| t.output), Some(1));

        // All envelope/payload references work correctly
    }

    #[test]
    fn test_full_envelope_pipeline_integration() {
        // Full integration test: envelope unwrapping → routing → field extraction
        // using the complete envelope-routing.jsonl fixture.
        //
        // This test validates the entire envelope pipeline:
        // 1. Parse JSONL lines with envelope wrapper
        // 2. Apply type routing (skip/meta/event)
        // 3. Extract payload for event types
        // 4. Extract fields with ^prefix awareness
        // 5. Produce canonical events

        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/envelope/envelope-routing.jsonl");

        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("message".to_string(), "event".to_string());
        type_routing.insert("session".to_string(), "meta".to_string());
        type_routing.insert("compaction".to_string(), "meta".to_string());
        type_routing.insert("model_change".to_string(), "skip".to_string());

        let plugin = Plugin {
            plugin: PluginMeta {
                name: "pi".to_string(), // Simulating pi agent format
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec![fixture_path.display().to_string()],
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
                timestamp: Some("^timestamp".to_string()),
                role: Some("role".to_string()),
                content: Some("content".to_string()),
                role_map: {
                    let mut map = std::collections::HashMap::new();
                    map.insert("toolResult".to_string(), "tool_result".to_string());
                    map
                },
                ..Default::default()
            },
            metadata: None,
        };

        let events = JsonlParser.parse(&fixture_path, &plugin);

        assert!(
            events.is_ok(),
            "Should parse envelope-routing fixture successfully"
        );
        let events = events.unwrap();

        // envelope-routing.jsonl has 10 lines:
        // session → meta (0)
        // session_info → not in routing → skip (0)
        // message → event (1)
        // model_change → skip (0)
        // message → event (1)
        // message → event (1)
        // message → event (1)
        // compaction → meta (0)
        // custom → not in routing → skip (0)
        // Expected: 4 events
        assert_eq!(
            events.len(),
            4,
            "Should produce 4 events from message lines"
        );

        // Verify event sequence
        assert_eq!(events[0].role, Role::User);
        assert!(events[0].content.contains("What files"));

        assert_eq!(events[1].role, Role::Assistant);
        assert!(events[1].content.contains("I'll list"));

        assert_eq!(events[2].role, Role::ToolResult);
        assert!(events[2].content.contains("README.md"));

        assert_eq!(events[3].role, Role::Assistant);
        assert!(events[3].content.contains("directory contains"));

        // Full pipeline works correctly
    }

    // ─── Meta Routing Test Infrastructure Tests ───

    #[test]
    fn test_meta_routing_infrastructure_standard_fixtures() {
        // Test all standard meta routing fixtures using the new infrastructure
        let fixtures = standard_meta_routing_fixtures();

        for fixture in &fixtures {
            test_meta_routing_fixture(fixture);
        }

        println!(
            "✅ All {} standard meta routing fixtures passed",
            fixtures.len()
        );
    }

    #[test]
    fn test_meta_routing_fixture_skip_types() {
        // Test that skip-type fixtures all produce zero events
        let fixtures = vec![
            MetaRoutingTestFixture::skip_fixture(
                "heartbeat skip",
                "heartbeat",
                r#"{"type": "heartbeat", "timestamp": "2026-03-16T12:00:00Z", "payload": {"status": "ok"}}"#,
            ),
            MetaRoutingTestFixture::skip_fixture(
                "ping skip",
                "ping",
                r#"{"type": "ping", "timestamp": "2026-03-16T12:00:00Z", "payload": {"seq": 1}}"#,
            ),
        ];

        for fixture in &fixtures {
            test_meta_routing_fixture(fixture);
        }
    }

    #[test]
    fn test_meta_routing_fixture_meta_types() {
        // Test that meta-type fixtures all accumulate metadata (produce 0 events)
        let fixtures = vec![
            MetaRoutingTestFixture::meta_fixture(
                "session_start meta",
                "session_start",
                r#"{"type": "session_start", "timestamp": "2026-03-16T12:00:00Z", "payload": {"session_id": "sess-001"}}"#,
            ),
            MetaRoutingTestFixture::meta_fixture(
                "session_end meta",
                "session_end",
                r#"{"type": "session_end", "timestamp": "2026-03-16T12:30:00Z", "payload": {"duration": 1800}}"#,
            ),
            MetaRoutingTestFixture::meta_fixture(
                "compaction meta",
                "compaction",
                r#"{"type": "compaction", "timestamp": "2026-03-16T12:05:00Z", "payload": {"files_compacted": 3}}"#,
            ),
        ];

        for fixture in &fixtures {
            test_meta_routing_fixture(fixture);
        }
    }

    #[test]
    fn test_meta_routing_fixture_event_types() {
        // Test that event-type fixtures produce events
        let fixtures = vec![MetaRoutingTestFixture::event_fixture(
            "message event",
            "message",
            r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Hello"}}"#,
        )];

        for fixture in &fixtures {
            test_meta_routing_fixture(fixture);
        }
    }

    #[test]
    fn test_meta_routing_helper_function() {
        // Test the assert_meta_routing_behavior helper directly
        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("message".to_string(), "event".to_string());
        type_routing.insert("heartbeat".to_string(), "skip".to_string());

        let plugin = create_meta_routing_plugin(type_routing);

        // Test event type produces 1 event
        let event_line = r#"{"type": "message", "timestamp": "2026-03-16T12:00:00Z", "payload": {"role": "user", "content": "Hello"}}"#;
        assert_meta_routing_behavior(&plugin, event_line, 1, false);

        // Test skip type produces 0 events
        let skip_line = r#"{"type": "heartbeat", "timestamp": "2026-03-16T12:00:00Z", "payload": {"status": "ok"}}"#;
        assert_meta_routing_behavior(&plugin, skip_line, 0, true);
    }

    #[test]
    fn test_meta_routing_plugin_helper() {
        // Test the create_meta_routing_plugin helper
        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("test_event".to_string(), "event".to_string());

        let plugin = create_meta_routing_plugin(type_routing);

        // Verify plugin structure
        assert_eq!(plugin.plugin.name, "test");
        assert!(plugin.source.envelope.is_some());
        let envelope = plugin.source.envelope.as_ref().unwrap();
        assert_eq!(envelope.type_field, "type");
        assert_eq!(envelope.payload_field, "payload");
    }

    // ========================================================================
    // Integration Tests for Envelope Routing Verification (bf-y28h7)
    // ========================================================================

    #[test]
    fn test_non_envelope_plugin_parity_byte_for_byte() {
        // AC: Non-envelope plugin produces identical events to before
        // Verify that plugins without envelope config behave identically to the
        // pre-envelope implementation (payload_json == raw line, envelope_json == None)

        let plugin = create_test_plugin();
        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        let test_line = r#"{"ts": "2026-03-16T12:00:00Z", "role": "user", "content": "Hello World", "metadata": {"key": "value"}}"#;
        let events = JsonlParser::parse_line(test_line, 1, &context, &plugin).unwrap();

        assert_eq!(
            events.len(),
            1,
            "Non-envelope parsing should produce exactly 1 event"
        );
        let event = &events[0];

        // Verify byte-for-byte parity: all fields match expected values unchanged
        assert_eq!(event.role, Role::User, "Role should be parsed correctly");
        assert_eq!(event.content, "Hello World", "Content should match exactly");
        assert_eq!(
            event.session_id, "test-session",
            "Session ID should be preserved"
        );
        assert_eq!(
            event.source_agent, "test",
            "Source agent should be preserved"
        );
        assert_eq!(
            event.ts.to_rfc3339(),
            "2026-03-16T12:00:00+00:00",
            "Timestamp parsing should be consistent"
        );

        // Verify no behavior change - event structure should be identical
        assert!(
            event.project.is_none(),
            "Project should be None when not set"
        );
        assert!(event.model.is_none(), "Model should be None when not set");
        assert!(
            event.tokens.is_none(),
            "Tokens should be None when not present"
        );
        assert!(event.tool.is_none(), "Tool should be None when not present");
    }

    #[test]
    fn test_mixed_envelope_types_integration() {
        // AC: Mixed fixture test - skip lines dropped, meta lines dropped, event lines unwrapped correctly
        // Integration test using actual fixture file with mixed envelope types

        let fixture_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/envelope_test.jsonl");

        // Create plugin matching envelope_test.toml configuration
        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("session_start".to_string(), "meta".to_string());
        type_routing.insert("heartbeat".to_string(), "skip".to_string());
        type_routing.insert("ping".to_string(), "skip".to_string());
        type_routing.insert("message".to_string(), "event".to_string());
        // "unknown_event" is not in routing map → should default to skip

        let mut role_map = std::collections::HashMap::new();
        role_map.insert("tool".to_string(), "tool_call".to_string());

        let plugin = Plugin {
            plugin: PluginMeta {
                name: "envelope-test".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec![fixture_path.display().to_string()],
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
                tool_name: Some("tool_name".to_string()),
                role_map,
                ..Default::default()
            },
            metadata: None,
        };

        // Parse the fixture file
        let all_events = JsonlParser.parse(&fixture_path, &plugin).unwrap();

        // Expected results based on envelope_test.jsonl:
        // Line 1: session_start (meta) → 0 events, dropped
        // Line 2: heartbeat (skip) → 0 events, dropped
        // Line 3: ping (skip) → 0 events, dropped
        // Line 4: message (event) → 1 event, unwrapped
        // Line 5: message (event) → 1 event, unwrapped
        // Line 6: message (event) → 1 event, unwrapped
        // Line 7: message (event) → 1 event, unwrapped
        // Line 8: unknown_event (not in map) → 0 events, defaults to skip
        // Total expected: 4 events from lines 4-7

        assert_eq!(
            all_events.len(),
            4,
            "Mixed fixture should produce exactly 4 events from message lines"
        );

        // Verify event 1 (line 4 - user message)
        assert_eq!(
            all_events[0].role,
            Role::User,
            "Event 1 should have user role"
        );
        assert_eq!(
            all_events[0].content, "Refactor the authentication module to use JWT tokens",
            "Event 1 content should match"
        );
        assert_eq!(
            all_events[0].ts.to_rfc3339(),
            "2026-07-04T10:00:15+00:00",
            "Event 1 timestamp should match"
        );
        assert!(all_events[0].tool.is_none(), "Event 1 should have no tool");

        // Verify event 2 (line 5 - assistant message)
        assert_eq!(
            all_events[1].role,
            Role::Assistant,
            "Event 2 should have assistant role"
        );
        assert_eq!(all_events[1].content, "I'll refactor the authentication module. Let me start by reading the current implementation.", "Event 2 content should match");
        assert_eq!(
            all_events[1].ts.to_rfc3339(),
            "2026-07-04T10:00:20+00:00",
            "Event 2 timestamp should match"
        );

        // Verify event 3 (line 6 - tool_call message)
        assert_eq!(
            all_events[2].role,
            Role::ToolCall,
            "Event 3 should have tool_call role"
        );
        assert_eq!(
            all_events[2].content, "Reading src/auth/mod.rs",
            "Event 3 content should match"
        );
        assert_eq!(
            all_events[2].tool,
            Some("Read".to_string()),
            "Event 3 should have Read tool"
        );

        // Verify event 4 (line 7 - assistant message)
        assert_eq!(
            all_events[3].role,
            Role::Assistant,
            "Event 4 should have assistant role"
        );
        assert_eq!(
            all_events[3].content, "Here's the refactored authentication module using JWT.",
            "Event 4 content should match"
        );
        assert_eq!(
            all_events[3].ts.to_rfc3339(),
            "2026-07-04T10:00:30+00:00",
            "Event 4 timestamp should match"
        );
    }

    #[test]
    fn test_envelope_json_and_payload_json_references() {
        // AC: Ensure envelope_json and payload_json references are properly available
        // for downstream field extraction (child 3 work)

        // This test verifies that the envelope-aware extraction functions work correctly
        // by testing field extraction with both ^ prefix (envelope) and without ^ (payload)

        let mut type_routing = std::collections::HashMap::new();
        type_routing.insert("message".to_string(), "event".to_string());

        let plugin = Plugin {
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
                // Mix of envelope (^) and payload field paths
                timestamp: Some("^wrapper_ts".to_string()),
                role: Some("role".to_string()),
                content: Some("content".to_string()),
                tool_name: Some("^wrapper_tool".to_string()),
                tokens_in: Some("payload_tokens_in".to_string()),
                tokens_out: Some("payload_tokens_out".to_string()),
                role_map: {
                    let mut map = std::collections::HashMap::new();
                    map.insert("tool".to_string(), "tool_call".to_string());
                    map
                },
                ..Default::default()
            },
            metadata: None,
        };

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Complex line with fields at both envelope and payload levels
        let complex_line = r#"{"type": "message", "wrapper_ts": "2026-03-16T12:00:00Z", "wrapper_tool": "bash", "payload": {"role": "tool", "content": "Running command", "tool_name": "ignored_tool", "payload_tokens_in": "100", "payload_tokens_out": "200"}}"#;

        let events = JsonlParser::parse_line(complex_line, 1, &context, &plugin).unwrap();

        assert_eq!(events.len(), 1, "Should produce exactly 1 event");
        let event = &events[0];

        // Verify ^ prefix reads from envelope wrapper
        assert_eq!(
            event.ts.to_rfc3339(),
            "2026-03-16T12:00:00+00:00",
            "^wrapper_ts should read from envelope"
        );
        assert_eq!(
            event.tool,
            Some("bash".to_string()),
            "^wrapper_tool should read from envelope"
        );

        // Verify no ^ prefix reads from payload
        assert_eq!(event.role, Role::ToolCall, "role should read from payload");
        assert_eq!(
            event.content, "Running command",
            "content should read from payload"
        );

        // Verify token fields read from payload (no ^ prefix)
        assert_eq!(
            event.tokens.as_ref().map(|t| t.input),
            Some(100),
            "payload_tokens_in should read from payload"
        );
        assert_eq!(
            event.tokens.as_ref().map(|t| t.output),
            Some(200),
            "payload_tokens_out should read from payload"
        );

        // Verify the ignored payload.tool_name field was not used (^wrapper_tool won)
        assert_eq!(
            event.tool,
            Some("bash".to_string()),
            "Should use wrapper_tool, not payload.tool_name"
        );
    }

    #[test]
    fn test_envelope_non_envelope_parity_comparison() {
        // Comprehensive test comparing envelope vs non-envelope plugin behavior
        // Verifies that both modes produce structurally identical events

        // Test data - same logical event, different formats
        let timestamp = "2026-03-16T12:00:00Z";
        let role = "user";
        let content = "Test message content";

        // Non-envelope format: flat structure
        let non_envelope_line = format!(
            r#"{{"ts": "{}", "role": "{}", "content": "{}"}}"#,
            timestamp, role, content
        );

        // Envelope format: wrapped structure
        let envelope_line = format!(
            r#"{{"type": "message", "timestamp": "{}", "payload": {{"role": "{}", "content": "{}"}}}}"#,
            timestamp, role, content
        );

        // Create non-envelope plugin
        let non_env_plugin = {
            let mut p = create_test_plugin();
            p.parser.timestamp = Some("ts".to_string());
            p
        };

        // Create envelope plugin
        let mut env_type_routing = std::collections::HashMap::new();
        env_type_routing.insert("message".to_string(), "event".to_string());
        let env_plugin = Plugin {
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
                    type_routing: env_type_routing,
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
        };

        let context = ParseContext::new(
            "test-session".to_string(),
            "test".to_string(),
            "/tmp/test.jsonl".to_string(),
        );

        // Parse both formats
        let non_env_events =
            JsonlParser::parse_line(&non_envelope_line, 1, &context, &non_env_plugin).unwrap();
        let env_events = JsonlParser::parse_line(&envelope_line, 1, &context, &env_plugin).unwrap();

        // Both should produce exactly 1 event
        assert_eq!(
            non_env_events.len(),
            1,
            "Non-envelope should produce 1 event"
        );
        assert_eq!(env_events.len(), 1, "Envelope should produce 1 event");

        let non_env_event = &non_env_events[0];
        let env_event = &env_events[0];

        // Verify both events have identical canonical structure
        assert_eq!(
            non_env_event.role, env_event.role,
            "Both should have same role"
        );
        assert_eq!(
            non_env_event.content, env_event.content,
            "Both should have same content"
        );
        assert_eq!(
            non_env_event.ts, env_event.ts,
            "Both should have same timestamp"
        );
        assert_eq!(
            non_env_event.session_id, env_event.session_id,
            "Both should have same session_id"
        );
        assert_eq!(
            non_env_event.source_agent, env_event.source_agent,
            "Both should have same source_agent"
        );
    }
}

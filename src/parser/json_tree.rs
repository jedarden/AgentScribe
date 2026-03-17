//! JSON tree format parser
//!
//! Parses JSON files organized in a directory tree structure.
//! Used by agents like OpenCode.

use crate::event::{Event, Role};
use crate::parser::{extract_string, SessionInfo};
use crate::plugin::{Plugin, TreeConfig};
use crate::error::{AgentScribeError, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::path::Path;
use std::collections::HashMap;
use glob::Pattern;

/// JSON tree parser implementation
pub struct JsonTreeParser;

/// A session in the tree
pub struct TreeSession {
    id: String,
    project_id: Option<String>,
    messages: Vec<String>, // message IDs
    metadata: Value,
}

/// A message in the tree
pub struct TreeMessage {
    id: String,
    session_id: String,
    parts: Vec<String>, // part IDs
    timestamp: Option<DateTime<Utc>>,
    role: Option<String>,
    metadata: Value,
}

/// A part in the tree (actual content)
pub struct TreePart {
    id: String,
    message_id: String,
    content_type: Option<String>,
    text: Option<String>,
    tool_name: Option<String>,
    metadata: Value,
}

impl JsonTreeParser {
    /// Load all sessions from the tree structure
    pub fn load_tree(base_path: &Path, config: &TreeConfig) -> Result<HashMap<String, TreeSession>> {
        let mut sessions = HashMap::new();

        // Find all session files
        let session_pattern = Pattern::new(&config.session_glob.replace("{projectId}", "*").replace("{sessionId}", "*"))
            .map_err(|e| AgentScribeError::InvalidPlugin(format!("Invalid session glob: {}", e)))?;

        // Walk the directory tree
        for entry in walkdir::WalkDir::new(base_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let rel_path = path.strip_prefix(base_path).unwrap_or(path);
            let path_str = rel_path.to_str().unwrap_or("");

            if !session_pattern.matches(path_str) {
                continue;
            }

            // Extract session ID from path
            let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
            let session_id = filename.to_string();

            // Load session metadata
            let content: Value = std::fs::read(path)
                .ok()
                .and_then(|d| serde_json::from_slice(&d).ok())
                .unwrap_or(Value::Null);

            let project_id = extract_string(&content, "projectId");
            let messages: Vec<String> = content
                .get("messageIds")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            sessions.insert(
                session_id.clone(),
                TreeSession {
                    id: session_id,
                    project_id,
                    messages,
                    metadata: content,
                },
            );
        }

        Ok(sessions)
    }

    /// Load all messages from the tree
    pub fn load_messages(base_path: &Path, config: &TreeConfig) -> Result<HashMap<String, TreeMessage>> {
        let mut messages = HashMap::new();

        let message_pattern = Pattern::new(&config.message_glob.replace("{sessionId}", "*").replace("{messageId}", "*"))
            .map_err(|e| AgentScribeError::InvalidPlugin(format!("Invalid message glob: {}", e)))?;

        for entry in walkdir::WalkDir::new(base_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let rel_path = path.strip_prefix(base_path).unwrap_or(path);
            let path_str = rel_path.to_str().unwrap_or("");

            if !message_pattern.matches(path_str) {
                continue;
            }

            let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
            let message_id = filename.to_string();

            // Extract session ID from path using the pattern
            let session_id = Self::extract_id_from_path(path_str, &config.message_glob, "{sessionId}")
                .unwrap_or_else(|| "unknown".to_string());

            let content: Value = std::fs::read(path)
                .ok()
                .and_then(|d| serde_json::from_slice(&d).ok())
                .unwrap_or(Value::Null);

            let parts: Vec<String> = content
                .get("partIds")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            let timestamp = content
                .get("createdAt")
                .and_then(|v| v.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc));

            let role = extract_string(&content, "role");

            messages.insert(
                message_id.clone(),
                TreeMessage {
                    id: message_id,
                    session_id,
                    parts,
                    timestamp,
                    role,
                    metadata: content,
                },
            );
        }

        Ok(messages)
    }

    /// Load all parts from the tree
    pub fn load_parts(base_path: &Path, config: &TreeConfig) -> Result<HashMap<String, TreePart>> {
        let mut parts = HashMap::new();

        let part_pattern = Pattern::new(&config.part_glob.replace("{messageId}", "*").replace("{partId}", "*"))
            .map_err(|e| AgentScribeError::InvalidPlugin(format!("Invalid part glob: {}", e)))?;

        for entry in walkdir::WalkDir::new(base_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let rel_path = path.strip_prefix(base_path).unwrap_or(path);
            let path_str = rel_path.to_str().unwrap_or("");

            if !part_pattern.matches(path_str) {
                continue;
            }

            let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
            let part_id = filename.to_string();

            let message_id = Self::extract_id_from_path(path_str, &config.part_glob, "{messageId}")
                .unwrap_or_else(|| "unknown".to_string());

            let content: Value = std::fs::read(path)
                .ok()
                .and_then(|d| serde_json::from_slice(&d).ok())
                .unwrap_or(Value::Null);

            let content_type = extract_string(&content, "type");
            let text = extract_string(&content, "text");
            let tool_name = extract_string(&content, "toolName");

            parts.insert(
                part_id.clone(),
                TreePart {
                    id: part_id,
                    message_id,
                    content_type,
                    text,
                    tool_name,
                    metadata: content,
                },
            );
        }

        Ok(parts)
    }

    /// Extract an ID from a file path using a glob pattern
    fn extract_id_from_path(path: &str, glob: &str, placeholder: &str) -> Option<String> {
        // Simple extraction: find the placeholder position in the glob
        // and extract corresponding segment from path
        let glob_parts: Vec<&str> = glob.split('/').collect();
        let path_parts: Vec<&str> = path.split('/').collect();

        for (i, g) in glob_parts.iter().enumerate() {
            if g.contains(placeholder) && i < path_parts.len() {
                let filename = path_parts[i];
                let id = filename.strip_suffix(".json").unwrap_or(filename);
                return Some(id.to_string());
            }
        }

        None
    }
}

impl super::FormatParser for JsonTreeParser {
    fn parse(&self, source_path: &Path, plugin: &Plugin) -> Result<Vec<Event>> {
        let config = plugin.source.tree.as_ref().ok_or_else(|| {
            AgentScribeError::InvalidPlugin("JSON tree format requires [source.tree]".to_string())
        })?;

        // Load the entire tree structure
        let sessions = Self::load_tree(source_path, config)?;
        let messages = Self::load_messages(source_path, config)?;
        let parts = Self::load_parts(source_path, config)?;

        let mut all_events = Vec::new();

        // Process each session
        for (session_id, session) in sessions {
            let prefixed_session_id = format!("{}/{}", plugin.plugin.name, session_id);
            let mut session_events = Vec::new();

            // Process messages in the session
            for message_id in &session.messages {
                if let Some(message) = messages.get(message_id) {
                    // Process parts in the message
                    for part_id in &message.parts {
                        if let Some(part) = parts.get(part_id) {
                            let role = message
                                .role
                                .as_ref()
                                .and_then(|r| Role::from_str(r))
                                .unwrap_or(Role::Assistant);

                            let ts = message.timestamp.unwrap_or_else(Utc::now);
                            let content = part.text.clone().unwrap_or_default();

                            let mut event = Event::new(
                                ts,
                                prefixed_session_id.clone(),
                                plugin.plugin.name.clone(),
                                role,
                                content,
                            );

                            if role == Role::ToolCall || role == Role::ToolResult {
                                event.tool = part.tool_name.clone();
                            }

                            session_events.push((ts, event));
                        }
                    }
                }
            }

            // Sort by timestamp
            session_events.sort_by_key(|(ts, _)| *ts);
            all_events.extend(session_events.into_iter().map(|(_, e)| e));
        }

        Ok(all_events)
    }

    fn detect_sessions(&self, source_path: &Path, plugin: &Plugin) -> Result<Vec<SessionInfo>> {
        let config = plugin.source.tree.as_ref().ok_or_else(|| {
            AgentScribeError::InvalidPlugin("JSON tree format requires [source.tree]".to_string())
        })?;

        let sessions = Self::load_tree(source_path, config)?;

        let mut session_infos = Vec::new();
        for (session_id, session) in sessions {
            session_infos.push(SessionInfo {
                session_id,
                start_offset: 0,
                end_offset: 0, // N/A for tree structure
                metadata: Some(session.metadata),
            });
        }

        Ok(session_infos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_id_from_path() {
        let path = "session/abc123/session-456.json";
        let glob = "session/{projectId}/{sessionId}.json";

        let result = JsonTreeParser::extract_id_from_path(path, glob, "{sessionId}");
        assert_eq!(result, Some("session-456".to_string()));
    }
}

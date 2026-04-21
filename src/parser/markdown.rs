//! Markdown format parser
//!
//! Parses append-only Markdown files where roles are distinguished by line prefixes.
//! Used by agents like Aider.

use crate::error::{AgentScribeError, Result};
use crate::event::{Event, Role};
use crate::parser::{ParseContext, SessionInfo};
use crate::plugin::{Plugin, SessionDetection};
use chrono::Utc;
use regex::Regex;
use std::path::Path;

/// Markdown parser implementation
pub struct MarkdownParser;

impl MarkdownParser {
    /// Parse a markdown file into events
    pub fn parse_content(
        content: &str,
        source_path: &Path,
        session_id: String,
        plugin: &Plugin,
    ) -> Result<Vec<Event>> {
        let parser = Self;
        let context = ParseContext::new(
            session_id,
            plugin.plugin.name.clone(),
            source_path.display().to_string(),
        );

        parser.parse_markdown(content, &context, plugin)
    }

    fn parse_markdown(
        &self,
        content: &str,
        context: &ParseContext,
        plugin: &Plugin,
    ) -> Result<Vec<Event>> {
        let mut events = Vec::new();
        let mut current_role: Option<Role> = None;
        let mut current_content = Vec::new();
        let mut event_ts = Utc::now(); // Default timestamp

        let user_prefix = plugin
            .parser
            .user_prefix
            .as_ref()
            .ok_or_else(|| {
                AgentScribeError::InvalidPlugin("Missing user_prefix in parser config".to_string())
            })?
            .clone();

        let assistant_prefix = plugin
            .parser
            .assistant_prefix
            .as_ref()
            .ok_or_else(|| {
                AgentScribeError::InvalidPlugin(
                    "Missing assistant_prefix in parser config".to_string(),
                )
            })?
            .clone();

        let tool_prefix = plugin.parser.tool_prefix.clone().unwrap_or_default();
        let system_prefix = plugin.parser.system_prefix.clone().unwrap_or_default();

        // Timestamp pattern if available
        let ts_regex = plugin
            .parser
            .timestamp_pattern
            .as_ref()
            .and_then(|p| Regex::new(p).ok());

        for line in content.lines() {
            // Check for delimiter (new session marker)
            if let SessionDetection::Delimiter { delimiter_pattern } =
                &plugin.source.session_detection
            {
                if let Ok(re) = Regex::new(delimiter_pattern) {
                    if re.is_match(line) {
                        // Flush current event before new session
                        if let Some(role) = current_role.take() {
                            if !current_content.is_empty() {
                                events.push(Event::new(
                                    event_ts,
                                    context.session_id.clone(),
                                    context.source_agent.clone(),
                                    role,
                                    current_content.join("\n"),
                                ));
                            }
                        }
                        current_content.clear();
                        continue;
                    }
                }
            }

            // Detect role from prefix
            let detected_role = if !user_prefix.is_empty() && line.starts_with(&user_prefix) {
                Some(Role::User)
            } else if !assistant_prefix.is_empty() && line.starts_with(&assistant_prefix) {
                Some(Role::Assistant)
            } else if !tool_prefix.is_empty() && line.starts_with(&tool_prefix) {
                Some(Role::ToolResult)
            } else if !system_prefix.is_empty() && line.starts_with(&system_prefix) {
                Some(Role::System)
            } else {
                None
            };

            if let Some(role) = detected_role {
                // Flush previous event
                if let Some(prev_role) = current_role.take() {
                    if !current_content.is_empty() {
                        events.push(Event::new(
                            event_ts,
                            context.session_id.clone(),
                            context.source_agent.clone(),
                            prev_role,
                            current_content.join("\n"),
                        ));
                    }
                    current_content.clear();
                }

                current_role = Some(role);
                // Strip prefix from content
                let content_line = if role == Role::User && !user_prefix.is_empty() {
                    line.strip_prefix(&user_prefix).unwrap_or(line)
                } else if role == Role::Assistant && !assistant_prefix.is_empty() {
                    line.strip_prefix(&assistant_prefix).unwrap_or(line)
                } else if role == Role::ToolResult && !tool_prefix.is_empty() {
                    line.strip_prefix(&tool_prefix).unwrap_or(line)
                } else if role == Role::System && !system_prefix.is_empty() {
                    line.strip_prefix(&system_prefix).unwrap_or(line)
                } else {
                    line
                };
                current_content.push(content_line.to_string());
            } else if current_role.is_some() {
                // Continuation of current event
                current_content.push(line.to_string());
            }

            // Try to extract timestamp from line
            if let Some(ref re) = ts_regex {
                if let Some(caps) = re.captures(line) {
                    if let Some(ts_str) = caps.get(1) {
                        if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(ts_str.as_str()) {
                            event_ts = ts.with_timezone(&Utc);
                        }
                    }
                }
            }
        }

        // Flush final event
        if let Some(role) = current_role {
            if !current_content.is_empty() {
                events.push(Event::new(
                    event_ts,
                    context.session_id.clone(),
                    context.source_agent.clone(),
                    role,
                    current_content.join("\n"),
                ));
            }
        }

        Ok(events)
    }
}

impl super::FormatParser for MarkdownParser {
    fn parse(&self, source_path: &Path, plugin: &Plugin) -> Result<Vec<Event>> {
        let content = std::fs::read_to_string(source_path)?;

        // For delimiter-based detection, we need to parse sessions
        // For Phase 1, we'll treat the whole file as one session for simplicity
        // Full implementation would split on delimiters

        let session_id = source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        Self::parse_content(&content, source_path, session_id, plugin)
    }

    fn detect_sessions(&self, source_path: &Path, plugin: &Plugin) -> Result<Vec<SessionInfo>> {
        match &plugin.source.session_detection {
            SessionDetection::Delimiter { delimiter_pattern } => {
                let content = std::fs::read_to_string(source_path)?;
                let re = Regex::new(delimiter_pattern).map_err(|e| {
                    AgentScribeError::InvalidPlugin(format!("Invalid delimiter regex: {}", e))
                })?;

                let mut sessions = Vec::new();
                let mut current_offset = 0u64;
                let mut session_num = 0u32;

                for line in content.lines() {
                    let line_bytes = line.len() as u64 + 1; // +1 for newline
                    if re.is_match(line) {
                        sessions.push(SessionInfo {
                            session_id: format!(
                                "{}-{}",
                                source_path
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("unknown"),
                                session_num
                            ),
                            start_offset: current_offset,
                            end_offset: current_offset + line_bytes,
                            metadata: None,
                        });
                        session_num += 1;
                    }
                    current_offset += line_bytes;
                }

                // If no delimiters found, treat whole file as one session
                if sessions.is_empty() {
                    let file_size = std::fs::metadata(source_path)?.len();
                    sessions.push(SessionInfo {
                        session_id: source_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        start_offset: 0,
                        end_offset: file_size,
                        metadata: None,
                    });
                }

                Ok(sessions)
            }
            _ => {
                // Default: one file = one session
                let file_size = std::fs::metadata(source_path)?.len();
                Ok(vec![SessionInfo {
                    session_id: source_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    start_offset: 0,
                    end_offset: file_size,
                    metadata: None,
                }])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{LogFormat, Parser, Plugin, PluginMeta, SessionDetection, Source};

    fn create_aider_plugin() -> Plugin {
        Plugin {
            plugin: PluginMeta {
                name: "aider".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec!["/tmp/.aider.chat.history.md".to_string()],
                exclude: vec![],
                format: LogFormat::Markdown,
                session_detection: SessionDetection::Delimiter {
                    delimiter_pattern: r"^# aider chat started at".to_string(),
                },
                tree: None,
                truncation_limit: None,
            },
            parser: Parser {
                user_prefix: Some("#### ".to_string()),
                assistant_prefix: Some("".to_string()),
                tool_prefix: Some("> ".to_string()),
                ..Default::default()
            },
            metadata: None,
        }
    }

    #[test]
    fn test_parse_aider_markdown() {
        let plugin = create_aider_plugin();
        let content = r#"# aider chat started at 2026-03-16 12:00:00
#### Fix the login bug
I'll help you fix the login bug.
> git status
On branch main
"#;

        let events = MarkdownParser::parse_content(
            content,
            Path::new("/tmp/.aider.chat.history.md"),
            "test-session".to_string(),
            &plugin,
        )
        .unwrap();

        assert!(!events.is_empty());
        assert!(events.iter().any(|e| e.role == Role::User));
    }
}

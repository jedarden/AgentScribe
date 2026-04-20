//! SQLite format parser
//!
//! Parses SQLite databases containing conversation history stored as JSON blobs.
//! Designed for state.vscdb-style databases (cursorDiskKV table pattern).
//!
//! Plugin config requires:
//! - `parser.query`: SQL SELECT query (e.g. `SELECT key, value FROM cursorDiskKV`)
//! - `parser.key_filter`: optional regex to filter on the first column (key)
//! - `parser.content_path`: optional JSON dot-path within the value blob to reach
//!    the message array (e.g. `"messages"`)
//! - `parser.role`, `parser.content`, `parser.timestamp`: JSON paths within each
//!    message object
//!
//! Memory notes:
//! - Opens the DB with SQLITE_OPEN_READ_ONLY — never modifies the source
//! - Sets `PRAGMA cache_size = -8000` (8 MB page cache)
//! - Streams rows via rusqlite iterator, never materialises the full table

use crate::error::{AgentScribeError, Result};
use crate::event::{Event, Role};
use crate::parser::{extract_field, extract_string, parse_timestamp, ParseContext, SessionInfo};
use crate::plugin::{Plugin, SessionDetection, SessionIdSource};
use chrono::Utc;
use regex::Regex;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::Path;

/// SQLite parser implementation
pub struct SqliteParser;

impl SqliteParser {
    /// Open the database read-only and configure memory limits.
    fn open_db(path: &Path) -> Result<Connection> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| AgentScribeError::Parse {
            file: path.display().to_string(),
            line: None,
            message: format!("cannot open SQLite database: {}", e),
        })?;

        // Limit read-cache to 8 MB (negative value → kibibytes)
        conn.execute_batch("PRAGMA cache_size = -8000;")
            .map_err(|e| AgentScribeError::Parse {
                file: path.display().to_string(),
                line: None,
                message: format!("cannot set PRAGMA cache_size: {}", e),
            })?;

        Ok(conn)
    }

    /// Derive session ID from plugin session-detection config.
    fn session_id_from_path(source_path: &Path, plugin: &Plugin) -> String {
        match &plugin.source.session_detection {
            SessionDetection::OneFilePerSession { session_id_from } => match session_id_from {
                SessionIdSource::Filename => source_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
                SessionIdSource::Field(_) => "unknown".to_string(),
            },
            _ => "unknown".to_string(),
        }
    }

    /// Extract a per-key session ID using `key_session_id_regex` (capture group 1).
    /// Returns `None` if the regex doesn't match.
    fn extract_key_session_id(re: &Regex, key: &str) -> Option<String> {
        re.captures(key)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
    }

    /// Parse a JSON value blob from one DB row and return its events.
    ///
    /// `row_num` is used for error location only.
    fn parse_value_blob(
        key: &str,
        value_str: &str,
        context: &ParseContext,
        plugin: &Plugin,
        row_num: usize,
    ) -> Result<Vec<Event>> {
        // Parse the value as JSON.  If it starts with '{' or '[' it is probably
        // JSON; otherwise treat it as a plain-text content string.
        let json: Value = match serde_json::from_str(value_str) {
            Ok(v) => v,
            Err(_) => {
                // Treat the raw string as a single assistant message if we have
                // no role/content path configured.
                if plugin.parser.role.is_none() {
                    let ts = Utc::now();
                    let mut ev = Event::new(
                        ts,
                        context.session_id.clone(),
                        context.source_agent.clone(),
                        Role::Assistant,
                        value_str.to_string(),
                    );
                    ev.project = context.project.clone();
                    ev.model = context.model.clone();
                    return Ok(vec![ev]);
                }
                return Err(AgentScribeError::parse_error_with_line(
                    &context.source_file,
                    row_num,
                    format!("cannot parse JSON blob for key '{}': not valid JSON", key),
                ));
            }
        };

        // Navigate to content_path if configured (e.g. "messages" or "data.turns").
        let messages_val: Value = if let Some(ref path) = plugin.parser.content_path {
            match extract_field(&json, path) {
                Some(v) => v,
                None => return Ok(Vec::new()), // path missing in this blob – skip silently
            }
        } else {
            json.clone()
        };

        // Accept both a JSON array of messages and a single message object.
        let message_list: Vec<Value> = match messages_val {
            Value::Array(arr) => arr,
            Value::Object(_) => vec![messages_val],
            _ => return Ok(Vec::new()),
        };

        let mut events = Vec::new();
        for (msg_idx, msg) in message_list.iter().enumerate() {
            match Self::parse_message(msg, context, plugin, row_num, msg_idx) {
                Ok(Some(ev)) => events.push(ev),
                Ok(None) => {}
                Err(e) if e.is_skippable() => {
                    eprintln!("Warning: {}", e);
                }
                Err(e) => return Err(e),
            }
        }

        Ok(events)
    }

    /// Extract a single `Event` from a message object.  Returns `Ok(None)` when
    /// the message should be silently skipped (unknown role, empty content, etc.).
    fn parse_message(
        msg: &Value,
        context: &ParseContext,
        plugin: &Plugin,
        row_num: usize,
        msg_idx: usize,
    ) -> Result<Option<Event>> {
        // Timestamp — fall back to now() if absent or unparseable.
        let ts = if let Some(ref ts_field) = plugin.parser.timestamp {
            parse_timestamp(msg, ts_field).unwrap_or_else(|_| Utc::now())
        } else {
            Utc::now()
        };

        // Role is required; skip messages without one.
        let role_str = match plugin.parser.role.as_ref().and_then(|f| extract_string(msg, f)) {
            Some(r) => r,
            None => return Ok(None),
        };

        let role = if let Some(mapped) = plugin.parser.role_map.get(&role_str) {
            Role::from_str(mapped).ok_or_else(|| {
                AgentScribeError::parse_error_with_line(
                    &context.source_file,
                    row_num * 10_000 + msg_idx,
                    format!("invalid role_map target: {}", mapped),
                )
            })?
        } else {
            match Role::from_str(&role_str) {
                Some(r) => r,
                None => return Ok(None), // silently skip unknown roles
            }
        };

        // Content — optional; skip empty messages.
        let content = if let Some(ref content_field) = plugin.parser.content {
            extract_string(msg, content_field).unwrap_or_default()
        } else {
            // No content field configured: serialise the whole message object.
            msg.to_string()
        };

        if content.is_empty() {
            return Ok(None);
        }

        let mut ev = Event::new(
            ts,
            context.session_id.clone(),
            context.source_agent.clone(),
            role,
            content,
        );
        ev.project = context.project.clone();
        ev.model = context.model.clone();

        Ok(Some(ev))
    }
}

impl super::FormatParser for SqliteParser {
    fn parse(&self, source_path: &Path, plugin: &Plugin) -> Result<Vec<Event>> {
        let conn = Self::open_db(source_path)?;

        let query_str = plugin
            .parser
            .query
            .as_deref()
            .ok_or_else(|| AgentScribeError::InvalidPlugin("SQLite format requires query".into()))?;

        // Compile the key-filter regex once.
        let key_re: Option<Regex> = plugin
            .parser
            .key_filter
            .as_deref()
            .map(|pat| {
                Regex::new(pat).map_err(|e| {
                    AgentScribeError::InvalidPlugin(format!("invalid key_filter regex: {}", e))
                })
            })
            .transpose()?;

        // Compile the per-key session-ID regex once.
        let key_session_re: Option<Regex> = plugin
            .parser
            .key_session_id_regex
            .as_deref()
            .map(|pat| {
                Regex::new(pat).map_err(|e| {
                    AgentScribeError::InvalidPlugin(format!(
                        "invalid key_session_id_regex: {}",
                        e
                    ))
                })
            })
            .transpose()?;

        let default_session_id = Self::session_id_from_path(source_path, plugin);
        let base_context = ParseContext::new(
            default_session_id.clone(),
            plugin.plugin.name.clone(),
            source_path.display().to_string(),
        );

        let mut stmt = conn.prepare(query_str).map_err(|e| AgentScribeError::Parse {
            file: source_path.display().to_string(),
            line: None,
            message: format!("cannot prepare query: {}", e),
        })?;

        let col_count = stmt.column_count();

        let mut rows = stmt.query([]).map_err(|e| AgentScribeError::Parse {
            file: source_path.display().to_string(),
            line: None,
            message: format!("cannot execute query: {}", e),
        })?;

        let mut events = Vec::new();
        let mut row_num: usize = 0;

        loop {
            let row = rows.next().map_err(|e| AgentScribeError::Parse {
                file: source_path.display().to_string(),
                line: None,
                message: format!("row read error at row {}: {}", row_num, e),
            })?;

            let row = match row {
                Some(r) => r,
                None => break,
            };
            row_num += 1;

            // Column layout:
            //   ≥2 columns → col[0] = key, col[1] = value  (cursorDiskKV pattern)
            //   1 column   → col[0] = value (no key filter applicable)
            let (key, value_str): (String, String) = if col_count >= 2 {
                let k: String = row.get::<_, String>(0).unwrap_or_default();
                let v: String = row.get::<_, String>(1).unwrap_or_default();
                (k, v)
            } else {
                let v: String = row.get::<_, String>(0).unwrap_or_default();
                (String::new(), v)
            };

            // Apply key filter.
            if let Some(ref re) = key_re {
                if !re.is_match(&key) {
                    continue;
                }
            }

            if value_str.is_empty() {
                continue;
            }

            // Derive per-row session ID from the key when key_session_id_regex is set.
            let context = if let Some(ref re) = key_session_re {
                match Self::extract_key_session_id(re, &key) {
                    Some(sid) => {
                        let mut ctx = base_context.clone();
                        ctx.session_id = sid;
                        ctx
                    }
                    None => base_context.clone(),
                }
            } else {
                base_context.clone()
            };

            match Self::parse_value_blob(&key, &value_str, &context, plugin, row_num) {
                Ok(mut row_events) => events.append(&mut row_events),
                Err(e) if e.is_skippable() => {
                    eprintln!("Warning: row {}: {}", row_num, e);
                }
                Err(e) => return Err(e),
            }
        }

        Ok(events)
    }

    fn detect_sessions(&self, source_path: &Path, plugin: &Plugin) -> Result<Vec<SessionInfo>> {
        let file_size = std::fs::metadata(source_path)?.len();

        // When key_session_id_regex is set, query the DB to discover distinct
        // session IDs so the scraper can route events to separate output files.
        if let Some(ref regex_str) = plugin.parser.key_session_id_regex {
            let re = Regex::new(regex_str).map_err(|e| {
                AgentScribeError::InvalidPlugin(format!(
                    "invalid key_session_id_regex: {}",
                    e
                ))
            })?;

            let key_re: Option<Regex> = plugin
                .parser
                .key_filter
                .as_deref()
                .map(|pat| Regex::new(pat))
                .transpose()
                .map_err(|e| {
                    AgentScribeError::InvalidPlugin(format!("invalid key_filter regex: {}", e))
                })?;

            let query_str = plugin
                .parser
                .query
                .as_deref()
                .ok_or_else(|| AgentScribeError::InvalidPlugin("SQLite format requires query".into()))?;

            let conn = Self::open_db(source_path)?;
            let mut stmt = conn.prepare(query_str).map_err(|e| AgentScribeError::Parse {
                file: source_path.display().to_string(),
                line: None,
                message: format!("cannot prepare query: {}", e),
            })?;

            let col_count = stmt.column_count();
            let mut rows = stmt.query([]).map_err(|e| AgentScribeError::Parse {
                file: source_path.display().to_string(),
                line: None,
                message: format!("cannot execute query: {}", e),
            })?;

            let mut session_ids: Vec<String> = Vec::new();
            loop {
                let row = rows.next().map_err(|e| AgentScribeError::Parse {
                    file: source_path.display().to_string(),
                    line: None,
                    message: format!("row read error: {}", e),
                })?;
                let row = match row {
                    Some(r) => r,
                    None => break,
                };

                let key: String = if col_count >= 2 {
                    row.get(0).unwrap_or_default()
                } else {
                    String::new()
                };

                if let Some(ref kre) = key_re {
                    if !kre.is_match(&key) {
                        continue;
                    }
                }

                if let Some(sid) = Self::extract_key_session_id(&re, &key) {
                    if !session_ids.contains(&sid) {
                        session_ids.push(sid);
                    }
                }
            }

            if session_ids.is_empty() {
                // No matching keys — return a single placeholder so the scraper
                // still marks the file as processed.
                return Ok(vec![SessionInfo {
                    session_id: Self::session_id_from_path(source_path, plugin),
                    start_offset: 0,
                    end_offset: file_size,
                    metadata: None,
                }]);
            }

            return Ok(session_ids
                .into_iter()
                .map(|sid| SessionInfo {
                    session_id: sid,
                    start_offset: 0,
                    end_offset: file_size,
                    metadata: None,
                })
                .collect());
        }

        // Default: one session per file, ID from filename.
        let session_id = Self::session_id_from_path(source_path, plugin);
        Ok(vec![SessionInfo {
            session_id,
            start_offset: 0,
            end_offset: file_size,
            metadata: None,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::FormatParser;
    use crate::plugin::{LogFormat, Parser, Plugin, PluginMeta, SessionDetection, SessionIdSource, Source};
    use rusqlite::Connection;
    use std::collections::HashMap;
    use tempfile::NamedTempFile;

    fn make_plugin(query: &str, content_path: Option<&str>) -> Plugin {
        Plugin {
            plugin: PluginMeta {
                name: "test-sqlite".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec!["/tmp/test.vscdb".to_string()],
                exclude: vec![],
                format: LogFormat::Sqlite,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: None,
            },
            parser: Parser {
                query: Some(query.to_string()),
                content_path: content_path.map(String::from),
                role: Some("role".to_string()),
                content: Some("text".to_string()),
                role_map: {
                    let mut m = HashMap::new();
                    m.insert("AI".to_string(), "assistant".to_string());
                    m
                },
                ..Default::default()
            },
            metadata: None,
        }
    }

    fn create_test_db(messages_json: &str) -> NamedTempFile {
        let tmp = NamedTempFile::new().unwrap();
        let conn = Connection::open(tmp.path()).unwrap();
        conn.execute_batch(
            "CREATE TABLE cursorDiskKV (key TEXT, value TEXT);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params!["history.session1", messages_json],
        )
        .unwrap();
        tmp
    }

    #[test]
    fn test_parse_flat_array() {
        let blob = r#"[{"role":"user","text":"hello"},{"role":"assistant","text":"hi"}]"#;
        let db = create_test_db(blob);
        let plugin = make_plugin("SELECT key, value FROM cursorDiskKV", None);

        let parser = SqliteParser;
        let events = parser.parse(db.path(), &plugin).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].role, Role::User);
        assert_eq!(events[0].content, "hello");
        assert_eq!(events[1].role, Role::Assistant);
    }

    #[test]
    fn test_parse_with_content_path() {
        let blob = r#"{"messages":[{"role":"user","text":"hi"},{"role":"AI","text":"hello"}]}"#;
        let db = create_test_db(blob);
        let plugin = make_plugin(
            "SELECT key, value FROM cursorDiskKV",
            Some("messages"),
        );

        let parser = SqliteParser;
        let events = parser.parse(db.path(), &plugin).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].role, Role::Assistant); // "AI" mapped to assistant
    }

    #[test]
    fn test_key_filter_skips_non_matching() {
        let tmp = NamedTempFile::new().unwrap();
        let conn = Connection::open(tmp.path()).unwrap();
        conn.execute_batch("CREATE TABLE cursorDiskKV (key TEXT, value TEXT);").unwrap();
        conn.execute(
            "INSERT INTO cursorDiskKV VALUES (?1, ?2)",
            rusqlite::params![
                "history.session1",
                r#"[{"role":"user","text":"keep"}]"#
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO cursorDiskKV VALUES (?1, ?2)",
            rusqlite::params![
                "settings.foo",
                r#"[{"role":"user","text":"discard"}]"#
            ],
        )
        .unwrap();

        let mut plugin = make_plugin("SELECT key, value FROM cursorDiskKV", None);
        plugin.parser.key_filter = Some(r"^history\.".to_string());

        let events = SqliteParser.parse(tmp.path(), &plugin).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].content, "keep");
    }

    #[test]
    fn test_detect_sessions_returns_one() {
        let db = create_test_db(r#"[]"#);
        let plugin = make_plugin("SELECT key, value FROM cursorDiskKV", None);
        let sessions = SqliteParser.detect_sessions(db.path(), &plugin).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, db.path().file_stem().unwrap().to_str().unwrap());
    }

    #[test]
    fn test_readonly_does_not_modify_db() {
        let db = create_test_db(r#"[]"#);
        let plugin = make_plugin("SELECT key, value FROM cursorDiskKV", None);

        // Get mtime before
        let meta_before = std::fs::metadata(db.path()).unwrap();
        let mtime_before = meta_before.modified().unwrap();

        SqliteParser.parse(db.path(), &plugin).unwrap();

        let meta_after = std::fs::metadata(db.path()).unwrap();
        let mtime_after = meta_after.modified().unwrap();

        assert_eq!(mtime_before, mtime_after, "parser must not modify source DB");
    }

    #[test]
    fn test_key_session_id_regex_extracts_session_id() {
        let tmp = NamedTempFile::new().unwrap();
        let conn = Connection::open(tmp.path()).unwrap();
        conn.execute_batch("CREATE TABLE cursorDiskKV (key TEXT, value TEXT);").unwrap();

        // Insert multiple sessions using bubbleId pattern (like Cursor/Windsurf)
        conn.execute(
            "INSERT INTO cursorDiskKV VALUES (?1, ?2)",
            rusqlite::params![
                "bubbleId:composer-abc:001",
                r#"[{"role":"user","text":"hello from abc"}]"#
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO cursorDiskKV VALUES (?1, ?2)",
            rusqlite::params![
                "bubbleId:composer-def:002",
                r#"[{"role":"user","text":"hello from def"}]"#
            ],
        )
        .unwrap();

        let mut plugin = make_plugin("SELECT key, value FROM cursorDiskKV", None);
        plugin.parser.key_filter = Some(r"^bubbleId:".to_string());
        plugin.parser.key_session_id_regex = Some(r"^bubbleId:([^:]+):".to_string());

        let parser = SqliteParser;
        let sessions = parser.detect_sessions(tmp.path(), &plugin).unwrap();

        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_id, "composer-abc");
        assert_eq!(sessions[1].session_id, "composer-def");
    }

    #[test]
    fn test_key_session_id_regex_filters_non_matching_keys() {
        let tmp = NamedTempFile::new().unwrap();
        let conn = Connection::open(tmp.path()).unwrap();
        conn.execute_batch("CREATE TABLE cursorDiskKV (key TEXT, value TEXT);").unwrap();

        // Mix of bubbleId keys (should match) and other keys (should be filtered)
        conn.execute(
            "INSERT INTO cursorDiskKV VALUES (?1, ?2)",
            rusqlite::params![
                "bubbleId:session-1:001",
                r#"[{"role":"user","text":"keep this"}]"#
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO cursorDiskKV VALUES (?1, ?2)",
            rusqlite::params![
                "composerData:session-1",
                r#"{"title":"Session Metadata"}"#
            ],
        )
        .unwrap();

        let mut plugin = make_plugin("SELECT key, value FROM cursorDiskKV", None);
        plugin.parser.key_filter = Some(r"^bubbleId:".to_string());
        plugin.parser.key_session_id_regex = Some(r"^bubbleId:([^:]+):".to_string());

        let parser = SqliteParser;
        let sessions = parser.detect_sessions(tmp.path(), &plugin).unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "session-1");
    }

    #[test]
    fn test_parse_cursor_disk_kv_pattern() {
        let tmp = NamedTempFile::new().unwrap();
        let conn = Connection::open(tmp.path()).unwrap();
        conn.execute_batch("CREATE TABLE cursorDiskKV (key TEXT, value TEXT);").unwrap();

        // Simulate Cursor/Windsurf bubbleId pattern
        conn.execute(
            "INSERT INTO cursorDiskKV VALUES (?1, ?2)",
            rusqlite::params![
                "bubbleId:cmp-123:bbl-1",
                r#"[{"type":"1","text":"user message"},{"type":"2","text":"assistant message"}]"#
            ],
        )
        .unwrap();

        let mut plugin = make_plugin("SELECT key, value FROM cursorDiskKV", None);
        plugin.parser.key_filter = Some(r"^bubbleId:".to_string());
        plugin.parser.key_session_id_regex = Some(r"^bubbleId:([^:]+):".to_string());
        plugin.parser.role = Some("type".to_string());
        plugin.parser.content = Some("text".to_string());
        // Add role mappings for integer type codes (like Cursor/Windsurf)
        plugin.parser.role_map.insert("1".to_string(), "user".to_string());
        plugin.parser.role_map.insert("2".to_string(), "assistant".to_string());

        let parser = SqliteParser;
        let events = parser.parse(tmp.path(), &plugin).unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].session_id, "cmp-123");
        assert_eq!(events[0].role, Role::User);
        assert_eq!(events[0].content, "user message");
        assert_eq!(events[1].role, Role::Assistant);
    }

    #[test]
    fn test_detect_sessions_with_no_matching_keys_returns_placeholder() {
        let tmp = NamedTempFile::new().unwrap();
        let conn = Connection::open(tmp.path()).unwrap();
        conn.execute_batch("CREATE TABLE cursorDiskKV (key TEXT, value TEXT);").unwrap();

        // Insert keys that don't match the filter
        conn.execute(
            "INSERT INTO cursorDiskKV VALUES (?1, ?2)",
            rusqlite::params![
                "otherKey:data",
                r#"[]"#
            ],
        )
        .unwrap();

        let mut plugin = make_plugin("SELECT key, value FROM cursorDiskKV", None);
        plugin.parser.key_filter = Some(r"^bubbleId:".to_string());
        plugin.parser.key_session_id_regex = Some(r"^bubbleId:([^:]+):".to_string());

        let parser = SqliteParser;
        let sessions = parser.detect_sessions(tmp.path(), &plugin).unwrap();

        // Should return one placeholder session so the file is marked as processed
        assert_eq!(sessions.len(), 1);
        // The session_id comes from the filename stem (tmpXXXX), which varies
        // Just verify it's not empty and not "unknown" (which would mean no file stem was found)
        assert!(!sessions[0].session_id.is_empty());
    }
}

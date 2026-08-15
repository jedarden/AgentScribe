//! Behavioral signals: per-session quantitative metrics.
//!
//! Extracts counts, rates, and patterns from events that a reflection tool
//! can use to spot behavioral trends across sessions.

use crate::event::{Event, Role};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Default config/memory file glob patterns used for detection.
/// Can be overridden via config.toml [behavioral_signals.config_patterns]
static DEFAULT_CONFIG_FILE_PATTERNS: &[&str] = &[
    "CLAUDE.md",
    "AGENTS.md",
    ".claude/",
    ".needle/",
    "memory/",
    "docs/notes/",
    "MEMORY.md",
];

/// A single config file write event with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigWriteEvent {
    /// Path to the config file that was written
    pub path: String,
    /// Timestamp when the write occurred
    pub timestamp: i64,
    /// Tool type that performed the write ("Write" or "Edit")
    pub tool_type: String,
}

/// Quantitative behavioral metrics for a session.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BehavioralSignals {
    /// Total tool calls in session
    pub tool_call_count: u32,
    /// Tool call counts broken down by tool name
    pub tool_call_counts_by_name: HashMap<String, u32>,
    /// Files read more than once in the same session (re-read pattern)
    pub re_read_files: Vec<String>,
    /// Number of re-read events (each duplicate read beyond the first counts as one)
    pub re_read_count: u32,
    /// Bash commands that returned non-zero exit code
    pub bash_failure_count: u32,
    /// Files written/edited more than once (possible revert pattern)
    pub multi_edit_files: Vec<String>,
    /// Approximate session duration in seconds (last_ts - first_ts)
    pub duration_secs: u64,
    /// Ratio: assistant turns / total turns
    pub assistant_turn_ratio: f32,
    /// Whether agent read any config/memory files (CLAUDE.md, AGENTS.md, .claude/**, .needle/**, memory/, docs/notes/)
    pub read_config_files: Vec<String>,
    /// Whether agent wrote/edited any config/memory files
    pub modified_config_files: Vec<String>,
    /// Number of times agent switched working directory (cwd changes in events)
    pub cwd_switch_count: u32,
    /// Individual config file write events with timestamps and tool types
    pub config_writes: Vec<ConfigWriteEvent>,
}

/// Compute behavioral signals from a session's events.
///
/// # Arguments
///
/// * `events` - Session events to analyze
/// * `config_patterns` - Optional config file patterns from config.toml.
///   If None, uses the default patterns.
pub fn compute_behavioral_signals(
    events: &[Event],
    config_patterns: Option<&[String]>,
) -> BehavioralSignals {
    let mut signals = BehavioralSignals::default();

    if events.is_empty() {
        return signals;
    }

    // Use provided patterns or defaults
    let patterns =
        config_patterns.map(|ps| ps.as_ref().iter().map(|s| s.as_str()).collect::<Vec<_>>());
    let default_patterns: Vec<&str> = DEFAULT_CONFIG_FILE_PATTERNS.to_vec();
    let effective_patterns = patterns.as_deref().unwrap_or(&default_patterns);

    // Track reads, writes, edits per file
    let mut file_reads: HashMap<&str, u32> = HashMap::new();
    let mut file_writes_edits: HashMap<&str, u32> = HashMap::new();
    let mut config_reads: Vec<String> = Vec::new();
    let mut config_modifies: Vec<String> = Vec::new();
    let mut config_read_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut config_modify_seen: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut config_writes: Vec<ConfigWriteEvent> = Vec::new();
    let mut total_turns: u32 = 0;
    let mut assistant_turns: u32 = 0;
    let mut last_cwd: Option<String> = None;

    for event in events {
        // Count turns by role (user, assistant, system)
        match event.role {
            Role::User | Role::Assistant | Role::System => {
                total_turns += 1;
                if event.role == Role::Assistant {
                    assistant_turns += 1;
                }
            }
            _ => {}
        }

        // Tool calls
        if event.role == Role::ToolCall {
            signals.tool_call_count += 1;
            if let Some(ref tool_name) = event.tool {
                *signals
                    .tool_call_counts_by_name
                    .entry(tool_name.clone())
                    .or_insert(0) += 1;
            }

            // Track file reads (Read tool)
            if event.tool.as_deref() == Some("Read") {
                if let Some(ref fp) = extract_file_path_from_params(event) {
                    *file_reads.entry(fp).or_insert(0) += 1;

                    if is_config_file(fp, effective_patterns)
                        && config_read_seen.insert(fp.to_string())
                    {
                        config_reads.push(fp.to_string());
                    }
                }
            }

            // Track file writes (Write tool)
            if event.tool.as_deref() == Some("Write") {
                if let Some(ref fp) = extract_file_path_from_params(event) {
                    *file_writes_edits.entry(fp).or_insert(0) += 1;

                    if is_config_file(fp, effective_patterns) {
                        if config_modify_seen.insert(fp.to_string()) {
                            config_modifies.push(fp.to_string());
                        }
                        // Track individual config write event
                        config_writes.push(ConfigWriteEvent {
                            path: fp.to_string(),
                            timestamp: event.ts.timestamp(),
                            tool_type: "Write".to_string(),
                        });
                    }
                }
            }

            // Track file edits (Edit tool)
            if event.tool.as_deref() == Some("Edit") {
                if let Some(ref fp) = extract_file_path_from_params(event) {
                    *file_writes_edits.entry(fp).or_insert(0) += 1;

                    if is_config_file(fp, effective_patterns) {
                        if config_modify_seen.insert(fp.to_string()) {
                            config_modifies.push(fp.to_string());
                        }
                        // Track individual config write event
                        config_writes.push(ConfigWriteEvent {
                            path: fp.to_string(),
                            timestamp: event.ts.timestamp(),
                            tool_type: "Edit".to_string(),
                        });
                    }
                }
            }

            // Track CWD changes (Bash tool with cd commands)
            if event.tool.as_deref() == Some("Bash") {
                if let Some(cwd) = detect_cwd_change(event) {
                    if last_cwd.is_none_or(|prev| prev != cwd.as_str()) {
                        signals.cwd_switch_count += 1;
                    }
                    last_cwd = Some(cwd);
                }
            }
        }

        // Tool results — check for bash failures
        if event.role == Role::ToolResult && event.tool.as_deref() == Some("Bash") {
            if let Some(ref params) = event.tool_params {
                if let Some(exit_code) = params.get("exit_code").and_then(|v| v.as_i64()) {
                    if exit_code != 0 {
                        signals.bash_failure_count += 1;
                    }
                }
            }
        }
    }

    // Re-read files: files read more than once
    for (file, count) in &file_reads {
        if *count > 1 {
            signals.re_read_count += count - 1;
            signals.re_read_files.push(file.to_string());
        }
    }
    signals.re_read_files.sort();

    // Multi-edit files: files written/edited more than once
    for (file, count) in &file_writes_edits {
        if *count > 1 {
            signals.multi_edit_files.push(file.to_string());
        }
    }
    signals.multi_edit_files.sort();

    // Config files
    signals.read_config_files = config_reads;
    signals.modified_config_files = config_modifies;
    signals.config_writes = config_writes;

    // Duration
    let first_ts = events.first().map(|e| e.ts).unwrap();
    let last_ts = events.last().map(|e| e.ts).unwrap();
    signals.duration_secs = (last_ts - first_ts).num_seconds().unsigned_abs();

    // Assistant turn ratio
    if total_turns > 0 {
        signals.assistant_turn_ratio = assistant_turns as f32 / total_turns as f32;
    }

    signals
}

/// Extract a file_path from a tool_call event's tool_params.
fn extract_file_path_from_params(event: &Event) -> Option<&str> {
    event
        .tool_params
        .as_ref()
        .and_then(|p| p.get("file_path"))
        .and_then(|v| v.as_str())
}

/// Check if a file path matches any config/memory file pattern.
fn is_config_file(path: &str, patterns: &[&str]) -> bool {
    let path_normalized = path.replace('\\', "/");
    for pattern in patterns {
        // Check as exact suffix match or contains match
        let segments: Vec<&str> = path_normalized.split('/').collect();
        for segment in &segments {
            if *segment == *pattern {
                return true;
            }
        }
        // Also check if the path contains the pattern as a directory component
        if path_normalized.contains(pattern) && pattern.starts_with('.') {
            return true;
        }
        // Check for memory/*.md pattern
        if (*pattern == "memory/" || *pattern == "docs/notes/") && path_normalized.contains(pattern)
        {
            return true;
        }
        // Check for MEMORY.md at root level or in a path
        if *pattern == "MEMORY.md" {
            let filename = segments.last().copied().unwrap_or("");
            if filename == "MEMORY.md" {
                return true;
            }
        }
    }
    false
}

/// Write behavioral signals to a sidecar JSON file.
///
/// The sidecar is stored as `sessions/<agent>/<session_id>.behavioral.json`
/// alongside the session JSONL.
pub fn write_behavioral_sidecar(
    data_dir: &std::path::Path,
    session_id: &str,
    signals: &BehavioralSignals,
) -> std::io::Result<()> {
    let session_dir = data_dir.join("sessions");
    let parts: Vec<&str> = session_id.splitn(2, '/').collect();
    if parts.len() != 2 {
        return Ok(());
    }

    let plugin_dir = session_dir.join(parts[0]);
    std::fs::create_dir_all(&plugin_dir)?;

    let sidecar_path = plugin_dir.join(format!("{}.behavioral.json", parts[1]));
    let json = serde_json::to_string_pretty(signals)?;
    std::fs::write(&sidecar_path, json)
}

/// Load behavioral signals from a session's sidecar file.
///
/// Returns None if the sidecar doesn't exist or can't be parsed.
pub fn load_behavioral_signals(
    data_dir: &std::path::Path,
    session_id: &str,
) -> Option<BehavioralSignals> {
    let session_dir = data_dir.join("sessions");
    let parts: Vec<&str> = session_id.splitn(2, '/').collect();
    if parts.len() != 2 {
        return None;
    }

    let sidecar_path = session_dir
        .join(parts[0])
        .join(format!("{}.behavioral.json", parts[1]));

    let content = std::fs::read_to_string(&sidecar_path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Read and parse behavioral_signals.json sidecar for a session.
///
/// # Arguments
///
/// * `session_id` - Session identifier string (format: "agent/session-id")
///
/// # Returns
///
/// * `Ok(BehavioralSignals)` - Successfully parsed signals
/// * `Ok(BehavioralSignals::default())` - Sidecar file doesn't exist yet
/// * `Err` - File exists but cannot be read or parsed
///
/// # Examples
///
/// ```ignore
/// use agentscribe::enrichment::behavioral_signals::read_behavioral_signals;
///
/// let signals = read_behavioral_signals("claude-code/abc123").unwrap();
/// println!("Tool calls: {}", signals.tool_call_count);
/// ```
pub fn read_behavioral_signals(session_id: &str) -> Result<BehavioralSignals> {
    // Get the data directory from environment or default
    let data_dir = std::env::var("AGENTSCRIBE_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = directories::BaseDirs::new()
                .expect("Failed to determine home directory")
                .home_dir()
                .to_path_buf();
            home.join(".agentscribe")
        });

    let session_dir = data_dir.join("sessions");
    let parts: Vec<&str> = session_id.splitn(2, '/').collect();
    if parts.len() != 2 {
        anyhow::bail!(
            "Invalid session_id format: expected 'agent/session-id', got: {}",
            session_id
        );
    }

    let sidecar_path = session_dir
        .join(parts[0])
        .join(format!("{}.behavioral.json", parts[1]));

    // Return empty default if file doesn't exist
    if !sidecar_path.exists() {
        return Ok(BehavioralSignals::default());
    }

    // Read and parse the file
    let content = std::fs::read_to_string(&sidecar_path).map_err(|e| {
        anyhow::anyhow!(
            "Failed to read behavioral signals file {:?}: {}",
            sidecar_path,
            e
        )
    })?;

    serde_json::from_str(&content).map_err(|e| {
        anyhow::anyhow!(
            "Failed to parse behavioral signals JSON from {:?}: {}",
            sidecar_path,
            e
        )
    })
}

/// Detect a working directory change from a Bash tool_call event.
///
/// Looks for `cd <dir>` commands in the Bash tool's params.
fn detect_cwd_change(event: &Event) -> Option<String> {
    event
        .tool_params
        .as_ref()
        .and_then(|p| p.get("command"))
        .and_then(|v| v.as_str())
        .and_then(|cmd| {
            // Simple cd detection: command starts with "cd " or contains " && cd " etc.
            let trimmed = cmd.trim();
            if let Some(dir) = trimmed.strip_prefix("cd ") {
                let dir = dir.trim();
                // Skip cd without argument or cd -
                if !dir.is_empty() && dir != "-" {
                    return Some(dir.to_string());
                }
            }
            None
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use chrono::Utc;
    use serde_json::json;

    fn make_event(role: Role, tool: Option<&str>, content: &str) -> Event {
        let mut event = Event::new(
            Utc::now(),
            "test/1".to_string(),
            "claude".to_string(),
            role,
            content.to_string(),
        );
        event.tool = tool.map(|s| s.to_string());
        event
    }

    fn make_tool_call(tool: &str, params: serde_json::Value) -> Event {
        let mut event = make_event(Role::ToolCall, Some(tool), "");
        event.tool_params = Some(params);
        event
    }

    fn make_tool_result(tool: &str, params: serde_json::Value) -> Event {
        let mut event = make_event(Role::ToolResult, Some(tool), "");
        event.tool_params = Some(params);
        event
    }

    #[test]
    fn test_empty_events() {
        let signals = compute_behavioral_signals(&[], None);
        assert_eq!(signals.tool_call_count, 0);
        assert_eq!(signals.bash_failure_count, 0);
        assert_eq!(signals.duration_secs, 0);
        assert_eq!(signals.assistant_turn_ratio, 0.0);
        assert_eq!(signals.config_writes.len(), 0);
    }

    #[test]
    fn test_tool_call_counts() {
        let events = vec![
            make_tool_call("Bash", json!({"command": "ls"})),
            make_tool_call("Read", json!({"file_path": "src/main.rs"})),
            make_tool_call("Bash", json!({"command": "cargo test"})),
        ];
        let signals = compute_behavioral_signals(&events, None);
        assert_eq!(signals.tool_call_count, 3);
        assert_eq!(*signals.tool_call_counts_by_name.get("Bash").unwrap(), 2);
        assert_eq!(*signals.tool_call_counts_by_name.get("Read").unwrap(), 1);
    }

    #[test]
    fn test_re_read_detection() {
        let events = vec![
            make_tool_call("Read", json!({"file_path": "/project/src/main.rs"})),
            make_tool_call("Read", json!({"file_path": "/project/src/main.rs"})),
            make_tool_call("Read", json!({"file_path": "/project/src/main.rs"})),
            make_tool_call("Read", json!({"file_path": "/project/src/lib.rs"})),
        ];
        let signals = compute_behavioral_signals(&events, None);
        assert_eq!(signals.re_read_count, 2); // 3 reads - 1 = 2 re-reads
        assert_eq!(signals.re_read_files, vec!["/project/src/main.rs"]);
    }

    #[test]
    fn test_bash_failure_count() {
        let events = vec![
            make_tool_call("Bash", json!({"command": "cargo test"})),
            make_tool_result("Bash", json!({"exit_code": 0})),
            make_tool_call("Bash", json!({"command": "cargo build"})),
            make_tool_result("Bash", json!({"exit_code": 1})),
            make_tool_call("Bash", json!({"command": "cargo clippy"})),
            make_tool_result("Bash", json!({"exit_code": 101})),
        ];
        let signals = compute_behavioral_signals(&events, None);
        assert_eq!(signals.bash_failure_count, 2);
    }

    #[test]
    fn test_multi_edit_files() {
        let events = vec![
            make_tool_call("Edit", json!({"file_path": "/project/src/main.rs"})),
            make_tool_call("Edit", json!({"file_path": "/project/src/main.rs"})),
            make_tool_call("Write", json!({"file_path": "/project/src/main.rs"})),
            make_tool_call("Edit", json!({"file_path": "/project/src/lib.rs"})),
        ];
        let signals = compute_behavioral_signals(&events, None);
        assert!(signals
            .multi_edit_files
            .contains(&"/project/src/main.rs".to_string()));
        assert!(!signals
            .multi_edit_files
            .contains(&"/project/src/lib.rs".to_string()));
    }

    #[test]
    fn test_config_file_read_detection() {
        let events = vec![
            make_tool_call("Read", json!({"file_path": "/project/CLAUDE.md"})),
            make_tool_call("Read", json!({"file_path": "/project/src/main.rs"})),
            make_tool_call(
                "Read",
                json!({"file_path": "/project/.claude/settings.json"}),
            ),
            make_tool_call("Read", json!({"file_path": "/project/memory/team.md"})),
        ];
        let signals = compute_behavioral_signals(&events, None);
        assert!(signals
            .read_config_files
            .contains(&"/project/CLAUDE.md".to_string()));
        assert!(signals
            .read_config_files
            .contains(&"/project/.claude/settings.json".to_string()));
        assert!(signals
            .read_config_files
            .contains(&"/project/memory/team.md".to_string()));
        // src/main.rs is NOT a config file
        assert!(!signals
            .read_config_files
            .iter()
            .any(|f| f.contains("src/main.rs")));
    }

    #[test]
    fn test_config_file_write_detection() {
        let events = vec![
            make_tool_call("Write", json!({"file_path": "/project/AGENTS.md"})),
            make_tool_call("Edit", json!({"file_path": "/project/CLAUDE.md"})),
        ];
        let signals = compute_behavioral_signals(&events, None);
        assert!(signals
            .modified_config_files
            .contains(&"/project/AGENTS.md".to_string()));
        assert!(signals
            .modified_config_files
            .contains(&"/project/CLAUDE.md".to_string()));
    }

    #[test]
    fn test_assistant_turn_ratio() {
        let events = vec![
            // 1 user turn
            make_event(Role::User, None, "fix the bug"),
            // 3 assistant turns
            make_event(Role::Assistant, None, "I'll fix it"),
            make_event(Role::Assistant, None, "Here's the fix"),
            make_event(Role::Assistant, None, "Done"),
        ];

        let signals = compute_behavioral_signals(&events, None);
        // 3 assistant / 4 total = 0.75
        assert!((signals.assistant_turn_ratio - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_cwd_switch_count() {
        let events = vec![
            make_tool_call("Bash", json!({"command": "cd /home/user/project"})),
            make_tool_call("Bash", json!({"command": "ls"})),
            make_tool_call("Bash", json!({"command": "cd /tmp"})),
            make_tool_call("Bash", json!({"command": "ls"})),
            make_tool_call("Bash", json!({"command": "cd /home/user/project"})), // back to original
        ];
        let signals = compute_behavioral_signals(&events, None);
        // First cd: no previous cwd → counts as switch
        // cd /tmp: switch
        // cd /home/user/project: switch (different from /tmp)
        assert_eq!(signals.cwd_switch_count, 3);
    }

    #[test]
    fn test_is_config_file_patterns() {
        let patterns: Vec<&str> = DEFAULT_CONFIG_FILE_PATTERNS.to_vec();
        assert!(is_config_file("/project/CLAUDE.md", &patterns));
        assert!(is_config_file("/project/AGENTS.md", &patterns));
        assert!(is_config_file("/project/.claude/settings.json", &patterns));
        assert!(is_config_file("/project/.needle/config.toml", &patterns));
        assert!(is_config_file("/project/memory/team.md", &patterns));
        assert!(is_config_file(
            "/project/docs/notes/architecture.md",
            &patterns
        ));
        assert!(is_config_file("/project/MEMORY.md", &patterns));

        // NOT config files
        assert!(!is_config_file("/project/src/main.rs", &patterns));
        assert!(!is_config_file("/project/README.md", &patterns));
        assert!(!is_config_file("/project/Cargo.toml", &patterns));
    }

    #[test]
    fn test_serialization_roundtrip() {
        let events = vec![
            make_tool_call("Bash", json!({"command": "cargo test"})),
            make_tool_result("Bash", json!({"exit_code": 1})),
        ];
        let signals = compute_behavioral_signals(&events, None);

        let json = serde_json::to_string(&signals).unwrap();
        let deser: BehavioralSignals = serde_json::from_str(&json).unwrap();

        assert_eq!(deser.tool_call_count, signals.tool_call_count);
        assert_eq!(deser.bash_failure_count, signals.bash_failure_count);
    }

    #[test]
    fn test_config_writes_tracking() {
        let events = vec![
            make_tool_call("Write", json!({"file_path": "/project/CLAUDE.md"})),
            make_tool_call("Edit", json!({"file_path": "/project/CLAUDE.md"})),
            make_tool_call("Write", json!({"file_path": "/project/AGENTS.md"})),
            make_tool_call("Edit", json!({"file_path": "/project/src/main.rs"})), // NOT a config file
        ];

        let signals = compute_behavioral_signals(&events, None);

        // Should have 3 config writes (2 to CLAUDE.md, 1 to AGENTS.md)
        assert_eq!(signals.config_writes.len(), 3);

        // Check first write to CLAUDE.md
        assert_eq!(signals.config_writes[0].path, "/project/CLAUDE.md");
        assert_eq!(signals.config_writes[0].tool_type, "Write");

        // Check second write to CLAUDE.md
        assert_eq!(signals.config_writes[1].path, "/project/CLAUDE.md");
        assert_eq!(signals.config_writes[1].tool_type, "Edit");

        // Check write to AGENTS.md
        assert_eq!(signals.config_writes[2].path, "/project/AGENTS.md");
        assert_eq!(signals.config_writes[2].tool_type, "Write");
    }

    #[test]
    fn test_custom_config_patterns() {
        let events = vec![
            make_tool_call("Write", json!({"file_path": "/project/CONFIG.yaml"})),
            make_tool_call("Edit", json!({"file_path": "/project/.env"})),
        ];

        let custom_patterns = vec!["CONFIG.yaml".to_string(), ".env".to_string()];
        let signals = compute_behavioral_signals(&events, Some(&custom_patterns));

        // Both should be detected as config writes
        assert_eq!(signals.config_writes.len(), 2);
        assert_eq!(signals.config_writes[0].path, "/project/CONFIG.yaml");
        assert_eq!(signals.config_writes[1].path, "/project/.env");
    }
}

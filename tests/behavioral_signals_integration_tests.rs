//! Integration tests for behavioral signals through the full enrichment pipeline.
//!
//! Tests:
//!   - Realistic session with multiple tool calls, re-reads, bash failures, config file reads
//!   - Sidecar roundtrip: write_behavioral_sidecar → load_behavioral_signals → assert equality
//!   - Integration with enrich_session() to verify behavioral_signals is populated

use std::fs;
use std::path::Path;

use agentscribe::enrichment::behavioral_signals::{
    compute_behavioral_signals, load_behavioral_signals, write_behavioral_sidecar,
    BehavioralSignals,
};
use agentscribe::enrichment::{enrich_session, OutcomeConfig};
use agentscribe::event::{Event, Role, SessionManifest};
use agentscribe::scraper::Scraper;
use chrono::{Duration, Utc};
use serde_json::json;

// ─── Test helpers ─────────────────────────────────────────────────────────────

/// Create a temp data directory with the required sub-structure.
fn make_data_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    fs::create_dir_all(dir.path().join("plugins")).unwrap();
    fs::create_dir_all(dir.path().join("sessions")).unwrap();
    fs::create_dir_all(dir.path().join("state")).unwrap();
    dir
}

/// Create a minimal mock scraper (enough for enrichment tests).
fn make_mock_scraper(data_dir: &Path) -> Scraper {
    Scraper::new_with_lock_timeout(data_dir.to_path_buf(), 30).expect("failed to create scraper")
}

fn make_event(ts: chrono::DateTime<Utc>, role: Role, tool: Option<&str>, content: &str) -> Event {
    let mut event = Event::new(
        ts,
        "test/123".to_string(),
        "claude-code".to_string(),
        role,
        content.to_string(),
    );
    event.tool = tool.map(|s| s.to_string());
    event
}

fn make_tool_call(ts: chrono::DateTime<Utc>, tool: &str, params: serde_json::Value) -> Event {
    let mut event = make_event(ts, Role::ToolCall, Some(tool), "");
    event.tool_params = Some(params);
    event
}

fn make_tool_result(ts: chrono::DateTime<Utc>, tool: &str, params: serde_json::Value) -> Event {
    let mut event = make_event(ts, Role::ToolResult, Some(tool), "");
    event.tool_params = Some(params);
    event
}

fn make_user_event(ts: chrono::DateTime<Utc>, content: &str) -> Event {
    make_event(ts, Role::User, None, content)
}

fn make_assistant_event(ts: chrono::DateTime<Utc>, content: &str) -> Event {
    make_event(ts, Role::Assistant, None, content)
}

// ─── Integration Tests ───────────────────────────────────────────────────────

#[test]
fn test_realistic_session_behavioral_signals() {
    let base_time = Utc::now();

    // Create a realistic session with:
    // - Multiple tool calls (Read, Edit, Bash, Write)
    // - Re-read patterns (same file read multiple times)
    // - Bash failures (non-zero exit codes)
    // - Config file reads (CLAUDE.md, memory files)
    // - Config file modifications
    // - CWD switches
    // - Multiple assistant turns

    let events = vec![
        // Turn 1: User asks for help
        make_user_event(base_time, "Fix the authentication bug in src/auth.rs"),
        // Turn 2: Assistant starts investigating
        make_assistant_event(
            base_time + Duration::seconds(1),
            "I'll help you fix the auth bug",
        ),
        // Turn 3: Read the config file first
        make_tool_call(
            base_time + Duration::seconds(2),
            "Read",
            json!({"file_path": "/project/CLAUDE.md"}),
        ),
        // Turn 4: Read memory docs
        make_tool_call(
            base_time + Duration::seconds(3),
            "Read",
            json!({"file_path": "/project/memory/team.md"}),
        ),
        // Turn 5: Read the auth file
        make_tool_call(
            base_time + Duration::seconds(4),
            "Read",
            json!({"file_path": "/project/src/auth.rs"}),
        ),
        // Turn 6: Re-read auth file (detecting re-read pattern)
        make_tool_call(
            base_time + Duration::seconds(5),
            "Read",
            json!({"file_path": "/project/src/auth.rs"}),
        ),
        // Turn 7: Re-read auth file again (strong re-read signal)
        make_tool_call(
            base_time + Duration::seconds(6),
            "Read",
            json!({"file_path": "/project/src/auth.rs"}),
        ),
        // Turn 8: Try to run tests - fails
        make_tool_call(
            base_time + Duration::seconds(7),
            "Bash",
            json!({"command": "cargo test"}),
        ),
        make_tool_result(
            base_time + Duration::seconds(8),
            "Bash",
            json!({"exit_code": 101}),
        ),
        // Turn 9: Edit the auth file
        make_tool_call(
            base_time + Duration::seconds(9),
            "Edit",
            json!({"file_path": "/project/src/auth.rs", "old_text": "old", "new_text": "new"}),
        ),
        // Turn 10: Run tests again - succeeds
        make_tool_call(
            base_time + Duration::seconds(10),
            "Bash",
            json!({"command": "cargo test"}),
        ),
        make_tool_result(
            base_time + Duration::seconds(11),
            "Bash",
            json!({"exit_code": 0}),
        ),
        // Turn 11: Edit auth file again (multi-edit pattern)
        make_tool_call(
            base_time + Duration::seconds(12),
            "Edit",
            json!({"file_path": "/project/src/auth.rs", "old_text": "fix", "new_text": "fix_v2"}),
        ),
        // Turn 12: CWD switch
        make_tool_call(
            base_time + Duration::seconds(13),
            "Bash",
            json!({"command": "cd /tmp && ls"}),
        ),
        // Turn 13: Another CWD switch
        make_tool_call(
            base_time + Duration::seconds(14),
            "Bash",
            json!({"command": "cd /home/user/project"}),
        ),
        // Turn 14: Modify config file
        make_tool_call(
            base_time + Duration::seconds(15),
            "Write",
            json!({"file_path": "/project/AGENTS.md", "content": "# Updated rules"}),
        ),
        // Turn 15: Assistant concludes
        make_assistant_event(base_time + Duration::seconds(16), "The auth bug is fixed"),
        // Turn 16: User confirms
        make_user_event(base_time + Duration::seconds(17), "Thanks, that works!"),
    ];

    let signals = compute_behavioral_signals(&events, None);

    // Verify tool call counts
    assert_eq!(signals.tool_call_count, 12); // All tool calls
    assert_eq!(*signals.tool_call_counts_by_name.get("Read").unwrap(), 5); // Config + memory + auth (x3)
    assert_eq!(*signals.tool_call_counts_by_name.get("Bash").unwrap(), 4); // test (x2) + cd (x2)
    assert_eq!(*signals.tool_call_counts_by_name.get("Edit").unwrap(), 2); // auth file edited twice
    assert_eq!(*signals.tool_call_counts_by_name.get("Write").unwrap(), 1); // AGENTS.md

    // Verify re-read detection
    assert_eq!(signals.re_read_count, 2); // 3 reads - 1 = 2 re-reads
    assert_eq!(signals.re_read_files, vec!["/project/src/auth.rs"]);

    // Verify bash failure counting
    assert_eq!(signals.bash_failure_count, 1); // Only the first cargo test failed

    // Verify multi-edit files
    assert!(signals
        .multi_edit_files
        .contains(&"/project/src/auth.rs".to_string()));
    assert_eq!(signals.multi_edit_files.len(), 1); // Only auth.rs edited multiple times

    // Verify config file reads
    assert!(signals
        .read_config_files
        .contains(&"/project/CLAUDE.md".to_string()));
    assert!(signals
        .read_config_files
        .contains(&"/project/memory/team.md".to_string()));
    assert_eq!(signals.read_config_files.len(), 2); // CLAUDE.md + memory/team.md

    // Verify config file modifications
    assert!(signals
        .modified_config_files
        .contains(&"/project/AGENTS.md".to_string()));
    assert_eq!(signals.modified_config_files.len(), 1);

    // Verify CWD switches
    assert_eq!(signals.cwd_switch_count, 2); // cd /tmp + cd /home/user/project

    // Verify duration (should be ~17 seconds)
    assert!(signals.duration_secs >= 16 && signals.duration_secs <= 18);

    // Verify assistant turn ratio
    // Total turns: user (2) + assistant (2) = 4
    // Assistant turns: 2
    // Ratio: 2/4 = 0.5
    assert!((signals.assistant_turn_ratio - 0.5).abs() < 0.01);
}

#[test]
fn test_sidecar_roundtrip() {
    let data_dir = make_data_dir();
    let session_id = "claude-code/test-session-123";

    // Create behavioral signals
    let original_signals = BehavioralSignals {
        tool_call_count: 42,
        tool_call_counts_by_name: vec![("Read".to_string(), 10), ("Bash".to_string(), 5)]
            .into_iter()
            .collect(),
        re_read_files: vec!["/project/src/auth.rs".to_string()],
        re_read_count: 3,
        bash_failure_count: 2,
        multi_edit_files: vec!["/project/src/lib.rs".to_string()],
        duration_secs: 3600,
        assistant_turn_ratio: 0.65,
        read_config_files: vec!["/project/CLAUDE.md".to_string()],
        modified_config_files: vec!["/project/AGENTS.md".to_string()],
        cwd_switch_count: 5,
        ..Default::default()
    };

    // Write sidecar
    write_behavioral_sidecar(data_dir.path(), session_id, &original_signals)
        .expect("failed to write sidecar");

    // Load sidecar
    let loaded_signals =
        load_behavioral_signals(data_dir.path(), session_id).expect("failed to load sidecar");

    // Verify equality
    assert_eq!(
        loaded_signals.tool_call_count,
        original_signals.tool_call_count
    );
    assert_eq!(
        loaded_signals.tool_call_counts_by_name,
        original_signals.tool_call_counts_by_name
    );
    assert_eq!(loaded_signals.re_read_files, original_signals.re_read_files);
    assert_eq!(loaded_signals.re_read_count, original_signals.re_read_count);
    assert_eq!(
        loaded_signals.bash_failure_count,
        original_signals.bash_failure_count
    );
    assert_eq!(
        loaded_signals.multi_edit_files,
        original_signals.multi_edit_files
    );
    assert_eq!(loaded_signals.duration_secs, original_signals.duration_secs);
    assert!(
        (loaded_signals.assistant_turn_ratio - original_signals.assistant_turn_ratio).abs() < 0.001
    );
    assert_eq!(
        loaded_signals.read_config_files,
        original_signals.read_config_files
    );
    assert_eq!(
        loaded_signals.modified_config_files,
        original_signals.modified_config_files
    );
    assert_eq!(
        loaded_signals.cwd_switch_count,
        original_signals.cwd_switch_count
    );

    // Verify sidecar file exists and is valid JSON
    let sidecar_path = data_dir
        .path()
        .join("sessions/claude-code")
        .join("test-session-123.behavioral.json");
    assert!(sidecar_path.exists());

    let sidecar_content = fs::read_to_string(&sidecar_path).expect("failed to read sidecar");
    let parsed: BehavioralSignals =
        serde_json::from_str(&sidecar_content).expect("sidecar is not valid JSON");

    assert_eq!(parsed.tool_call_count, original_signals.tool_call_count);
}

#[test]
fn test_sidecar_load_missing_file() {
    let data_dir = make_data_dir();
    let session_id = "claude-code/nonexistent-session";

    // Try to load a sidecar that doesn't exist
    let result = load_behavioral_signals(data_dir.path(), session_id);

    assert!(
        result.is_none(),
        "loading non-existent sidecar should return None"
    );
}

#[test]
fn test_enrich_session_populates_behavioral_signals() {
    let data_dir = make_data_dir();
    let scraper = make_mock_scraper(data_dir.path());

    let base_time = Utc::now();
    let events = vec![
        make_user_event(base_time, "Help me fix a bug"),
        make_assistant_event(base_time + Duration::seconds(1), "I'll help"),
        make_tool_call(
            base_time + Duration::seconds(2),
            "Read",
            json!({"file_path": "/project/src/main.rs"}),
        ),
        make_tool_call(
            base_time + Duration::seconds(3),
            "Bash",
            json!({"command": "cargo test"}),
        ),
        make_tool_result(
            base_time + Duration::seconds(4),
            "Bash",
            json!({"exit_code": 1}),
        ),
        make_assistant_event(base_time + Duration::seconds(5), "Fixed it"),
    ];

    let mut manifest = SessionManifest::new("test/123".to_string(), "claude-code".to_string());
    manifest.started = base_time;
    manifest.ended = Some(base_time + Duration::seconds(5));
    manifest.project = Some("/project".to_string());

    let outcome_config = OutcomeConfig::default();

    let result = enrich_session(
        &events,
        &manifest,
        &outcome_config,
        data_dir.path(),
        &scraper,
    );

    // Verify behavioral_signals is populated
    assert!(
        result.behavioral_signals.is_some(),
        "behavioral_signals should be populated after enrichment"
    );

    let signals = result.behavioral_signals.as_ref().unwrap();

    // Verify basic signal computation
    assert_eq!(signals.tool_call_count, 2); // Read + Bash
    assert_eq!(signals.bash_failure_count, 1); // One failed test
    assert!(signals.duration_secs >= 4 && signals.duration_secs <= 6);
}

#[test]
fn test_behavioral_signals_all_fields_populated() {
    let base_time = Utc::now();

    // Create a session that exercises ALL behavioral signal fields
    let events = vec![
        // User and assistant turns for ratio calculation
        make_user_event(base_time, "Fix everything"),
        make_assistant_event(base_time + Duration::seconds(1), "Working on it"),
        make_assistant_event(base_time + Duration::seconds(2), "Still working"),
        // Tool calls for counting
        make_tool_call(
            base_time + Duration::seconds(3),
            "Read",
            json!({"file_path": "/project/src/a.rs"}),
        ),
        make_tool_call(
            base_time + Duration::seconds(4),
            "Edit",
            json!({"file_path": "/project/src/a.rs"}),
        ),
        make_tool_call(
            base_time + Duration::seconds(5),
            "Write",
            json!({"file_path": "/project/src/b.rs"}),
        ),
        // Re-read pattern
        make_tool_call(
            base_time + Duration::seconds(6),
            "Read",
            json!({"file_path": "/project/src/a.rs"}),
        ),
        // Config file read
        make_tool_call(
            base_time + Duration::seconds(7),
            "Read",
            json!({"file_path": "/project/.claude/settings.json"}),
        ),
        // Config file write
        make_tool_call(
            base_time + Duration::seconds(8),
            "Write",
            json!({"file_path": "/project/CLAUDE.md"}),
        ),
        // CWD switch
        make_tool_call(
            base_time + Duration::seconds(9),
            "Bash",
            json!({"command": "cd /tmp"}),
        ),
        // Bash failure
        make_tool_call(
            base_time + Duration::seconds(10),
            "Bash",
            json!({"command": "false"}),
        ),
        make_tool_result(
            base_time + Duration::seconds(11),
            "Bash",
            json!({"exit_code": 1}),
        ),
        // Multi-edit pattern (edit same file twice)
        make_tool_call(
            base_time + Duration::seconds(12),
            "Edit",
            json!({"file_path": "/project/src/b.rs"}),
        ),
    ];

    let signals = compute_behavioral_signals(&events, None);

    // Verify ALL fields are populated
    assert_ne!(
        signals.tool_call_count, 0,
        "tool_call_count should be populated"
    );
    assert!(
        !signals.tool_call_counts_by_name.is_empty(),
        "tool_call_counts_by_name should be populated"
    );
    assert_ne!(
        signals.duration_secs, 0,
        "duration_secs should be populated"
    );
    assert_ne!(
        signals.assistant_turn_ratio, 0.0,
        "assistant_turn_ratio should be populated"
    );

    // Fields that may be empty but should be initialized vectors
    assert!(
        signals.re_read_files.is_sorted(),
        "re_read_files should be sorted"
    );
    assert!(
        signals.multi_edit_files.is_sorted(),
        "multi_edit_files should be sorted"
    );
    assert!(
        signals.read_config_files.is_sorted(),
        "read_config_files should be sorted"
    );
    assert!(
        signals.modified_config_files.is_sorted(),
        "modified_config_files should be sorted"
    );

    // Verify specific expected values
    assert_eq!(signals.tool_call_count, 9); // All tool calls
    assert_eq!(signals.re_read_count, 1); // a.rs read twice
    assert!(signals
        .re_read_files
        .contains(&"/project/src/a.rs".to_string()));
    assert_eq!(signals.bash_failure_count, 1); // false command failed
    assert!(signals
        .multi_edit_files
        .contains(&"/project/src/b.rs".to_string()));
    assert!(signals
        .read_config_files
        .contains(&"/project/.claude/settings.json".to_string()));
    assert!(signals
        .modified_config_files
        .contains(&"/project/CLAUDE.md".to_string()));
    assert_eq!(signals.cwd_switch_count, 1); // cd /tmp

    // Verify assistant turn ratio: 2 assistant / 3 total turns = ~0.67
    assert!((signals.assistant_turn_ratio - 0.67).abs() < 0.05);
}

#[test]
fn test_empty_session_behavioral_signals() {
    let signals = compute_behavioral_signals(&[], None);

    // All fields should be at sensible defaults for empty session
    assert_eq!(signals.tool_call_count, 0);
    assert!(signals.tool_call_counts_by_name.is_empty());
    assert!(signals.re_read_files.is_empty());
    assert_eq!(signals.re_read_count, 0);
    assert_eq!(signals.bash_failure_count, 0);
    assert!(signals.multi_edit_files.is_empty());
    assert_eq!(signals.duration_secs, 0);
    assert_eq!(signals.assistant_turn_ratio, 0.0);
    assert!(signals.read_config_files.is_empty());
    assert!(signals.modified_config_files.is_empty());
    assert_eq!(signals.cwd_switch_count, 0);
}

#[test]
fn test_config_file_detection_various_patterns() {
    let base_time = Utc::now();

    // Test various config file patterns
    let events = vec![
        make_tool_call(
            base_time,
            "Read",
            json!({"file_path": "/project/CLAUDE.md"}),
        ),
        make_tool_call(
            base_time + Duration::seconds(1),
            "Read",
            json!({"file_path": "/project/AGENTS.md"}),
        ),
        make_tool_call(
            base_time + Duration::seconds(2),
            "Read",
            json!({"file_path": "/project/.claude/settings.json"}),
        ),
        make_tool_call(
            base_time + Duration::seconds(3),
            "Read",
            json!({"file_path": "/project/.needle/config.toml"}),
        ),
        make_tool_call(
            base_time + Duration::seconds(4),
            "Read",
            json!({"file_path": "/project/memory/team.md"}),
        ),
        make_tool_call(
            base_time + Duration::seconds(5),
            "Read",
            json!({"file_path": "/project/docs/notes/arch.md"}),
        ),
        make_tool_call(
            base_time + Duration::seconds(6),
            "Read",
            json!({"file_path": "/project/MEMORY.md"}),
        ),
        make_tool_call(
            base_time + Duration::seconds(7),
            "Read",
            json!({"file_path": "/project/src/main.rs"}),
        ), // Not a config file
    ];

    let signals = compute_behavioral_signals(&events, None);

    // Should detect all config files except src/main.rs
    assert_eq!(signals.read_config_files.len(), 7);
    assert!(signals
        .read_config_files
        .iter()
        .all(|f| f != "/project/src/main.rs"));
}

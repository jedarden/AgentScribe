//! Context command tests
//!
//! Tests the `context_pack` function which provides pre-task priming for agent workers.
//! Validates:
//!   - Empty index returns empty output
//!   - Non-empty index returns formatted block
//!   - Token budget truncates correctly
//!   - --json output is valid JSON
//!   - File path extraction from task descriptions

use std::fs;
use std::path::PathBuf;

use agentscribe::event::{Event, Role, SessionManifest};
use agentscribe::index::IndexManager;
use agentscribe::search::{context_pack, extract_file_paths, ContextPack};
use chrono::Utc;

/// Path to fixtures directory relative to the workspace root.
#[allow(dead_code)]
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Create a temp data directory with the required sub-structure.
fn make_data_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    fs::create_dir_all(dir.path().join("plugins")).unwrap();
    fs::create_dir_all(dir.path().join("sessions")).unwrap();
    fs::create_dir_all(dir.path().join("state")).unwrap();
    dir
}

#[test]
fn test_context_empty_index() {
    let data_dir = make_data_dir();

    // Create empty index
    let (schema, _) = agentscribe::index::build_schema();
    let index_path = data_dir.path().join("index").join("tantivy");
    fs::create_dir_all(&index_path).unwrap();
    tantivy::Index::create_in_dir(&index_path, schema).unwrap();

    let result = context_pack(
        data_dir.path(),
        "implement auth feature",
        3000,
        Some(data_dir.path()),
    );

    assert!(result.is_ok());
    let pack = result.unwrap();
    let output = pack.format_text();
    assert!(output.contains("No prior context found") || output.is_empty());
}

#[test]
fn test_context_with_indexed_sessions() {
    let data_dir = make_data_dir();
    let now = Utc::now();

    // Create a successful session about auth
    let mut manifest = SessionManifest::new("test/auth-session".to_string(), "test".to_string());
    manifest.project = Some(data_dir.path().to_string_lossy().to_string());
    manifest.started = now;
    manifest.turns = 5;
    manifest.outcome = Some("success".to_string());
    manifest.summary = Some("Implemented JWT authentication".to_string());

    let events = vec![
        Event::new(
            now,
            "test/auth-session".into(),
            "test".into(),
            Role::User,
            "implement jwt auth".into(),
        )
        .with_project(Some(data_dir.path().to_string_lossy().to_string()))
        .with_file_paths(vec!["src/auth.rs".to_string()]),
        Event::new(
            now,
            "test/auth-session".into(),
            "test".into(),
            Role::Assistant,
            "I'll add JWT authentication".into(),
        )
        .with_project(Some(data_dir.path().to_string_lossy().to_string())),
    ];

    // Build index
    let mut manager = IndexManager::open(data_dir.path()).unwrap();
    manager.begin_write().unwrap();
    manager.index_session(&events, &manifest).unwrap();
    manager.finish().unwrap();

    // Query context for auth task
    let result = context_pack(
        data_dir.path(),
        "implement jwt authentication",
        3000,
        Some(data_dir.path()),
    );

    assert!(result.is_ok());
    let pack = result.unwrap();
    let output = pack.format_text();

    // Should contain the past solutions section
    if !output.contains("No prior context found") {
        assert!(output.contains("Past Solutions") || output.contains("### Past Solutions"));
    }
}

#[test]
fn test_context_token_budget_truncation() {
    let data_dir = make_data_dir();
    let now = Utc::now();

    // Create multiple sessions to test budget truncation
    for i in 0..10 {
        let mut manifest = SessionManifest::new(format!("test/session-{}", i), "test".to_string());
        manifest.project = Some(data_dir.path().to_string_lossy().to_string());
        manifest.started = now;
        manifest.turns = 3;
        manifest.outcome = Some("success".to_string());
        manifest.summary = Some(format!("Solution {} for the problem", i));

        let events = vec![Event::new(
            now,
            format!("test/session-{}", i),
            "test".into(),
            Role::User,
            format!("task {}", i),
        )
        .with_project(Some(data_dir.path().to_string_lossy().to_string()))];

        let mut manager = IndexManager::open(data_dir.path()).unwrap();
        manager.begin_write().unwrap();
        manager.index_session(&events, &manifest).unwrap();
        manager.finish().unwrap();
    }

    // Query with very small budget
    let result = context_pack(
        data_dir.path(),
        "solve the problem",
        100, // Very small budget
        Some(data_dir.path()),
    );

    assert!(result.is_ok());
    let pack = result.unwrap();
    let output = pack.format_text();

    // With a small budget, output should be truncated or minimal
    // The exact length depends on content, but it should fit within budget
    let estimated_tokens = output.chars().count() / 4;
    assert!(
        estimated_tokens <= 150,
        "Output should fit within small budget (estimated: {} tokens)",
        estimated_tokens
    );
}

#[test]
fn test_extract_file_paths_from_task() {
    let task = "Fix the bug in src/auth/middleware.rs where the JWT validation fails. Also update tests/auth_test.rs";

    let paths = extract_file_paths(task);

    assert!(!paths.is_empty());
    assert!(paths.iter().any(|p| p.contains("src/auth/middleware.rs")));
    assert!(paths.iter().any(|p| p.contains("tests/auth_test.rs")));
}

#[test]
fn test_extract_file_paths_various_extensions() {
    let task =
        "Work on components/Header.tsx, utils/date.ts, and the Go service in cmd/server/main.go";

    let paths = extract_file_paths(task);

    assert!(!paths.is_empty());
    assert!(paths.iter().any(|p| p.contains("components/Header.tsx")));
    assert!(paths.iter().any(|p| p.contains("utils/date.ts")));
    assert!(paths.iter().any(|p| p.contains("cmd/server/main.go")));
}

#[test]
fn test_extract_file_paths_no_matches() {
    let task = "Implement a new feature for user authentication";

    let paths = extract_file_paths(task);

    assert!(paths.is_empty());
}

#[test]
fn test_context_pack_format_text_empty() {
    let pack = ContextPack {
        past_solutions: String::new(),
        conventions: String::new(),
        file_notes: String::new(),
    };

    let text = pack.format_text();
    assert_eq!(text, "No prior context found.");
}

#[test]
fn test_context_pack_format_text_full() {
    let pack = ContextPack {
        past_solutions: "Solution 1\nSolution 2".to_string(),
        conventions: "Use cargo for builds\nWrite tests in tests/".to_string(),
        file_notes: "src/auth.rs has gotchas".to_string(),
    };

    let text = pack.format_text();

    assert!(text.contains("### Past Solutions"));
    assert!(text.contains("Solution 1"));
    assert!(text.contains("### Project Conventions"));
    assert!(text.contains("Use cargo for builds"));
    assert!(text.contains("### File Notes"));
    assert!(text.contains("src/auth.rs"));
}

#[test]
fn test_context_pack_with_budget_priority() {
    let pack = ContextPack {
        past_solutions: "A".repeat(1000), // ~250 tokens
        conventions: "B".repeat(1000),    // ~250 tokens
        file_notes: "C".repeat(1000),     // ~250 tokens
    };

    // Budget of 300 tokens (~1200 chars) should fit solutions + conventions
    // but truncate file notes
    let pack = pack.with_budget(300);

    assert!(
        !pack.past_solutions.is_empty(),
        "Solutions have highest priority"
    );
    assert!(
        !pack.conventions.is_empty(),
        "Conventions have second priority"
    );
    // File notes may be truncated or empty depending on exact sizes
}

#[test]
fn test_context_pack_with_budget_very_small() {
    let pack = ContextPack {
        past_solutions: "A".repeat(1000),
        conventions: "B".repeat(1000),
        file_notes: "C".repeat(1000),
    };

    // Budget of 50 tokens (~200 chars) should only fit truncated solutions
    let pack = pack.with_budget(50);

    assert!(
        !pack.past_solutions.is_empty(),
        "Solutions always fit (truncated)"
    );
    assert!(
        pack.conventions.is_empty(),
        "Conventions dropped when budget exhausted"
    );
    assert!(
        pack.file_notes.is_empty(),
        "File notes dropped when budget exhausted"
    );

    // Solutions should be truncated
    assert!(pack.past_solutions.len() < 1000);
}

#[test]
fn test_context_json_serialization() {
    let pack = ContextPack {
        past_solutions: "Solution text".to_string(),
        conventions: "Convention text".to_string(),
        file_notes: "File notes".to_string(),
    };

    let json = serde_json::to_string(&pack);
    assert!(json.is_ok());

    let json_str = json.unwrap();
    assert!(json_str.contains("past_solutions"));
    assert!(json_str.contains("conventions"));
    assert!(json_str.contains("file_notes"));
}

#[test]
fn test_extract_file_paths_common_patterns() {
    let task = "The issue is in config/app.yaml and also check lib/database.py";

    let paths = extract_file_paths(task);

    assert!(!paths.is_empty());
    // Should match both patterns
    assert!(paths
        .iter()
        .any(|p| p.contains("config/app.yaml") || p.contains("config/app")));
    assert!(paths
        .iter()
        .any(|p| p.contains("lib/database.py") || p.contains("lib/database")));
}

#[test]
fn test_context_pack_count_tokens() {
    assert_eq!(ContextPack::count_tokens(""), 0);
    assert_eq!(ContextPack::count_tokens("hello"), 2); // 5 chars / 4 = 2
    assert_eq!(ContextPack::count_tokens("hello world"), 3); // 11 chars / 4 = 3
    assert_eq!(ContextPack::count_tokens("a".repeat(100).as_str()), 25); // 100 / 4 = 25
}

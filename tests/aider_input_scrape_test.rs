//! Test aider_input scrape path with fixtures
//!
//! This test exercises the full scrape path for aider_input using persistent
//! fixture files in tests/fixtures/aider_input/ to ensure end-to-end wiring works.
//!
//! Acceptance criteria (bf-61un1):
//! - Test loads and parses the chat.md fixture
//! - Test verifies the scrape-path wiring is exercised end-to-end
//! - Test follows existing test patterns in the codebase
//!
//! The scrape path:
//! 1. MarkdownParser::parse() loads the chat.md file
//! 2. Auto-discovers the sibling .aider.input.history file
//! 3. Injects timestamps from input history into user events
//! 4. Returns fully-typed events

use std::path::PathBuf;
use agentscribe::event::Role;
use agentscribe::parser::{FormatParser, MarkdownParser};
use agentscribe::plugin::{LogFormat, Parser, Plugin, PluginMeta, SessionDetection, Source};

/// Helper: create a minimal aider plugin config for testing
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
            array: None,
            envelope: None,
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
fn test_aider_input_scrape_path_with_fixtures() {
    let plugin = create_aider_plugin();

    // Build path to fixture files
    let mut fixtures_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fixtures_path.push("tests/fixtures/aider_input");

    let chat_md = fixtures_path.join("chat.md");
    let input_history = fixtures_path.join(".aider.input.history");

    // Verify fixtures exist
    assert!(
        chat_md.exists(),
        "chat.md fixture should exist at {:?}",
        chat_md
    );
    assert!(
        input_history.exists(),
        ".aider.input.history fixture should exist at {:?}",
        input_history
    );

    // Parse through the FormatParser::parse() scrape path
    // This should auto-discover and load the sibling .aider.input.history file
    let parser = MarkdownParser;
    let events = parser
        .parse(&chat_md, &plugin)
        .expect("parsing should succeed");

    // Verify we got events
    assert!(!events.is_empty(), "should have parsed events from the fixture");

    // Find user events
    let user_events: Vec<_> = events.iter().filter(|e| e.role == Role::User).collect();
    assert_eq!(user_events.len(), 3, "should have 3 user events");

    // Verify each user event has the timestamp from .aider.input.history (not Utc::now())
    // First user event: "Fix the authentication middleware" at 2024-07-06 12:00:30
    assert!(
        user_events[0]
            .content
            .contains("Fix the authentication middleware"),
        "first user event should contain the expected content"
    );
    assert_eq!(
        user_events[0].ts.timestamp(),
        1720267230, // 2024-07-06 12:00:30
        "first user event should have timestamp from input history, not Utc::now()"
    );

    // Second user event: "Add error handling for expired tokens" at 2024-07-06 12:52:25
    assert!(
        user_events[1]
            .content
            .contains("Add error handling for expired tokens"),
        "second user event should contain the expected content"
    );
    assert_eq!(
        user_events[1].ts.timestamp(),
        1720270345, // 2024-07-06 12:52:25
        "second user event should have timestamp from input history, not Utc::now()"
    );

    // Third user event: "Test the authentication flow" at 2024-07-06 13:18:55
    assert!(
        user_events[2]
            .content
            .contains("Test the authentication flow"),
        "third user event should contain the expected content"
    );
    assert_eq!(
        user_events[2].ts.timestamp(),
        1720272135, // 2024-07-06 13:18:55
        "third user event should have timestamp from input history, not Utc::now()"
    );

    // Verify assistant and tool events were also parsed
    let assistant_events: Vec<_> = events
        .iter()
        .filter(|e| e.role == Role::Assistant)
        .collect();
    let tool_events: Vec<_> = events
        .iter()
        .filter(|e| e.role == Role::ToolResult)
        .collect();

    assert!(!assistant_events.is_empty(), "should have assistant events");
    assert!(!tool_events.is_empty(), "should have tool result events");

    println!("✓ Aider input scrape-path test passed!");
    println!("  - Parsed {} total events", events.len());
    println!("  - Found {} user events with correct timestamps", user_events.len());
    println!("  - Found {} assistant events", assistant_events.len());
    println!("  - Found {} tool events", tool_events.len());
}

#[test]
fn test_aider_input_fixture_files_exist() {
    // Verify the fixture files exist and are readable
    let mut fixtures_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fixtures_path.push("tests/fixtures/aider_input");

    let chat_md = fixtures_path.join("chat.md");
    let input_history = fixtures_path.join(".aider.input.history");

    assert!(
        chat_md.exists(),
        "chat.md fixture should exist at {:?}",
        chat_md
    );
    assert!(
        input_history.exists(),
        ".aider.input.history fixture should exist at {:?}",
        input_history
    );

    // Read and verify the content format
    let chat_content = std::fs::read_to_string(&chat_md)
        .expect("chat.md should be readable");
    let history_content = std::fs::read_to_string(&input_history)
        .expect(".aider.input.history should be readable");

    assert!(
        chat_content.contains("# aider chat started at"),
        "chat.md should contain session delimiter"
    );
    assert!(
        chat_content.contains("#### Fix the authentication middleware"),
        "chat.md should contain first user prompt"
    );

    assert!(
        history_content.contains("# 2024-07-06 12:00:30"),
        ".aider.input.history should contain first timestamp"
    );
    assert!(
        history_content.contains("+ Fix the authentication middleware"),
        ".aider.input.history should contain first user input"
    );

    println!("✓ Fixture files exist and have correct format!");
}

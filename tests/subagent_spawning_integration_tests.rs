//! Integration tests for subagent spawning flow
//!
//! This test suite verifies that the full subagent spawning flow correctly propagates
//! parent_session_id throughout the lifecycle: main → subagent → grandchild.
//!
//! These tests:
//! - Create real session files on disk
//! - Use the actual spawning mechanism (not mocked)
//! - Verify database persistence of parent_session_id
//! - Cover the full lifecycle: main → subagent → grandchild
//! - Test actual Tantivy index storage and retrieval

use std::fs;

use agentscribe::index::IndexManager;
use agentscribe::plugin::{
    LogFormat, Parser, Plugin, PluginMeta, SessionDetection, SessionIdSource, Source,
};
use agentscribe::scraper::Scraper;
use agentscribe::search;
use agentscribe::search::SearchOptions;
use tantivy::{Searcher, TantivyDocument};

// ─── Test helpers ─────────────────────────────────────────────────────────────

/// Create a temp data directory with the required sub-structure.
fn make_data_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    fs::create_dir_all(dir.path().join("plugins")).unwrap();
    fs::create_dir_all(dir.path().join("sessions")).unwrap();
    fs::create_dir_all(dir.path().join("state")).unwrap();
    dir
}

/// Create a minimal JSONL plugin for testing.
fn jsonl_plugin(name: &str, glob: &str) -> Plugin {
    Plugin {
        plugin: PluginMeta {
            name: name.to_string(),
            version: "1.0".to_string(),
        },
        source: Source {
            paths: vec![glob.to_string()],
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
            static_fields: {
                let mut map = std::collections::HashMap::new();
                map.insert("source_agent".to_string(), serde_json::json!(name));
                map
            },
            ..Default::default()
        },
        metadata: None,
    }
}

/// Create test JSONL content with minimal events.
fn test_jsonl_content() -> String {
    r#"{"timestamp": "2026-07-23T10:00:00Z", "role": "user", "content": "Test message"}
{"timestamp": "2026-07-23T10:00:01Z", "role": "assistant", "content": "Test response"}"#
        .to_string()
}

/// Create JSONL content with specific source_agent.
fn test_jsonl_content_with_source(source_agent: &str) -> String {
    format!(
        r#"{{"timestamp": "2026-07-23T10:00:00Z", "role": "user", "content": "Test message", "source_agent": "{}" }}
{{"timestamp": "2026-07-23T10:00:01Z", "role": "assistant", "content": "Test response", "source_agent": "{}" }}"#,
        source_agent, source_agent
    )
}

// ─── Integration Test 1: Main → Subagent → Grandchild Lifecycle ────────────────────

#[test]
fn test_full_lifecycle_main_to_grandchild() {
    // Test the complete lifecycle: main session → subagent → grandchild
    // Verify parent_session_id propagation at each level

    let data_dir = make_data_dir();
    let claude_dir = data_dir.path().join("sessions/claude-code");

    // 1. Create main session (parent_session_id = None)
    let main_session_id = "main-session-integration-test";
    let main_path = claude_dir.join(format!("{}.jsonl", main_session_id));
    fs::create_dir_all(main_path.parent().unwrap()).expect("Failed to create main directory");
    fs::write(&main_path, test_jsonl_content()).expect("Failed to write main session content");

    // 2. Create subagent session (parent_session_id = main_session_id)
    let subagent_session_id = "subagent-session-integration-test";
    let subagent_path = claude_dir
        .join(main_session_id)
        .join("subagents")
        .join(format!("{}.jsonl", subagent_session_id));

    fs::create_dir_all(subagent_path.parent().unwrap())
        .expect("Failed to create subagent directory");
    fs::write(
        &subagent_path,
        test_jsonl_content_with_source("claude-code-subagent"),
    )
    .expect("Failed to write subagent content");

    // 3. Create grandchild session (parent_session_id = subagent_session_id)
    let grandchild_session_id = "grandchild-session-integration-test";
    let grandchild_path = claude_dir
        .join(main_session_id)
        .join("subagents")
        .join(subagent_session_id)
        .join("subagents")
        .join(format!("{}.jsonl", grandchild_session_id));

    fs::create_dir_all(grandchild_path.parent().unwrap())
        .expect("Failed to create grandchild directory");
    fs::write(
        &grandchild_path,
        test_jsonl_content_with_source("claude-code-subagent"),
    )
    .expect("Failed to write grandchild content");

    // 4. Create scraper and plugin
    let mut scraper =
        Scraper::new(data_dir.path().to_path_buf()).expect("Failed to create scraper");

    let plugin = jsonl_plugin(
        "claude-code",
        claude_dir.join("**/*.jsonl").to_str().unwrap(),
    );

    scraper.plugin_manager_mut().add_plugin(plugin);

    // 5. Scrape the plugin (this will parse and index all sessions)
    let plugin_name = "claude-code";
    let plugin = scraper.plugin_manager().get(plugin_name).unwrap().clone();
    let result = scraper
        .scrape_plugin(&plugin)
        .expect("Scrape should succeed");

    // Verify all sessions were scraped
    assert_eq!(
        result.sessions_scraped, 3,
        "Should scrape main, subagent, and grandchild sessions"
    );

    assert_eq!(
        result.sessions_indexed, 3,
        "Should index all three sessions"
    );

    // 6. Verify parent_session_id propagation via index search
    let index_manager = IndexManager::open(data_dir.path()).expect("Failed to open index");
    let index = index_manager.index();
    let reader = index
        .reader_builder()
        .reload_policy(tantivy::ReloadPolicy::OnCommitWithDelay)
        .try_into()
        .expect("Failed to create index reader");

    let searcher = &reader.searcher();

    // Verify main session has no parent_session_id
    let main_session_id_full = format!("claude-code/{}", main_session_id);
    let main_doc = search_by_session_id(searcher, &main_session_id_full)
        .expect("Should find main session in index");

    let main_parent = get_doc_parent_session_id(searcher, &main_doc);
    assert!(
        main_parent.is_none(),
        "Main session should have no parent_session_id, got: {:?}",
        main_parent
    );

    // Verify subagent session has correct parent_session_id
    let subagent_session_id_full = format!(
        "claude-code/{}/{}/{}",
        main_session_id, "subagents", subagent_session_id
    );
    let subagent_doc = search_by_session_id(searcher, &subagent_session_id_full)
        .expect("Should find subagent session in index");

    let subagent_parent = get_doc_parent_session_id(searcher, &subagent_doc);
    assert_eq!(
        subagent_parent,
        Some(main_session_id_full.clone()),
        "Subagent session should have parent_session_id = main session ID, got: {:?}",
        subagent_parent
    );

    // Verify grandchild session has correct parent_session_id (should be subagent session ID)
    let grandchild_session_id_full = format!(
        "claude-code/{}/{}/{}/{}/{}",
        main_session_id, "subagents", subagent_session_id, "subagents", grandchild_session_id
    );
    let grandchild_doc = search_by_session_id(searcher, &grandchild_session_id_full)
        .expect("Should find grandchild session in index");

    let grandchild_parent = get_doc_parent_session_id(searcher, &grandchild_doc);
    assert_eq!(
        grandchild_parent,
        Some(subagent_session_id_full),
        "Grandchild session should have parent_session_id = subagent session ID, got: {:?}",
        grandchild_parent
    );
}

// ─── Integration Test 2: Database Persistence Verification ────────────────────────

#[test]
fn test_parent_session_id_database_persistence() {
    // Test that parent_session_id is correctly persisted in the Tantivy index
    // and can be retrieved after scraping

    let data_dir = make_data_dir();
    let claude_dir = data_dir.path().join("sessions/claude-code");

    // Create parent session
    let parent_session_id = "parent-db-test-123";
    let parent_path = claude_dir.join(format!("{}.jsonl", parent_session_id));
    fs::create_dir_all(parent_path.parent().unwrap()).expect("Failed to create parent directory");
    fs::write(&parent_path, test_jsonl_content()).expect("Failed to write parent content");

    // Create subagent session
    let subagent_session_id = "subagent-db-test-456";
    let subagent_path = claude_dir
        .join(parent_session_id)
        .join("subagents")
        .join(format!("{}.jsonl", subagent_session_id));

    fs::create_dir_all(subagent_path.parent().unwrap())
        .expect("Failed to create subagent directory");
    fs::write(
        &subagent_path,
        test_jsonl_content_with_source("claude-code-subagent"),
    )
    .expect("Failed to write subagent content");

    // Create scraper and plugin
    let mut scraper =
        Scraper::new(data_dir.path().to_path_buf()).expect("Failed to create scraper");

    let plugin = jsonl_plugin(
        "claude-code",
        claude_dir.join("**/*.jsonl").to_str().unwrap(),
    );

    scraper.plugin_manager_mut().add_plugin(plugin);

    // Scrape the plugin
    let plugin_name = "claude-code";
    let plugin = scraper.plugin_manager().get(plugin_name).unwrap().clone();
    let result = scraper
        .scrape_plugin(&plugin)
        .expect("Scrape should succeed");

    assert_eq!(result.sessions_indexed, 2, "Should index both sessions");

    // Open index and verify persistence
    let index_manager = IndexManager::open(data_dir.path()).expect("Failed to open index");
    let index = index_manager.index();
    let reader = index
        .reader_builder()
        .reload_policy(tantivy::ReloadPolicy::OnCommitWithDelay)
        .try_into()
        .expect("Failed to create index reader");

    let searcher = &reader.searcher();

    // Verify we can search for sessions by parent_session_id
    let subagent_session_id_full = format!(
        "claude-code/{}/{}/{}",
        parent_session_id, "subagents", subagent_session_id
    );

    let subagent_doc = search_by_session_id(searcher, &subagent_session_id_full)
        .expect("Should find subagent session");

    // Verify parent_session_id field is stored
    let parent_session_id_field = index
        .schema()
        .get_field("parent_session_id")
        .expect("parent_session_id field should exist in schema");

    use tantivy::schema::Value;
    let stored_parent_id = subagent_doc
        .get_first(parent_session_id_field)
        .and_then(|v| v.as_str());

    assert_eq!(
        stored_parent_id,
        Some(format!("claude-code/{}", parent_session_id).as_str()),
        "parent_session_id should be persisted in Tantivy index"
    );

    // Verify we can search using SearchOptions
    let search_options = SearchOptions {
        session_id: Some(subagent_session_id_full.clone()),
        ..Default::default()
    };

    // This search should find the subagent session
    let search_results =
        search::execute_search(data_dir.path(), &search_options).expect("Search should succeed");

    assert_eq!(
        search_results.results.len(),
        1,
        "Should find exactly one session"
    );

    assert_eq!(
        search_results.results[0].session_id, subagent_session_id_full,
        "Should find the subagent session"
    );
}

// ─── Integration Test 3: Multiple Subagents Same Parent ───────────────────────────

#[test]
fn test_multiple_subagents_same_parent_propagation() {
    // Test that multiple subagent sessions from the same parent all have
    // the correct parent_session_id

    let data_dir = make_data_dir();
    let claude_dir = data_dir.path().join("sessions/claude-code");

    let parent_session_id = "parent-multi-test-789";
    let subagent_count = 5;

    // Create parent session
    let parent_path = claude_dir.join(format!("{}.jsonl", parent_session_id));
    fs::create_dir_all(parent_path.parent().unwrap()).expect("Failed to create parent directory");
    fs::write(&parent_path, test_jsonl_content()).expect("Failed to write parent content");

    // Create multiple subagent sessions
    for i in 0..subagent_count {
        let subagent_session_id = format!("subagent-multi-{:03}", i);
        let subagent_path = claude_dir
            .join(parent_session_id)
            .join("subagents")
            .join(format!("{}.jsonl", subagent_session_id));

        fs::create_dir_all(subagent_path.parent().unwrap())
            .expect("Failed to create subagent directory");
        fs::write(
            &subagent_path,
            test_jsonl_content_with_source("claude-code-subagent"),
        )
        .expect("Failed to write subagent content");
    }

    // Create scraper and plugin
    let mut scraper =
        Scraper::new(data_dir.path().to_path_buf()).expect("Failed to create scraper");

    let plugin = jsonl_plugin(
        "claude-code",
        claude_dir.join("**/*.jsonl").to_str().unwrap(),
    );

    scraper.plugin_manager_mut().add_plugin(plugin);

    // Scrape the plugin
    let plugin_name = "claude-code";
    let plugin = scraper.plugin_manager().get(plugin_name).unwrap().clone();
    let result = scraper
        .scrape_plugin(&plugin)
        .expect("Scrape should succeed");

    assert_eq!(
        result.sessions_scraped,
        1 + subagent_count,
        "Should scrape parent and all subagent sessions"
    );

    // Verify all subagents have correct parent_session_id
    let index_manager = IndexManager::open(data_dir.path()).expect("Failed to open index");
    let index = index_manager.index();
    let reader = index
        .reader_builder()
        .reload_policy(tantivy::ReloadPolicy::OnCommitWithDelay)
        .try_into()
        .expect("Failed to create index reader");

    let searcher = &reader.searcher();
    let parent_session_id_full = format!("claude-code/{}", parent_session_id);

    for i in 0..subagent_count {
        let subagent_session_id = format!("subagent-multi-{:03}", i);
        let subagent_session_id_full = format!(
            "claude-code/{}/{}/{}",
            parent_session_id, "subagents", subagent_session_id
        );

        let subagent_doc = search_by_session_id(searcher, &subagent_session_id_full)
            .unwrap_or_else(|| panic!("Should find subagent session {}", i));

        let subagent_parent = get_doc_parent_session_id(searcher, &subagent_doc);
        assert_eq!(
            subagent_parent,
            Some(parent_session_id_full.clone()),
            "Subagent {} should have correct parent_session_id",
            i
        );
    }
}

// ─── Integration Test 4: Deep Nesting Verification ───────────────────────────────────

#[test]
fn test_deep_nesting_parent_session_id_propagation() {
    // Test parent_session_id propagation with deeply nested subagents (4+ levels)
    // Verify that each level correctly identifies its direct parent

    let data_dir = make_data_dir();
    let claude_dir = data_dir.path().join("sessions/claude-code");

    // Level 0: Main session
    let level0_id = "level0-main";
    let level0_path = claude_dir.join(format!("{}.jsonl", level0_id));
    fs::create_dir_all(level0_path.parent().unwrap()).unwrap();
    fs::write(&level0_path, test_jsonl_content()).unwrap();

    // Level 1: First subagent
    let level1_id = "level1-subagent";
    let level1_path = claude_dir
        .join(level0_id)
        .join("subagents")
        .join(format!("{}.jsonl", level1_id));
    fs::create_dir_all(level1_path.parent().unwrap()).unwrap();
    fs::write(
        &level1_path,
        test_jsonl_content_with_source("claude-code-subagent"),
    )
    .unwrap();

    // Level 2: Second subagent (child of level1)
    let level2_id = "level2-subagent";
    let level2_path = claude_dir
        .join(level0_id)
        .join("subagents")
        .join(level1_id)
        .join("subagents")
        .join(format!("{}.jsonl", level2_id));
    fs::create_dir_all(level2_path.parent().unwrap()).unwrap();
    fs::write(
        &level2_path,
        test_jsonl_content_with_source("claude-code-subagent"),
    )
    .unwrap();

    // Level 3: Third subagent (child of level2)
    let level3_id = "level3-subagent";
    let level3_path = claude_dir
        .join(level0_id)
        .join("subagents")
        .join(level1_id)
        .join("subagents")
        .join(level2_id)
        .join("subagents")
        .join(format!("{}.jsonl", level3_id));
    fs::create_dir_all(level3_path.parent().unwrap()).unwrap();
    fs::write(
        &level3_path,
        test_jsonl_content_with_source("claude-code-subagent"),
    )
    .unwrap();

    // Level 4: Fourth subagent (child of level3)
    let level4_id = "level4-subagent";
    let level4_path = claude_dir
        .join(level0_id)
        .join("subagents")
        .join(level1_id)
        .join("subagents")
        .join(level2_id)
        .join("subagents")
        .join(level3_id)
        .join("subagents")
        .join(format!("{}.jsonl", level4_id));
    fs::create_dir_all(level4_path.parent().unwrap()).unwrap();
    fs::write(
        &level4_path,
        test_jsonl_content_with_source("claude-code-subagent"),
    )
    .unwrap();

    // Create scraper and plugin
    let mut scraper =
        Scraper::new(data_dir.path().to_path_buf()).expect("Failed to create scraper");

    let plugin = jsonl_plugin(
        "claude-code",
        claude_dir.join("**/*.jsonl").to_str().unwrap(),
    );

    scraper.plugin_manager_mut().add_plugin(plugin);

    // Scrape the plugin
    let plugin_name = "claude-code";
    let plugin = scraper.plugin_manager().get(plugin_name).unwrap().clone();
    let result = scraper
        .scrape_plugin(&plugin)
        .expect("Scrape should succeed");

    assert_eq!(result.sessions_scraped, 5, "Should scrape all 5 levels");

    // Verify parent_session_id chain
    let index_manager = IndexManager::open(data_dir.path()).expect("Failed to open index");
    let index = index_manager.index();
    let reader = index
        .reader_builder()
        .reload_policy(tantivy::ReloadPolicy::OnCommitWithDelay)
        .try_into()
        .expect("Failed to create index reader");

    let searcher = &reader.searcher();

    // Level 0: No parent
    let level0_full = format!("claude-code/{}", level0_id);
    let level0_doc = search_by_session_id(searcher, &level0_full).expect("Should find level0");
    assert!(
        get_doc_parent_session_id(searcher, &level0_doc).is_none(),
        "Level0 should have no parent"
    );

    // Level 1: Parent is level0
    let level1_full = format!("claude-code/{}/{}/{}", level0_id, "subagents", level1_id);
    let level1_doc = search_by_session_id(searcher, &level1_full).expect("Should find level1");
    assert_eq!(
        get_doc_parent_session_id(searcher, &level1_doc),
        Some(level0_full.clone()),
        "Level1 parent should be level0"
    );

    // Level 2: Parent is level1
    let level2_full = format!(
        "claude-code/{}/{}/{}/{}/{}",
        level0_id, "subagents", level1_id, "subagents", level2_id
    );
    let level2_doc = search_by_session_id(searcher, &level2_full).expect("Should find level2");
    assert_eq!(
        get_doc_parent_session_id(searcher, &level2_doc),
        Some(level1_full.clone()),
        "Level2 parent should be level1"
    );

    // Level 3: Parent is level2
    let level3_full = format!(
        "claude-code/{}/{}/{}/{}/{}/{}/{}/{}",
        level0_id, "subagents", level1_id, "subagents", level2_id, "subagents", level3_id, ""
    )
    .trim_end_matches('/')
    .to_string();
    let level3_doc = search_by_session_id(searcher, &level3_full).expect("Should find level3");
    assert_eq!(
        get_doc_parent_session_id(searcher, &level3_doc),
        Some(level2_full.clone()),
        "Level3 parent should be level2"
    );

    // Level 4: Parent is level3
    let level4_full = format!(
        "claude-code/{}/{}/{}/{}/{}/{}/{}/{}/{}/{}",
        level0_id,
        "subagents",
        level1_id,
        "subagents",
        level2_id,
        "subagents",
        level3_id,
        "subagents",
        level4_id,
        ""
    )
    .trim_end_matches('/')
    .to_string();
    let level4_doc = search_by_session_id(searcher, &level4_full).expect("Should find level4");
    assert_eq!(
        get_doc_parent_session_id(searcher, &level4_doc),
        Some(level3_full.clone()),
        "Level4 parent should be level3"
    );
}

// ─── Helper Functions ─────────────────────────────────────────────────────────────

/// Search for a document by session_id in the Tantivy index
fn search_by_session_id(searcher: &Searcher, session_id: &str) -> Option<TantivyDocument> {
    use tantivy::query::TermQuery;
    use tantivy::schema::Term;

    let schema = searcher.schema();
    let session_id_field = schema.get_field("session_id").unwrap();

    let term = Term::from_field_text(session_id_field, session_id);
    let query = TermQuery::new(term, tantivy::schema::IndexRecordOption::Basic);

    let top_docs = searcher
        .search(&query, &tantivy::collector::TopDocs::with_limit(1))
        .expect("Search should succeed");

    top_docs
        .first()
        .map(|(_score, doc_address)| searcher.doc(*doc_address))
        .transpose()
        .ok()
        .flatten()
}

/// Get the parent_session_id value from a Tantivy document
fn get_doc_parent_session_id(searcher: &Searcher, doc: &TantivyDocument) -> Option<String> {
    use tantivy::schema::Value;

    let schema = searcher.schema();
    let parent_session_id_field = schema.get_field("parent_session_id").ok()?;

    doc.get_first(parent_session_id_field)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

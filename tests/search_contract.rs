//! Tests for search exit-code contract (Phase 9, bf-3g45 split)
//!
//! Exit-code contract:
//! - Exit 0 with populated results when matches are found
//! - Exit 0 with empty results[] when nothing matches (NOT an error)
//! - Non-zero only for real failures (bad args, corrupt index, etc.)
//!
//! This contract allows fire-and-forget usage by agents and shell integrations.

use agentscribe::error::{AgentScribeError, Result};
use agentscribe::search::execute_search;
use agentscribe::search::SearchOptions;
use std::path::PathBuf;
use tempfile::TempDir;

/// Helper to create a minimal empty Tantivy index
fn create_empty_index() -> Result<(TempDir, PathBuf)> {
    let temp_dir = TempDir::new()
        .map_err(|e| AgentScribeError::Config(format!("Failed to create temp dir: {}", e)))?;

    let data_dir = temp_dir.path().join("agentscribe");
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| AgentScribeError::Config(format!("Failed to create data dir: {}", e)))?;

    // Create an empty index using IndexManager
    // IndexManager::open() already creates the index structure if it doesn't exist
    let _index_manager = agentscribe::index::IndexManager::open(&data_dir)
        .map_err(|e| AgentScribeError::Config(format!("Failed to create index manager: {}", e)))?;

    Ok((temp_dir, data_dir))
}

#[test]
fn test_search_empty_index_returns_ok_with_empty_results() {
    // Setup: Empty index (no sessions indexed yet)
    let (_temp_dir, data_dir) = create_empty_index().unwrap();

    let opts = SearchOptions {
        query: Some("nonexistent query".to_string()),
        ..Default::default()
    };

    // Execute search against empty index
    let result = execute_search(&data_dir, &opts);

    // Contract: Empty index should return Ok with empty results, NOT an error
    assert!(
        result.is_ok(),
        "Search against empty index should return Ok, not Err"
    );

    let output = result.unwrap();
    assert_eq!(
        output.results.len(),
        0,
        "Empty index should return zero results"
    );
    assert_eq!(
        output.total_matches, 0,
        "total_matches should be 0 for empty index"
    );
}

#[test]
fn test_search_no_match_query_returns_ok_with_empty_results() {
    // Search against empty index - equivalent to no-match query
    let (_temp_dir, data_dir) = create_empty_index().unwrap();

    let opts = SearchOptions {
        query: Some("gibberish query that matches nothing".to_string()),
        ..Default::default()
    };

    let result = execute_search(&data_dir, &opts);

    // Contract: No-match query should return Ok with empty results, NOT an error
    assert!(
        result.is_ok(),
        "No-match query should return Ok with empty results, not Err"
    );

    let output = result.unwrap();
    assert_eq!(output.results.len(), 0);
}

#[test]
fn test_search_without_query_returns_error() {
    let (_temp_dir, data_dir) = create_empty_index().unwrap();

    let opts = SearchOptions {
        query: None,
        error_pattern: None,
        code_query: None,
        like_session: None,
        file_path: None,
        git_commit: None,
        anti_patterns: false,
        ..Default::default()
    };

    let result = execute_search(&data_dir, &opts);

    // Contract: Missing required query should return error (non-zero exit)
    assert!(
        result.is_err(),
        "Search without any query or filters should return Err"
    );

    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("No search query provided"),
        "Error should mention missing query"
    );
}

#[test]
fn test_search_output_json_serializes_correctly_for_empty_results() {
    let (_temp_dir, data_dir) = create_empty_index().unwrap();

    let opts = SearchOptions {
        query: Some("test".to_string()),
        ..Default::default()
    };

    let result = execute_search(&data_dir, &opts).unwrap();

    // Verify JSON serialization works for empty results
    let json_output = serde_json::to_string_pretty(&result);
    assert!(
        json_output.is_ok(),
        "SearchOutput should serialize to JSON even with empty results"
    );

    let json_str = json_output.unwrap();
    assert!(
        json_str.contains("\"results\": []"),
        "JSON output should have empty results array"
    );
    assert!(
        json_str.contains("\"total_matches\": 0"),
        "JSON output should show zero matches"
    );
}

#[test]
fn test_search_with_invalid_session_id_returns_ok_empty_results() {
    // Contract: Session lookup for non-existent ID returns Ok with empty results
    // This is NOT an error - it's a "no match" condition
    let (_temp_dir, data_dir) = create_empty_index().unwrap();

    let opts = SearchOptions {
        session_id: Some("nonexistent-session-id".to_string()),
        ..Default::default()
    };

    let result = execute_search(&data_dir, &opts);

    // Session lookup returns empty results, NOT an error
    assert!(
        result.is_ok(),
        "Lookup of nonexistent session should return Ok with empty results, not Err"
    );

    let output = result.unwrap();
    assert_eq!(output.results.len(), 0);
}

#[test]
fn test_search_semantic_stub_returns_error() {
    let (_temp_dir, data_dir) = create_empty_index().unwrap();

    let opts = SearchOptions {
        query: Some("test".to_string()),
        semantic: true,
        ..Default::default()
    };

    let result = execute_search(&data_dir, &opts);

    // Contract: Semantic search (stub mode) should return clear error
    assert!(
        result.is_err(),
        "Semantic search (non-functional stub) should return Err"
    );

    let err = result.unwrap_err();
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("non-functional")
            || err_msg.contains("stub")
            || err_msg.contains("turbovec"),
        "Error should explain stub mode, got: {}",
        err_msg
    );
}

#[test]
fn test_search_hybrid_stub_returns_error() {
    let (_temp_dir, data_dir) = create_empty_index().unwrap();

    let opts = SearchOptions {
        query: Some("test".to_string()),
        hybrid: true,
        ..Default::default()
    };

    let result = execute_search(&data_dir, &opts);

    // Contract: Hybrid search (stub mode) should return clear error
    assert!(
        result.is_err(),
        "Hybrid search (non-functional stub) should return Err"
    );

    let err = result.unwrap_err();
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("non-functional")
            || err_msg.contains("stub")
            || err_msg.contains("turbovec"),
        "Error should explain stub mode, got: {}",
        err_msg
    );
}

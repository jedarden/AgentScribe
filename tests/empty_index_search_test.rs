//! Test search operations on empty AgentScribe index
//!
//! This module demonstrates that search operations work correctly
//! when the index contains no documents, returning empty results
//! rather than errors.

use agentscribe::event::{Event, Role, SessionManifest};
use agentscribe::index::IndexManager;
use agentscribe::search::{execute_search, SearchOptions};
use chrono::Utc;
use std::path::Path;

mod test_helpers;
use test_helpers::setup_empty_index;

#[test]
fn test_search_returns_empty_results_on_empty_index() {
    // Create an empty index
    let (temp_dir, _index_manager) = setup_empty_index();
    let data_dir = temp_dir.path().join(".agentscribe");

    // Attempt to search for any content
    let options = SearchOptions {
        query: Some("test query that won't match anything".to_string()),
        max_results: 10,
        ..Default::default()
    };

    // Search should succeed but return empty results
    let results = execute_search(&data_dir, &options).unwrap();
    assert!(
        results.results.is_empty(),
        "Empty index should return empty results"
    );
    assert_eq!(results.total_matches, 0);
}

#[test]
fn test_search_with_filters_on_empty_index() {
    // Create an empty index
    let (temp_dir, _index_manager) = setup_empty_index();
    let data_dir = temp_dir.path().join(".agentscribe");

    // Search with various filters
    let options = SearchOptions {
        query: Some("anything".to_string()),
        max_results: 5,
        agent: vec!["claude-code".to_string()],
        project: Some("/home/coding/test".to_string()),
        outcome: Some("success".to_string()),
        ..Default::default()
    };

    // Should return empty results, not error
    let results = execute_search(&data_dir, &options).unwrap();
    assert!(results.results.is_empty());
    assert_eq!(results.total_matches, 0);
}

#[test]
fn test_empty_index_can_receive_documents() {
    // Create an empty index
    let (_temp_dir, mut index_manager) = setup_empty_index();

    // Verify it's initially empty
    let reader_before = index_manager.index().reader().unwrap();
    let searcher_before = reader_before.searcher();
    assert_eq!(searcher_before.num_docs(), 0);

    // Add a document
    let now = Utc::now();
    let events = vec![Event::new(
        now,
        "test/1".to_string(),
        "test".to_string(),
        Role::User,
        "hello world".to_string(),
    )];

    let mut manifest = SessionManifest::new("test/1".to_string(), "test".to_string());
    manifest.project = Some("/home/coding/test".to_string());

    index_manager.begin_write().unwrap();
    index_manager.index_session(&events, &manifest).unwrap();
    index_manager.finish().unwrap();

    // Verify document was added
    let reader_after = index_manager.index().reader().unwrap();
    let searcher_after = reader_after.searcher();
    assert_eq!(searcher_after.num_docs(), 1, "Document should be indexed");
}

#[test]
fn test_empty_index_schema_fields_accessible() {
    // Create an empty index
    let (_temp_dir, index_manager) = setup_empty_index();

    // Verify schema fields are accessible
    let schema = index_manager.schema();

    // Critical search fields should exist
    assert!(schema.get_field("content").is_ok());
    assert!(schema.get_field("session_id").is_ok());
    assert!(schema.get_field("timestamp").is_ok());
    assert!(schema.get_field("source_agent").is_ok());
    assert!(schema.get_field("project").is_ok());
    assert!(schema.get_field("tags").is_ok());
    assert!(schema.get_field("outcome").is_ok());
}

#[test]
fn test_empty_index_supports_multiple_searches() {
    // Create an empty index
    let (_temp_dir, index_manager) = setup_empty_index();

    // Perform multiple searches (simulating concurrent access)
    for query in &["test1", "test2", "test3"] {
        let options = SearchOptions {
            query: query.to_string(),
            max_results: 10,
            ..Default::default()
        };

        let results = execute_search(&index_manager, &options).unwrap();
        assert!(results.is_empty());
    }
}

#[test]
fn test_empty_index_handles_large_max_results() {
    // Create an empty index
    let (_temp_dir, index_manager) = setup_empty_index();

    // Request many results from empty index
    let options = SearchOptions {
        query: "anything".to_string(),
        max_results: 10000,
        ..Default::default()
    };

    let results = execute_search(&index_manager, &options).unwrap();
    assert!(
        results.is_empty(),
        "Even with large max_results, should return empty"
    );
}

#[test]
fn test_empty_index_search_with_all_filter_types() {
    // Create an empty index
    let (_temp_dir, index_manager) = setup_empty_index();

    // Search with all possible filter types
    let options = SearchOptions {
        query: "comprehensive query".to_string(),
        max_results: 10,
        agent: vec!["claude-code".to_string(), "aider".to_string()],
        project: Some("/home/coding/test".to_string()),
        outcome: Some("success".to_string()),
        tag: vec!["rust".to_string(), "async".to_string()],
        ..Default::default()
    };

    // Should handle all filters gracefully and return empty results
    let results = execute_search(&index_manager, &options).unwrap();
    assert!(results.is_empty());
}

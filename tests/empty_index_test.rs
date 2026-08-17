//! Tests for empty AgentScribe index behavior
//!
//! This module tests the setup and usage of empty Tantivy indices for
//! testing search behavior when no documents have been indexed yet.

mod test_helpers;

use agentscribe::index::IndexManager;
use test_helpers::setup_empty_index;

#[test]
fn test_empty_index_creation() {
    // Create an empty index using the helper function
    let (temp_dir, _index_manager) = setup_empty_index();

    // Verify the index directory exists
    let index_path = temp_dir.path().join(".agentscribe/index/tantivy");
    assert!(
        index_path.exists(),
        "Index directory should exist at: {}",
        index_path.display()
    );

    // Verify it's a directory
    assert!(index_path.is_dir(), "Index path should be a directory");
}

#[test]
fn test_empty_index_has_zero_documents() {
    // Create an empty index
    let (_temp_dir, index_manager) = setup_empty_index();

    // Get a reader and searcher to verify document count
    let reader = index_manager
        .index()
        .reader()
        .expect("Failed to create index reader");
    let searcher = reader.searcher();

    // Verify no documents are indexed
    assert_eq!(
        searcher.num_docs(),
        0,
        "Empty index should have zero documents"
    );
}

#[test]
fn test_empty_index_is_searchable() {
    // Create an empty index
    let (_temp_dir, index_manager) = setup_empty_index();

    // Verify we can create a reader (search operations work)
    let reader = index_manager
        .index()
        .reader()
        .expect("Failed to create index reader");

    // Verify searcher can be created
    let searcher = reader.searcher();
    assert_eq!(searcher.num_docs(), 0);

    // Verify index schema is accessible
    let schema = index_manager.index().schema();
    assert!(schema.get_field("content").is_ok());
    assert!(schema.get_field("session_id").is_ok());
    assert!(schema.get_field("timestamp").is_ok());
}

#[test]
fn test_empty_index_supports_write_operations() {
    // Create an empty index
    let (_temp_dir, mut index_manager) = setup_empty_index();

    // Verify we can begin a write session
    assert!(
        index_manager.begin_write().is_ok(),
        "Should be able to begin write session on empty index"
    );

    // Verify we can finish the write session
    assert!(
        index_manager.finish().is_ok(),
        "Should be able to finish write session on empty index"
    );
}

#[test]
fn test_empty_index_persists_across_reopen() {
    use tempfile::TempDir;

    // Create first index manager and close it
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let data_dir = temp_dir.path().join(".agentscribe");
    std::fs::create_dir_all(data_dir.join("index")).unwrap();

    {
        let _manager = IndexManager::open(&data_dir).expect("Failed to create initial index");
    }

    // Reopen the same index
    let manager2 = IndexManager::open(&data_dir).expect("Failed to reopen index");

    // Verify it's still empty
    let reader = manager2.index().reader().unwrap();
    let searcher = reader.searcher();
    assert_eq!(searcher.num_docs(), 0);
}

#[test]
fn test_multiple_empty_indices_are_independent() {
    // Create two separate empty indices
    let (temp_dir1, _manager1) = setup_empty_index();
    let (temp_dir2, _manager2) = setup_empty_index();

    // Verify they have different paths
    assert_ne!(
        temp_dir1.path(),
        temp_dir2.path(),
        "Multiple empty indices should be independent"
    );
}

#[test]
fn test_empty_index_path_is_documented() {
    // Create an empty index
    let (temp_dir, _index_manager) = setup_empty_index();

    // The documented path structure is:
    // <temp_dir>/.agentscribe/index/tantivy
    let expected_path = temp_dir.path().join(".agentscribe/index/tantivy");

    assert!(
        expected_path.exists(),
        "Empty index should exist at documented path: {}",
        expected_path.display()
    );
}

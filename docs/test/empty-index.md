# Empty Test Index for AgentScribe

## Overview

AgentScribe provides a built-in helper function for creating empty Tantivy indices specifically designed for testing search behavior. This is useful when you need to:

- Test search operations without any indexed documents
- Verify index initialization and schema setup
- Test error handling for empty results
- Validate search behavior before indexing any sessions

## Index Location

### Temporary Test Index (Default)

The standard empty test index is created at:
```
<temp_dir>/.agentscribe/index/tantivy/
```

Where `<temp_dir>` is a temporary directory created by `tempfile::tempdir()` that is automatically cleaned up when dropped.

### Persistent Test Index

For tests that require a persistent index across multiple test runs or manual testing, use the repo's dedicated test index location:

```
/home/coding/AgentScribe/tests/test_index/.agentscribe/
```

**Purpose:** This location provides a persistent, empty Tantivy index that:
- Survives across test runs (unlike temporary directories created by `tempfile`)
- Lives under the repo's `tests/` directory for clear test ownership
- Maintains separation from production data (`~/.agentscribe/`)
- Follows the same directory structure as production for consistency
- Is clearly identifiable as test infrastructure by its location

**Standard directory structure:**
```
/home/coding/AgentScribe/tests/test_index/.agentscribe/
├── index/
│   └── tantivy/          # Tantivy search index (created by IndexManager)
├── sessions/             # Normalized session files (empty initially)
├── state/                # Scrape state tracking
└── plugins/              # Plugin definitions for testing
```

**When to use:**
- Manual testing and experimentation with search features
- Integration tests that need a stable index location
- Performance testing where index setup cost matters
- Development and debugging of search functionality
- Test fixture development where you want to inspect index state

**Setup:**
```bash
# The directory structure already exists in the repo:
# /home/coding/AgentScribe/tests/test_index/.agentscribe/

# Initialize the index (this creates the tantivy subdirectory):
cd /home/coding/AgentScribe
cargo test --test empty_index_test -- --nocapture

# Or manually create the structure:
mkdir -p tests/test_index/.agentscribe/{index,sessions,state,plugins}
```

**Git management:** The `tests/test_index/.agentscribe/` directory contains generated index data and should be added to `.gitignore` to prevent committing test index files:

```
# In .gitignore
tests/test_index/.agentscribe/
```

The directory structure itself is tracked by git, but the actual index files within it are not.

## Usage

### Basic Usage

```rust
use agentscribe::test_helpers::setup_empty_index;

// Create an empty index
let (temp_dir, index_manager) = setup_empty_index();

// The index is now ready for use
// - temp_dir.path() returns the temporary directory path
// - index_manager provides access to the index
```

### Verifying Index is Empty

```rust
use agentscribe::test_helpers::setup_empty_index;

let (_temp_dir, index_manager) = setup_empty_index();

// Get a reader and searcher
let reader = index_manager.index().reader().unwrap();
let searcher = reader.searcher();

// Verify no documents are indexed
assert_eq!(searcher.num_docs(), 0);
```

### Search Operations on Empty Index

```rust
use agentscribe::test_helpers::setup_empty_index;
use agentscribe::search;

let (_temp_dir, index_manager) = setup_empty_index();

// Create search options
let options = search::SearchOptions {
    query: "test query".to_string(),
    max_results: 10,
    ..Default::default()
};

// Search returns empty results (no error)
let results = search::execute_search(&index_manager, &options).unwrap();
assert!(results.is_empty());
```

## Test Coverage

The empty index functionality is tested in `/home/coding/AgentScribe/tests/empty_index_test.rs`:

- `test_empty_index_creation` - Verifies index directory is created
- `test_empty_index_has_zero_documents` - Confirms no documents exist
- `test_empty_index_is_searchable` - Validates search operations work
- `test_empty_index_supports_write_operations` - Tests write lifecycle
- `test_empty_index_persists_across_reopen` - Verifies index persistence
- `test_multiple_empty_indices_are_independent` - Confirms isolation
- `test_empty_index_path_is_documented` - Validates path structure

## Implementation Details

The `setup_empty_index()` helper function:

1. Creates a temporary directory with standard AgentScribe layout:
   - `.agentscribe/plugins/` - For plugin definitions
   - `.agentscribe/sessions/` - For normalized session files
   - `.agentscribe/index/tantivy/` - For the search index
   - `.agentscribe/state/` - For scrape state

2. Initializes `IndexManager::open()` which creates:
   - A Tantivy index with the standard AgentScribe schema
   - All required field definitions (content, session_id, timestamp, etc.)
   - Proper directory structure

3. Performs initialization:
   - Calls `begin_write()` and `finish()` to ensure proper initialization
   - Verifies zero documents are indexed
   - Returns both the `TempDir` and `IndexManager`

## Index Schema

The empty index includes all standard AgentScribe fields:

### Full-text searchable fields (indexed, not stored)
- `content` - Conversation content (not stored per ADR-2)

### Stored display fields
- `summary` - Session summary
- `solution_summary` - Extracted solution
- `code_content` - Code artifact content

### Faceted filtering fields
- `session_id` - Unique session identifier
- `source_agent` - Agent type (claude-code, aider, etc.)
- `project` - Project path
- `tags` - Searchable tags
- `outcome` - success/failure/abandoned/unknown
- `error_fingerprint` - Normalized error patterns
- `file_paths` - Files referenced in session
- `git_commits` - Associated commit hashes
- `doc_type` - "session" or "code_artifact"

### Code artifact fields
- `code_language` - Programming language
- `code_file_path` - File path
- `code_is_final` - Final version flag

### Analytics fields
- `model` - LLM model name
- `session_type` - debug/feature/refactor/etc.
- `parent_session_id` - Subagent parent reference

### Date and numeric fields
- `timestamp` - Session start time
- `turn_count` - Number of conversation turns

## Acceptance Criteria

✅ **Empty index is available and ready for testing**
- The `setup_empty_index()` function creates a fully initialized index
- All tests pass (14/14)

✅ **Index path/location is documented**
- Location: `<temp_dir>/.agentscribe/index/tantivy/`
- Documented in this file and in test_helpers.rs doc comments

✅ **Index is accessible for search operations**
- Search operations return empty results (not errors)
- Write operations (begin_write/finish) work correctly
- Schema is accessible for field queries

## Running Tests

```bash
# Run all empty index tests
cargo test --test empty_index_test

# Run a specific test
cargo test --test empty_index_test test_empty_index_creation

# Run with output
cargo test --test empty_index_test -- --nocapture

# Run tests in parallel
cargo test --test empty_index_test -- --test-threads=4
```

## Examples

See `/home/coding/AgentScribe/tests/empty_index_test.rs` for complete examples of:
- Creating and verifying empty indices
- Testing search behavior with no documents
- Validating write operations
- Testing index persistence

## Related Documentation

- `/home/coding/AgentScribe/tests/test_helpers.rs` - Helper function implementation
- `/home/coding/AgentScribe/src/index.rs` - IndexManager implementation
- `/home/coding/AgentScribe/docs/plan.md` - Architecture documentation

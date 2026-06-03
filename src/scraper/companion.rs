//! Companion index file support
//!
//! Some agents store session metadata in a separate companion file that maps
//! session IDs to metadata like thread_id, model, cwd, etc. This module handles
//! reading and caching these companion index files.

use crate::error::{AgentScribeError, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::{Arc, RwLock};

/// A cached companion index that maps session IDs to their metadata.
#[derive(Debug, Clone)]
pub struct CompanionIndex {
    /// Map of session_id -> metadata JSON object
    entries: HashMap<String, Value>,
}

impl CompanionIndex {
    /// Create an empty companion index.
    pub fn empty() -> Self {
        CompanionIndex {
            entries: HashMap::new(),
        }
    }

    /// Load a companion index from a JSONL file.
    ///
    /// Each line should be a JSON object with at least a "thread_id" or "session_id" field
    /// and optional metadata fields like "model", "cwd", etc.
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let file =
            File::open(path).map_err(|_e| AgentScribeError::FileNotFound(path.to_path_buf()))?;
        let reader = BufReader::new(file);
        let mut entries = HashMap::new();

        for (line_num, line_result) in reader.lines().enumerate() {
            let line_num = line_num + 1;
            let line = line_result.map_err(|e| AgentScribeError::Parse {
                file: path.display().to_string(),
                line: Some(line_num),
                message: format!("Read error: {}", e),
            })?;

            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let json: Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => {
                    // Skip invalid JSON lines silently
                    continue;
                }
            };

            // Extract session ID - try both "thread_id" and "session_id" fields
            let session_id = if let Some(id) = json.get("thread_id").and_then(|v| v.as_str()) {
                id.to_string()
            } else if let Some(id) = json.get("session_id").and_then(|v| v.as_str()) {
                id.to_string()
            } else {
                // Skip entries without a recognizable ID field
                continue;
            };

            entries.insert(session_id, json);
        }

        Ok(CompanionIndex { entries })
    }

    /// Get metadata for a session by ID.
    ///
    /// Returns None if the session ID is not found in the index.
    pub fn get(&self, session_id: &str) -> Option<&Value> {
        self.entries.get(session_id)
    }

    /// Check if the index contains a session.
    pub fn contains(&self, session_id: &str) -> bool {
        self.entries.contains_key(session_id)
    }

    /// Get the number of entries in the index.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the index is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A thread-safe cache for companion indices.
///
/// This allows multiple scraper threads to access the same companion index
/// without re-reading the file each time.
#[derive(Debug, Clone)]
pub struct CompanionCache {
    inner: Arc<RwLock<HashMap<String, CompanionIndex>>>,
}

impl CompanionCache {
    /// Create a new empty cache.
    pub fn new() -> Self {
        CompanionCache {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get or load a companion index for the given file path.
    ///
    /// If the index is already cached, returns the cached version.
    /// Otherwise, loads it from disk and caches it.
    pub fn get_or_load(&self, path: &Path) -> Result<CompanionIndex> {
        // First, try to read from cache with a read lock
        {
            let reader = self.inner.read().map_err(|e| {
                AgentScribeError::DataDir(format!("Failed to acquire read lock: {}", e))
            })?;
            if let Some(index) = reader.get(path.to_str().unwrap_or("")) {
                return Ok(index.clone());
            }
        }

        // Not in cache, need to load - upgrade to write lock
        let mut writer = self.inner.write().map_err(|e| {
            AgentScribeError::DataDir(format!("Failed to acquire write lock: {}", e))
        })?;

        // Double-check in case another thread loaded it while we were waiting
        let path_key = path.to_str().unwrap_or("");
        if let Some(index) = writer.get(path_key) {
            return Ok(index.clone());
        }

        // Load the index
        let index = CompanionIndex::load_from_file(path)?;
        writer.insert(path_key.to_string(), index.clone());
        Ok(index)
    }

    /// Remove a cached index (e.g., after the file is modified).
    pub fn invalidate(&self, path: &Path) {
        if let Ok(mut writer) = self.inner.write() {
            writer.remove(path.to_str().unwrap_or(""));
        }
    }

    /// Clear all cached indices.
    pub fn clear(&self) {
        if let Ok(mut writer) = self.inner.write() {
            writer.clear();
        }
    }
}

impl Default for CompanionCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_companion_index_load_from_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write test data
        let mut file = File::create(path).unwrap();
        writeln!(
            file,
            r#"{{"thread_id": "abc123", "model": "gpt-4", "cwd": "/home/user/project"}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"thread_id": "def456", "model": "gpt-3.5-turbo", "cwd": "/home/user/other"}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"thread_id": "ghi789", "model": "gpt-4", "cwd": "/home/user/test"}}"#
        )
        .unwrap();

        // Load the index
        let index = CompanionIndex::load_from_file(path).unwrap();

        assert_eq!(index.len(), 3);
        assert!(index.contains("abc123"));
        assert!(index.contains("def456"));
        assert!(index.contains("ghi789"));
        assert!(!index.contains("nonexistent"));

        // Check metadata for a session
        let metadata = index.get("abc123").unwrap();
        assert_eq!(metadata.get("thread_id").unwrap().as_str(), Some("abc123"));
        assert_eq!(metadata.get("model").unwrap().as_str(), Some("gpt-4"));
        assert_eq!(
            metadata.get("cwd").unwrap().as_str(),
            Some("/home/user/project")
        );
    }

    #[test]
    fn test_companion_index_empty_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write empty file
        File::create(path).unwrap();

        // Load the index
        let index = CompanionIndex::load_from_file(path).unwrap();

        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn test_companion_index_skips_invalid_lines() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write test data with some invalid lines
        let mut file = File::create(path).unwrap();
        writeln!(file, r#"{{"thread_id": "abc123", "model": "gpt-4"}}"#).unwrap();
        writeln!(file).unwrap(); // Empty line
        writeln!(file, "not json").unwrap(); // Invalid JSON
        writeln!(
            file,
            r#"{{"thread_id": "def456", "model": "gpt-3.5-turbo"}}"#
        )
        .unwrap();
        writeln!(file, r#"{{"no_id": "true"}}"#).unwrap(); // No thread_id or session_id

        // Load the index
        let index = CompanionIndex::load_from_file(path).unwrap();

        // Should only have the two valid entries
        assert_eq!(index.len(), 2);
        assert!(index.contains("abc123"));
        assert!(index.contains("def456"));
    }

    #[test]
    fn test_companion_index_supports_session_id_field() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write test data using session_id instead of thread_id
        let mut file = File::create(path).unwrap();
        writeln!(file, r#"{{"session_id": "xyz789", "model": "gpt-4"}}"#).unwrap();

        // Load the index
        let index = CompanionIndex::load_from_file(path).unwrap();

        assert_eq!(index.len(), 1);
        assert!(index.contains("xyz789"));
    }

    #[test]
    fn test_companion_cache() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write test data
        let mut file = File::create(path).unwrap();
        writeln!(file, r#"{{"thread_id": "abc123", "model": "gpt-4"}}"#).unwrap();

        // Create cache
        let cache = CompanionCache::new();

        // First call should load from disk
        let index1 = cache.get_or_load(path).unwrap();
        assert_eq!(index1.len(), 1);

        // Second call should use cache
        let index2 = cache.get_or_load(path).unwrap();
        assert_eq!(index2.len(), 1);

        // Invalidate and reload
        cache.invalidate(path);
        let index3 = cache.get_or_load(path).unwrap();
        assert_eq!(index3.len(), 1);
    }

    #[test]
    fn test_companion_cache_clear() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write test data
        let mut file = File::create(path).unwrap();
        writeln!(file, r#"{{"thread_id": "abc123", "model": "gpt-4"}}"#).unwrap();

        // Create cache and load
        let cache = CompanionCache::new();
        cache.get_or_load(path).unwrap();

        // Clear cache
        cache.clear();

        // Verify cache is empty by checking file is re-read
        // (This is implicit - the key observation is that clear doesn't panic)
        cache.clear();
    }
}

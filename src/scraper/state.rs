//! Scrape state tracking for incremental scraping
//!
//! This module provides a persistence abstraction layer that allows multiple
//! backend implementations (JSON file, SQLite, etc.) to coexist. The StateManager
//! uses an enum-based approach to support different backends, making it easy to
//! switch between backends or support migration.
//!
//! # Backends
//!
//! - **JsonFileStateStore**: Legacy JSON file backend (full rewrites on each update)
//! - **SqliteStateStore**: SQLite backend (O(1) incremental updates)
//!
//! # Migration
//!
//! The SQLite backend automatically migrates data from the legacy JSON file on
//! first load if the JSON file exists but the SQLite database doesn't.

use crate::error::Result;
use crate::event::{ScrapeState, SourceFileState};
use chrono::Utc;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::state_sqlite::SqliteStateManager;

/// Default lock timeout (30 seconds).
const DEFAULT_LOCK_TIMEOUT: Duration = Duration::from_secs(30);

/// Trait abstracting state persistence operations.
///
/// This trait allows multiple backend implementations (JSON file, SQLite, etc.)
/// to coexist and be used interchangeably through the StateManager API.
///
/// # Contract
///
/// All methods must return an error if the operation cannot be completed.
/// Errors are wrapped in `Result<T>` and propagated to callers.
///
/// # Thread Safety
///
/// All implementations must be thread-safe (`Send + Sync`) to support
/// concurrent access from multiple scraper workers.
///
/// # Backends
///
/// - **JsonFileStateStore**: Legacy JSON file backend (full rewrites on each update)
/// - **SqliteStateStore**: SQLite backend (O(1) incremental updates)
///
/// # Error Types
///
/// Methods can return these errors:
/// - `Io(std::io::Error)`: File system operations failed
/// - `Serde(serde_json::Error)`: JSON serialization/deserialization failed
/// - `State(String)`: State management errors (corruption, lock poisoned, etc.)
/// - `FileNotFound(PathBuf)`: Requested file does not exist
///
/// Note: This trait uses an enum-based approach instead of trait objects
/// to avoid dyn compatibility issues with generic methods like `update_file_state`.
pub trait StateStore: Send + Sync {
    /// Get state for a single file.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Absolute path to the source file
    ///
    /// # Returns
    ///
    /// - `Ok(Some(state))` - File has existing state
    /// - `Ok(None)` - File not yet tracked
    /// - `Err(State(_))` - Lock poisoned or backend error
    fn get_file_state(&self, file_path: &str) -> Result<Option<SourceFileState>>;

    /// Set the last byte offset for a file.
    ///
    /// Used for JSONL and other position-based formats where we can seek
    /// directly to the last read position.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Absolute path to the source file
    /// * `offset` - Byte offset to resume reading from
    ///
    /// # Errors
    ///
    /// - `Io(_)` - File system write failed (JSON backend only)
    /// - `State(_)` - Lock poisoned or database error
    fn set_offset(&self, file_path: &str, offset: u64) -> Result<()>;

    /// Set the last delimiter offset for a file (for markdown delimiter-based parsing).
    ///
    /// Used by Aider and other agents that use delimiter markers to separate
    /// sessions within a single file.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Absolute path to the source file
    /// * `offset` - Byte offset of the last seen delimiter
    ///
    /// # Errors
    ///
    /// - `Io(_)` - File system write failed (JSON backend only)
    /// - `State(_)` - Lock poisoned or database error
    fn set_delimiter_offset(&self, file_path: &str, offset: u64) -> Result<()>;

    /// Update the modification time for a file.
    ///
    /// Stores the last known modification time to detect file changes.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Absolute path to the source file
    /// * `modified` - Last modification timestamp (from file metadata)
    ///
    /// # Errors
    ///
    /// - `Io(_)` - File system write failed (JSON backend only)
    /// - `State(_)` - Lock poisoned or database error
    fn set_modified(&self, file_path: &str, modified: chrono::DateTime<Utc>) -> Result<()>;

    /// Add a session ID to a file's state.
    ///
    /// Tracks which sessions have been extracted from this file.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Absolute path to the source file
    /// * `session_id` - Session ID to add (deduplicated if already present)
    ///
    /// # Errors
    ///
    /// - `Io(_)` - File system write failed (JSON backend only)
    /// - `State(_)` - Lock poisoned or database error
    fn add_session(&self, file_path: &str, session_id: String) -> Result<()>;

    /// Remove a file from the state.
    ///
    /// Deletes all tracking state for a file. Used when files are deleted
    /// or after truncation detection.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Absolute path to the source file
    ///
    /// # Errors
    ///
    /// - `Io(_)` - File system write failed (JSON backend only)
    /// - `State(_)` - Lock poisoned or database error
    fn remove_file(&self, file_path: &str) -> Result<()>;

    /// Get all files for a specific plugin.
    ///
    /// Returns a list of file paths tracked by the given plugin.
    ///
    /// # Arguments
    ///
    /// * `plugin` - Plugin name (e.g., "claude-code", "aider")
    ///
    /// # Returns
    ///
    /// - `Ok(files)` - Vector of file paths (empty if none)
    /// - `Err(State(_))` - Lock poisoned or database query failed
    fn files_for_plugin(&self, plugin: &str) -> Result<Vec<String>>;

    /// Get all state (for backward compatibility).
    ///
    /// Returns a complete snapshot of the scrape state. Used by legacy
    /// code that needs the entire state at once.
    ///
    /// # Returns
    ///
    /// - `Ok(state)` - Complete scrape state
    /// - `Err(State(_))` - Lock poisoned or serialization failed
    fn get_all(&self) -> Result<ScrapeState>;

    /// Update state for a single file using a closure.
    ///
    /// This is the primary method for mutations, allowing the backend to
    /// handle the update in the most efficient way (e.g., single SQL UPDATE).
    ///
    /// # Arguments
    ///
    /// * `file_path` - Absolute path to the source file
    /// * `update` - Closure that mutates the file state (called once)
    ///
    /// # Errors
    ///
    /// - `Io(_)` - File system write failed (JSON backend only)
    /// - `State(_)` - Lock poisoned or database error
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use agentscribe::event::SourceFileState;
    /// # use agentscribe::scraper::state::StateStore;
    /// # fn example(store: impl StateStore) {
    /// store.update_file_state("/path/to/file.jsonl", |state| {
    ///     state.last_byte_offset = 1000;
    ///     state.session_ids.push("session-123".to_string());
    /// }).unwrap();
    /// # }
    /// ```
    fn update_file_state<F>(&self, file_path: &str, update: F) -> Result<()>
    where
        F: FnOnce(&mut SourceFileState);

    /// Check if a file needs re-scraping based on modification time.
    ///
    /// Compares the file's current modification time against the stored
    /// last_modified timestamp.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Absolute path to the source file
    /// * `plugin` - Plugin name (used for state lookup)
    ///
    /// # Returns
    ///
    /// - `Ok(true)` - File has been modified or not yet tracked
    /// - `Ok(false)` - File unchanged since last scrape
    /// - `Err(FileNotFound(_))` - File does not exist
    /// - `Err(State(_))` - Invalid timestamp or lock poisoned
    fn needs_rescrape(&self, file_path: &Path, plugin: &str) -> Result<bool>;

    /// Check for truncated files and remove their state.
    ///
    /// Detects files that have been truncated (current size < last_byte_offset)
    /// and removes their state to force a full rescan.
    ///
    /// # Returns
    ///
    /// - `Ok(truncated_files)` - List of file paths that were truncated and removed
    /// - `Err(State(_))` - Lock poisoned or database error
    ///
    /// # Side Effects
    ///
    /// Removes state entries for all truncated files found.
    fn detect_truncation(&self) -> Result<Vec<String>>;

    /// Save state (no-op for backends that auto-commit).
    ///
    /// For JSON backend, writes the in-memory state to disk.
    /// For SQLite, this is a no-op since updates are auto-committed.
    ///
    /// # Errors
    ///
    /// - `Io(_)` - File system write failed (JSON backend only)
    /// - `Serde(_)` - JSON serialization failed (JSON backend only)
    /// - `State(_)` - Lock poisoned (JSON backend only)
    fn save(&self) -> Result<()>;
}

/// Enum-based backend store to avoid dyn compatibility issues.
///
/// This enum wraps all possible backend implementations and delegates
/// to the appropriate implementation. This approach avoids the need
/// for trait objects while maintaining flexibility.
pub enum StateBackend {
    /// JSON file backend
    Json(JsonFileStateStore),
    /// SQLite backend
    Sqlite(SqliteStateStore),
}

impl StateStore for StateBackend {
    fn get_file_state(&self, file_path: &str) -> Result<Option<SourceFileState>> {
        match self {
            StateBackend::Json(store) => store.get_file_state(file_path),
            StateBackend::Sqlite(store) => store.get_file_state(file_path),
        }
    }

    fn update_file_state<F>(&self, file_path: &str, update: F) -> Result<()>
    where
        F: FnOnce(&mut SourceFileState),
    {
        match self {
            StateBackend::Json(store) => store.update_file_state(file_path, update),
            StateBackend::Sqlite(store) => store.update_file_state(file_path, update),
        }
    }

    fn set_offset(&self, file_path: &str, offset: u64) -> Result<()> {
        match self {
            StateBackend::Json(store) => store.set_offset(file_path, offset),
            StateBackend::Sqlite(store) => store.set_offset(file_path, offset),
        }
    }

    fn set_delimiter_offset(&self, file_path: &str, offset: u64) -> Result<()> {
        match self {
            StateBackend::Json(store) => store.set_delimiter_offset(file_path, offset),
            StateBackend::Sqlite(store) => store.set_delimiter_offset(file_path, offset),
        }
    }

    fn set_modified(&self, file_path: &str, modified: chrono::DateTime<Utc>) -> Result<()> {
        match self {
            StateBackend::Json(store) => store.set_modified(file_path, modified),
            StateBackend::Sqlite(store) => store.set_modified(file_path, modified),
        }
    }

    fn add_session(&self, file_path: &str, session_id: String) -> Result<()> {
        match self {
            StateBackend::Json(store) => store.add_session(file_path, session_id),
            StateBackend::Sqlite(store) => store.add_session(file_path, session_id),
        }
    }

    fn remove_file(&self, file_path: &str) -> Result<()> {
        match self {
            StateBackend::Json(store) => store.remove_file(file_path),
            StateBackend::Sqlite(store) => store.remove_file(file_path),
        }
    }

    fn files_for_plugin(&self, plugin: &str) -> Result<Vec<String>> {
        match self {
            StateBackend::Json(store) => store.files_for_plugin(plugin),
            StateBackend::Sqlite(store) => store.files_for_plugin(plugin),
        }
    }

    fn get_all(&self) -> Result<ScrapeState> {
        match self {
            StateBackend::Json(store) => store.get_all(),
            StateBackend::Sqlite(store) => store.get_all(),
        }
    }

    fn needs_rescrape(&self, file_path: &Path, plugin: &str) -> Result<bool> {
        match self {
            StateBackend::Json(store) => store.needs_rescrape(file_path, plugin),
            StateBackend::Sqlite(store) => store.needs_rescrape(file_path, plugin),
        }
    }

    fn detect_truncation(&self) -> Result<Vec<String>> {
        match self {
            StateBackend::Json(store) => store.detect_truncation(),
            StateBackend::Sqlite(store) => store.detect_truncation(),
        }
    }

    fn save(&self) -> Result<()> {
        match self {
            StateBackend::Json(store) => store.save(),
            StateBackend::Sqlite(store) => store.save(),
        }
    }
}

/// JSON file backend for state persistence.
///
/// This is the legacy backend that stores all state in a single JSON file.
/// Each update rewrites the entire file, which is O(n) where n is the number
/// of tracked files. This backend is kept for backward compatibility and
/// testing purposes.
pub struct JsonFileStateStore {
    /// Path to the JSON state file
    state_file: PathBuf,
    /// In-memory cache of the state (for performance)
    state_cache: Arc<Mutex<ScrapeState>>,
}

impl JsonFileStateStore {
    /// Create a new JSON file state store.
    ///
    /// If an existing state file is corrupted, it will be renamed to
    /// `*.corrupt-<timestamp>` and a new empty state will be created instead.
    /// This makes the state persistence crash-safe and self-healing per ADR-1.
    pub fn new(state_file: PathBuf) -> Result<Self> {
        // Load existing state or create new
        let state = if state_file.exists() {
            let file = File::open(&state_file)?;
            let reader = BufReader::new(file);
            match serde_json::from_reader::<_, ScrapeState>(reader) {
                Ok(state) => state,
                Err(e) => {
                    // Corrupted state file - rename for debugging and start fresh
                    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
                    let corrupt_path =
                        state_file.with_extension(format!("json.corrupt-{}", timestamp));

                    // Rename the corrupted file
                    std::fs::rename(&state_file, &corrupt_path).map_err(|e| {
                        crate::error::AgentScribeError::State(format!(
                            "Failed to rename corrupted state file: {}. Original error: {}",
                            corrupt_path.display(),
                            e
                        ))
                    })?;

                    // Log error with backup path
                    eprintln!(
                        "ERROR: Failed to parse state file at {}: {}. \
                         Corrupted file backed up to: {}. \
                         Starting with empty state.",
                        state_file.display(),
                        e,
                        corrupt_path.display()
                    );

                    // Return empty state for self-healing
                    ScrapeState::new()
                }
            }
        } else {
            ScrapeState::new()
        };

        Ok(JsonFileStateStore {
            state_file,
            state_cache: Arc::new(Mutex::new(state)),
        })
    }

    /// Save the in-memory state to disk.
    fn save_to_disk(&self) -> Result<()> {
        // Clone the state to avoid holding the lock during serialization
        let state = {
            let state_guard = self
                .state_cache
                .lock()
                .map_err(|_| crate::error::AgentScribeError::State("Lock poisoned".to_string()))?;
            state_guard.clone()
        };

        // Atomic write: write to temp file, then rename
        // Use both process ID and thread ID to ensure uniqueness even with concurrent saves
        let pid = std::process::id();
        let tid = std::thread::current().id();
        // Append temp suffix before the extension (e.g., scrape-state.json -> scrape-state.tmp-123.json)
        let temp_file = self.state_file.with_file_name(format!(
            "{}.tmp-{}-{:?}.{}",
            self.state_file.file_stem().unwrap().to_str().unwrap(),
            pid,
            tid,
            self.state_file.extension().unwrap().to_str().unwrap()
        ));
        {
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&temp_file)?;
            let writer = BufWriter::new(file);
            serde_json::to_writer_pretty(writer, &state)?;
        }

        // Atomic rename
        std::fs::rename(&temp_file, &self.state_file)?;

        Ok(())
    }
}

impl StateStore for JsonFileStateStore {
    fn get_file_state(&self, file_path: &str) -> Result<Option<SourceFileState>> {
        let state = self
            .state_cache
            .lock()
            .map_err(|_| crate::error::AgentScribeError::State("Lock poisoned".to_string()))?;
        Ok(state.sources.get(file_path).cloned())
    }

    fn update_file_state<F>(&self, file_path: &str, update: F) -> Result<()>
    where
        F: FnOnce(&mut SourceFileState),
    {
        let mut state = self
            .state_cache
            .lock()
            .map_err(|_| crate::error::AgentScribeError::State("Lock poisoned".to_string()))?;

        let file_state = state
            .sources
            .entry(file_path.to_string())
            .or_insert_with(|| SourceFileState::new("unknown".to_string()));

        update(file_state);
        file_state.last_scraped = Utc::now();

        drop(state);
        self.save_to_disk()
    }

    fn set_offset(&self, file_path: &str, offset: u64) -> Result<()> {
        let mut state = self
            .state_cache
            .lock()
            .map_err(|_| crate::error::AgentScribeError::State("Lock poisoned".to_string()))?;

        let file_state = state
            .sources
            .entry(file_path.to_string())
            .or_insert_with(|| SourceFileState::new("unknown".to_string()));
        file_state.last_byte_offset = offset;
        file_state.last_scraped = Utc::now();

        drop(state);
        self.save_to_disk()
    }

    fn set_delimiter_offset(&self, file_path: &str, offset: u64) -> Result<()> {
        let mut state = self
            .state_cache
            .lock()
            .map_err(|_| crate::error::AgentScribeError::State("Lock poisoned".to_string()))?;

        let file_state = state
            .sources
            .entry(file_path.to_string())
            .or_insert_with(|| SourceFileState::new("unknown".to_string()));
        file_state.last_delimiter_offset = Some(offset);
        file_state.last_scraped = Utc::now();

        drop(state);
        self.save_to_disk()
    }

    fn set_modified(&self, file_path: &str, modified: chrono::DateTime<Utc>) -> Result<()> {
        let mut state = self
            .state_cache
            .lock()
            .map_err(|_| crate::error::AgentScribeError::State("Lock poisoned".to_string()))?;

        let file_state = state
            .sources
            .entry(file_path.to_string())
            .or_insert_with(|| SourceFileState::new("unknown".to_string()));
        file_state.last_modified = modified;
        file_state.last_scraped = Utc::now();

        drop(state);
        self.save_to_disk()
    }

    fn add_session(&self, file_path: &str, session_id: String) -> Result<()> {
        let mut state = self
            .state_cache
            .lock()
            .map_err(|_| crate::error::AgentScribeError::State("Lock poisoned".to_string()))?;

        let file_state = state
            .sources
            .entry(file_path.to_string())
            .or_insert_with(|| SourceFileState::new("unknown".to_string()));
        if !file_state.session_ids.contains(&session_id) {
            file_state.session_ids.push(session_id);
        }
        file_state.last_scraped = Utc::now();

        drop(state);
        self.save_to_disk()
    }

    fn remove_file(&self, file_path: &str) -> Result<()> {
        let mut state = self
            .state_cache
            .lock()
            .map_err(|_| crate::error::AgentScribeError::State("Lock poisoned".to_string()))?;

        state.sources.remove(file_path);

        drop(state);
        self.save_to_disk()
    }

    fn files_for_plugin(&self, plugin: &str) -> Result<Vec<String>> {
        let state = self
            .state_cache
            .lock()
            .map_err(|_| crate::error::AgentScribeError::State("Lock poisoned".to_string()))?;

        let files: Vec<String> = state
            .sources
            .iter()
            .filter(|(_, file_state)| file_state.plugin == plugin)
            .map(|(path, _)| path.clone())
            .collect();

        Ok(files)
    }

    fn get_all(&self) -> Result<ScrapeState> {
        let state = self
            .state_cache
            .lock()
            .map_err(|_| crate::error::AgentScribeError::State("Lock poisoned".to_string()))?;

        Ok(ScrapeState {
            sources: state.sources.clone(),
        })
    }

    fn needs_rescrape(&self, file_path: &Path, _plugin: &str) -> Result<bool> {
        let path_str = file_path
            .to_str()
            .ok_or_else(|| crate::error::AgentScribeError::FileNotFound(file_path.to_path_buf()))?;

        let metadata = std::fs::metadata(file_path)?;
        let system_time = metadata.modified()?;
        let duration = system_time
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| {
                crate::error::AgentScribeError::State("Invalid file modification time".to_string())
            })?;
        let modified =
            chrono::DateTime::from_timestamp(duration.as_secs() as i64, duration.subsec_nanos())
                .ok_or_else(|| {
                    crate::error::AgentScribeError::State("Invalid timestamp".to_string())
                })?;

        let state = self
            .state_cache
            .lock()
            .map_err(|_| crate::error::AgentScribeError::State("Lock poisoned".to_string()))?;

        if let Some(file_state) = state.sources.get(path_str) {
            if modified > file_state.last_modified {
                return Ok(metadata.len() < file_state.last_byte_offset || true);
            }
            return Ok(false);
        }

        Ok(true)
    }

    fn detect_truncation(&self) -> Result<Vec<String>> {
        let mut state = self
            .state_cache
            .lock()
            .map_err(|_| crate::error::AgentScribeError::State("Lock poisoned".to_string()))?;

        let mut truncated = Vec::new();

        for (path, file_state) in &state.sources {
            if let Ok(metadata) = std::fs::metadata(path) {
                if metadata.len() < file_state.last_byte_offset {
                    truncated.push(path.clone());
                }
            }
        }

        // Remove truncated files from state
        for path in &truncated {
            state.sources.remove(path);
        }

        drop(state);
        if !truncated.is_empty() {
            self.save_to_disk()?;
        }

        Ok(truncated)
    }

    fn save(&self) -> Result<()> {
        self.save_to_disk()
    }
}

/// SQLite backend wrapper for state persistence.
///
/// This wraps the existing SqliteStateManager to implement the StateStore trait.
/// It provides O(1) incremental updates and handles concurrent access via SQLite
/// built-in locking.
pub struct SqliteStateStore {
    /// Internal SQLite state manager
    inner: SqliteStateManager,
}

impl SqliteStateStore {
    /// Create a new SQLite state store.
    pub fn new(state_dir: &Path, busy_timeout: Duration) -> Result<Self> {
        let inner = SqliteStateManager::new(state_dir, busy_timeout)?;
        Ok(SqliteStateStore { inner })
    }
}

impl StateStore for SqliteStateStore {
    fn get_file_state(&self, file_path: &str) -> Result<Option<SourceFileState>> {
        self.inner.get_file_state(file_path)
    }

    fn update_file_state<F>(&self, file_path: &str, update: F) -> Result<()>
    where
        F: FnOnce(&mut SourceFileState),
    {
        self.inner.update_file_state(file_path, update)
    }

    fn set_offset(&self, file_path: &str, offset: u64) -> Result<()> {
        self.inner.set_offset(file_path, offset)
    }

    fn set_delimiter_offset(&self, file_path: &str, offset: u64) -> Result<()> {
        self.inner.set_delimiter_offset(file_path, offset)
    }

    fn set_modified(&self, file_path: &str, modified: chrono::DateTime<Utc>) -> Result<()> {
        self.inner.set_modified(file_path, modified)
    }

    fn add_session(&self, file_path: &str, session_id: String) -> Result<()> {
        self.inner.add_session(file_path, session_id)
    }

    fn remove_file(&self, file_path: &str) -> Result<()> {
        self.inner.remove_file(file_path)
    }

    fn files_for_plugin(&self, plugin: &str) -> Result<Vec<String>> {
        self.inner.files_for_plugin(plugin)
    }

    fn get_all(&self) -> Result<ScrapeState> {
        self.inner.get_all()
    }

    fn needs_rescrape(&self, file_path: &Path, plugin: &str) -> Result<bool> {
        self.inner.needs_rescrape(file_path, plugin)
    }

    fn detect_truncation(&self) -> Result<Vec<String>> {
        self.inner.detect_truncation()
    }

    fn save(&self) -> Result<()> {
        // SQLite auto-commits on every update, so this is a no-op
        Ok(())
    }
}

/// Scrape state manager (uses the StateBackend enum for backend flexibility)
pub struct StateManager {
    /// The backend store (could be JSON, SQLite, etc.)
    backend: StateBackend,
}

impl StateManager {
    /// Create a new state manager with SQLite backend (default).
    ///
    /// This uses SQLite as the backend for O(1) incremental updates.
    /// The state file path is used to determine the state directory.
    pub fn new(state_file: PathBuf) -> Result<Self> {
        Self::new_with_timeout(state_file, DEFAULT_LOCK_TIMEOUT)
    }

    /// Create a new state manager with a configurable lock timeout.
    ///
    /// Pass `Duration::ZERO` to disable the timeout (wait indefinitely).
    /// The timeout is used as the SQLite busy timeout (how long to wait
    /// when the database is locked by another process).
    pub fn new_with_timeout(state_file: PathBuf, lock_timeout: Duration) -> Result<Self> {
        // Extract the state directory from the state file path
        let state_dir = state_file
            .parent()
            .ok_or_else(|| {
                crate::error::AgentScribeError::State("Invalid state file path".to_string())
            })?
            .to_path_buf();

        // Create the SQLite state store
        let backend = StateBackend::Sqlite(SqliteStateStore::new(&state_dir, lock_timeout)?);

        Ok(StateManager { backend })
    }

    /// Create a new state manager with a specific backend.
    ///
    /// This allows using different backends (e.g., JSON file for testing).
    pub fn with_backend(backend: StateBackend) -> Self {
        StateManager { backend }
    }

    /// Save state (delegates to backend).
    ///
    /// For SQLite, this is a no-op since updates are auto-committed.
    /// For JSON, this writes the state to disk.
    pub fn save(&self) -> Result<()> {
        self.backend.save()
    }

    /// Get state for a file
    pub fn get_file_state(&self, file_path: &str) -> Option<SourceFileState> {
        self.backend.get_file_state(file_path).ok().flatten()
    }

    /// Get or create state for a file
    pub fn get_or_create_file_state(&self, file_path: &str, plugin: &str) -> SourceFileState {
        match self.backend.get_file_state(file_path) {
            Ok(Some(state)) => state,
            _ => SourceFileState::new(plugin.to_string()),
        }
    }

    /// Update state for a file after scraping
    pub fn update_file_state<F>(&self, file_path: &str, update: F) -> Result<()>
    where
        F: FnMut(&mut SourceFileState),
    {
        // Convert FnMut to FnOnce for the trait
        let mut closure = update;
        self.backend.update_file_state(file_path, |s| closure(s))
    }

    /// Set the last byte offset for a file
    pub fn set_offset(&self, file_path: &str, offset: u64) -> Result<()> {
        self.backend.set_offset(file_path, offset)
    }

    /// Set the last delimiter offset for a file (for markdown delimiter-based parsing)
    pub fn set_delimiter_offset(&self, file_path: &str, offset: u64) -> Result<()> {
        self.backend.set_delimiter_offset(file_path, offset)
    }

    /// Update the modification time for a file
    pub fn set_modified(&self, file_path: &str, modified: chrono::DateTime<Utc>) -> Result<()> {
        self.backend.set_modified(file_path, modified)
    }

    /// Add a session ID to a file's state
    pub fn add_session(&self, file_path: &str, session_id: String) -> Result<()> {
        self.backend.add_session(file_path, session_id)
    }

    /// Remove a file from the state
    pub fn remove_file(&self, file_path: &str) -> Result<()> {
        self.backend.remove_file(file_path)
    }

    /// Get all files for a plugin
    pub fn files_for_plugin(&self, plugin: &str) -> Vec<String> {
        self.backend.files_for_plugin(plugin).unwrap_or_default()
    }

    /// Get all state (clone)
    pub fn get_all(&self) -> ScrapeState {
        self.backend
            .get_all()
            .unwrap_or_else(|_| ScrapeState::new())
    }

    /// Check if a file needs re-scraping based on modification time
    pub fn needs_rescrape(&self, file_path: &Path, plugin: &str) -> Result<bool> {
        self.backend.needs_rescrape(file_path, plugin)
    }

    /// Check for truncated files and remove their state
    pub fn detect_truncation(&self) -> Result<Vec<String>> {
        self.backend.detect_truncation()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Test helper: create a test state manager with SQLite backend
    fn create_sqlite_manager(temp_dir: &Path) -> StateManager {
        let state_file = temp_dir.join("scrape-state.db");
        StateManager::new(state_file).unwrap()
    }

    /// Test helper: create a test state manager with JSON backend
    fn create_json_manager(temp_dir: &Path) -> StateManager {
        let state_file = temp_dir.join("scrape-state.json");
        let backend = StateBackend::Json(JsonFileStateStore::new(state_file).unwrap());
        StateManager::with_backend(backend)
    }

    #[test]
    fn test_sqlite_basic_operations() {
        let temp_dir = TempDir::new().unwrap();
        let manager = create_sqlite_manager(temp_dir.path());

        // Test update and retrieve
        manager
            .update_file_state("/test/file.jsonl", |state| {
                state.last_byte_offset = 1000;
                state.session_ids.push("test-session".to_string());
            })
            .unwrap();

        let file_state = manager.get_file_state("/test/file.jsonl").unwrap();
        assert_eq!(file_state.last_byte_offset, 1000);
        assert_eq!(file_state.session_ids.len(), 1);

        // Test set_offset
        manager.set_offset("/test/file.jsonl", 2000).unwrap();
        let file_state = manager.get_file_state("/test/file.jsonl").unwrap();
        assert_eq!(file_state.last_byte_offset, 2000);

        // Test add_session
        manager
            .add_session("/test/file.jsonl", "another-session".to_string())
            .unwrap();
        let file_state = manager.get_file_state("/test/file.jsonl").unwrap();
        assert_eq!(file_state.session_ids.len(), 2);

        // Test remove_file
        manager.remove_file("/test/file.jsonl").unwrap();
        assert!(manager.get_file_state("/test/file.jsonl").is_none());
    }

    #[test]
    fn test_json_basic_operations() {
        let temp_dir = TempDir::new().unwrap();
        let manager = create_json_manager(temp_dir.path());

        // Test update and retrieve
        manager
            .update_file_state("/test/file.jsonl", |state| {
                state.last_byte_offset = 1000;
                state.session_ids.push("test-session".to_string());
            })
            .unwrap();

        let file_state = manager.get_file_state("/test/file.jsonl").unwrap();
        assert_eq!(file_state.last_byte_offset, 1000);
        assert_eq!(file_state.session_ids.len(), 1);

        // Test set_offset
        manager.set_offset("/test/file.jsonl", 2000).unwrap();
        let file_state = manager.get_file_state("/test/file.jsonl").unwrap();
        assert_eq!(file_state.last_byte_offset, 2000);

        // Test add_session
        manager
            .add_session("/test/file.jsonl", "another-session".to_string())
            .unwrap();
        let file_state = manager.get_file_state("/test/file.jsonl").unwrap();
        assert_eq!(file_state.session_ids.len(), 2);

        // Test remove_file
        manager.remove_file("/test/file.jsonl").unwrap();
        assert!(manager.get_file_state("/test/file.jsonl").is_none());
    }

    #[test]
    fn test_sqlite_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let state_file = temp_dir.path().join("scrape-state.db");

        // Create and save state
        let manager = StateManager::new(state_file.clone()).unwrap();
        manager
            .update_file_state("/test/file.jsonl", |state| {
                state.last_byte_offset = 1000;
                state.session_ids.push("test-session".to_string());
            })
            .unwrap();
        manager.save().unwrap();

        // Load state in new manager
        let manager2 = StateManager::new(state_file).unwrap();
        let file_state = manager2.get_file_state("/test/file.jsonl").unwrap();

        assert_eq!(file_state.last_byte_offset, 1000);
        assert_eq!(file_state.session_ids.len(), 1);
    }

    #[test]
    fn test_json_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let state_file = temp_dir.path().join("scrape-state.json");

        // Create and save state
        let backend1 = StateBackend::Json(JsonFileStateStore::new(state_file.clone()).unwrap());
        let manager = StateManager::with_backend(backend1);
        manager
            .update_file_state("/test/file.jsonl", |state| {
                state.last_byte_offset = 1000;
                state.session_ids.push("test-session".to_string());
            })
            .unwrap();
        manager.save().unwrap();

        // Load state in new manager
        let backend2 = StateBackend::Json(JsonFileStateStore::new(state_file).unwrap());
        let manager2 = StateManager::with_backend(backend2);
        let file_state = manager2.get_file_state("/test/file.jsonl").unwrap();

        assert_eq!(file_state.last_byte_offset, 1000);
        assert_eq!(file_state.session_ids.len(), 1);
    }

    #[test]
    fn test_needs_rescrape() {
        let temp_dir = TempDir::new().unwrap();

        // Test SQLite backend
        let sqlite_manager = create_sqlite_manager(temp_dir.path());
        assert!(sqlite_manager
            .needs_rescrape(temp_dir.path(), "test")
            .unwrap());

        // Test JSON backend
        let json_manager = create_json_manager(temp_dir.path());
        assert!(json_manager
            .needs_rescrape(temp_dir.path(), "test")
            .unwrap());
    }

    #[test]
    fn test_sqlite_files_for_plugin() {
        let temp_dir = TempDir::new().unwrap();
        let manager = create_sqlite_manager(temp_dir.path());

        // Add files for different plugins
        manager
            .update_file_state("/test/file1.jsonl", |state| {
                state.plugin = "claude-code".to_string();
            })
            .unwrap();
        manager
            .update_file_state("/test/file2.jsonl", |state| {
                state.plugin = "aider".to_string();
            })
            .unwrap();
        manager
            .update_file_state("/test/file3.jsonl", |state| {
                state.plugin = "claude-code".to_string();
            })
            .unwrap();

        // Get files for plugin
        let claude_files = manager.files_for_plugin("claude-code");
        assert_eq!(claude_files.len(), 2);

        let aider_files = manager.files_for_plugin("aider");
        assert_eq!(aider_files.len(), 1);
    }

    #[test]
    fn test_json_files_for_plugin() {
        let temp_dir = TempDir::new().unwrap();
        let manager = create_json_manager(temp_dir.path());

        // Add files for different plugins
        manager
            .update_file_state("/test/file1.jsonl", |state| {
                state.plugin = "claude-code".to_string();
            })
            .unwrap();
        manager
            .update_file_state("/test/file2.jsonl", |state| {
                state.plugin = "aider".to_string();
            })
            .unwrap();
        manager
            .update_file_state("/test/file3.jsonl", |state| {
                state.plugin = "claude-code".to_string();
            })
            .unwrap();

        // Get files for plugin
        let claude_files = manager.files_for_plugin("claude-code");
        assert_eq!(claude_files.len(), 2);

        let aider_files = manager.files_for_plugin("aider");
        assert_eq!(aider_files.len(), 1);
    }

    #[test]
    fn test_set_delimiter_offset() {
        let temp_dir = TempDir::new().unwrap();

        // Test SQLite backend
        let sqlite_manager = create_sqlite_manager(temp_dir.path());
        sqlite_manager
            .set_delimiter_offset("/test/file.md", 5000)
            .unwrap();
        let file_state = sqlite_manager.get_file_state("/test/file.md").unwrap();
        assert_eq!(file_state.last_delimiter_offset, Some(5000));

        // Test JSON backend
        let json_manager = create_json_manager(temp_dir.path());
        json_manager
            .set_delimiter_offset("/test/file2.md", 6000)
            .unwrap();
        let file_state = json_manager.get_file_state("/test/file2.md").unwrap();
        assert_eq!(file_state.last_delimiter_offset, Some(6000));
    }

    #[test]
    fn test_set_modified() {
        let temp_dir = TempDir::new().unwrap();
        let manager = create_sqlite_manager(temp_dir.path());

        let modified_time = Utc::now();
        manager
            .set_modified("/test/file.jsonl", modified_time)
            .unwrap();

        let file_state = manager.get_file_state("/test/file.jsonl").unwrap();
        assert_eq!(file_state.last_modified, modified_time);
    }

    #[test]
    fn test_get_all() {
        let temp_dir = TempDir::new().unwrap();

        // Test SQLite backend
        let sqlite_manager = create_sqlite_manager(temp_dir.path());
        for i in 0..5 {
            sqlite_manager
                .update_file_state(&format!("/test/file{}.jsonl", i), |state| {
                    state.plugin = "claude-code".to_string();
                    state.last_byte_offset = i as u64 * 1000;
                })
                .unwrap();
        }

        let all_state = sqlite_manager.get_all();
        assert_eq!(all_state.sources.len(), 5);

        // Test JSON backend
        let json_manager = create_json_manager(temp_dir.path());
        for i in 0..3 {
            json_manager
                .update_file_state(&format!("/test/jsonfile{}.jsonl", i), |state| {
                    state.plugin = "aider".to_string();
                })
                .unwrap();
        }

        let all_state = json_manager.get_all();
        assert_eq!(all_state.sources.len(), 3);
    }

    /// Two concurrent saves must not corrupt the state file.
    ///
    /// Both managers write different offsets for the same key. After both
    /// complete, the state must be valid (both SQLite and JSON handle this).
    #[test]
    fn test_concurrent_saves_no_corruption() {
        let temp_dir = TempDir::new().unwrap();

        // Test SQLite backend
        let state_file = temp_dir.path().join("scrape-state.db");
        let m1 = Arc::new(
            StateManager::new_with_timeout(state_file.clone(), Duration::from_secs(10)).unwrap(),
        );
        let m2 = Arc::new(
            StateManager::new_with_timeout(state_file.clone(), Duration::from_secs(10)).unwrap(),
        );

        m1.update_file_state("/test/file.jsonl", |s| s.last_byte_offset = 111)
            .unwrap();
        m2.update_file_state("/test/file.jsonl", |s| s.last_byte_offset = 222)
            .unwrap();

        let m1c = m1.clone();
        let m2c = m2.clone();

        let t1 = std::thread::spawn(move || m1c.save().unwrap());
        let t2 = std::thread::spawn(move || m2c.save().unwrap());

        t1.join().unwrap();
        t2.join().unwrap();

        // The state must be valid and readable
        let manager3 = StateManager::new(state_file).unwrap();
        let file_state = manager3.get_file_state("/test/file.jsonl");
        assert!(file_state.is_some());
        // One of the writes should have won
        assert!(matches!(file_state.unwrap().last_byte_offset, 111 | 222));

        // Test JSON backend (atomic writes prevent corruption)
        // Note: JSON backend doesn't support true concurrent access since each
        // store has its own in-memory cache. We test that atomic writes
        // produce valid state files even with concurrent filesystem access.
        let json_file = temp_dir.path().join("scrape-state.json");

        // First manager creates and saves initial state
        let backend1 = StateBackend::Json(JsonFileStateStore::new(json_file.clone()).unwrap());
        let jm1 = Arc::new(StateManager::with_backend(backend1));
        jm1.update_file_state("/test/jsonfile.jsonl", |s| {
            s.last_byte_offset = 100;
            s.plugin = "test-plugin".to_string();
        })
        .unwrap();
        jm1.save().unwrap();

        // Second manager loads the state and modifies it
        let backend2 = StateBackend::Json(JsonFileStateStore::new(json_file.clone()).unwrap());
        let jm2 = Arc::new(StateManager::with_backend(backend2));
        jm2.update_file_state("/test/jsonfile.jsonl", |s| {
            s.last_byte_offset = 200;
        })
        .unwrap();

        let jm1c = jm1.clone();
        let jm2c = jm2.clone();

        // Both try to save concurrently - one will win via atomic rename
        let jt1 = std::thread::spawn(move || {
            jm1c.update_file_state("/test/jsonfile.jsonl", |s| {
                s.last_byte_offset = 333;
            })
            .unwrap();
            jm1c.save().unwrap()
        });

        let jt2 = std::thread::spawn(move || {
            jm2c.update_file_state("/test/jsonfile.jsonl", |s| {
                s.last_byte_offset = 444;
            })
            .unwrap();
            jm2c.save().unwrap()
        });

        jt1.join().unwrap();
        jt2.join().unwrap();

        // The state must be valid and readable (no corruption)
        let backend3 = StateBackend::Json(JsonFileStateStore::new(json_file).unwrap());
        let jmanager3 = StateManager::with_backend(backend3);
        let file_state = jmanager3.get_file_state("/test/jsonfile.jsonl");
        assert!(file_state.is_some());
        // One of the writes should have won (333 or 444)
        assert!(matches!(file_state.unwrap().last_byte_offset, 333 | 444));
    }

    /// Test trait contract compliance: verify both backends implement the trait correctly
    #[test]
    fn test_trait_contract_compliance() {
        let temp_dir = TempDir::new().unwrap();

        // Test that both backends implement the required trait methods
        let sqlite_manager = create_sqlite_manager(temp_dir.path());
        let json_manager = create_json_manager(temp_dir.path());

        // Both should be able to perform all trait operations
        let test_file = "/test/trait-test.jsonl";

        // Test get_file_state (initially None)
        assert!(sqlite_manager.get_file_state(test_file).is_none());
        assert!(json_manager.get_file_state(test_file).is_none());

        // Test update_file_state
        sqlite_manager
            .update_file_state(test_file, |s| s.last_byte_offset = 123)
            .unwrap();
        json_manager
            .update_file_state(test_file, |s| s.last_byte_offset = 456)
            .unwrap();

        // Test set_offset
        sqlite_manager.set_offset(test_file, 789).unwrap();
        json_manager.set_offset(test_file, 101).unwrap();

        // Test set_delimiter_offset
        sqlite_manager.set_delimiter_offset(test_file, 200).unwrap();
        json_manager.set_delimiter_offset(test_file, 300).unwrap();

        // Test add_session
        sqlite_manager
            .add_session(test_file, "session-1".to_string())
            .unwrap();
        json_manager
            .add_session(test_file, "session-2".to_string())
            .unwrap();

        // Test get_all
        let sqlite_all = sqlite_manager.get_all();
        let json_all = json_manager.get_all();
        assert!(sqlite_all.sources.contains_key(test_file));
        assert!(json_all.sources.contains_key(test_file));

        // Test files_for_plugin
        let sqlite_files = sqlite_manager.files_for_plugin("unknown");
        let json_files = json_manager.files_for_plugin("unknown");
        assert_eq!(sqlite_files.len(), 1);
        assert_eq!(json_files.len(), 1);

        // Test remove_file
        sqlite_manager.remove_file(test_file).unwrap();
        json_manager.remove_file(test_file).unwrap();

        // Verify removal
        assert!(sqlite_manager.get_file_state(test_file).is_none());
        assert!(json_manager.get_file_state(test_file).is_none());
    }

    /// Verify that StateStore is NOT object-safe due to generic methods.
    ///
    /// This test demonstrates why StateStore cannot be used as `dyn StateStore`:
    /// - The trait has generic methods like `update_file_state<F>` which prevent
    ///   object safety (trait objects require all methods to be non-generic)
    /// - Attempting to use StateStore as a trait object will fail to compile
    ///
    /// The solution is the enum-based approach (StateBackend) that wraps all
    /// backend implementations and dispatches to concrete types.
    ///
    /// # Object Safety Rules
    ///
    /// A trait is object-safe only if:
    /// - No methods are generic (no type parameters)
    /// - No methods return `Self`
    /// - No methods have `where Self: Sized` bounds
    ///
    /// StateStore violates the first rule due to `update_file_state<F>`,
    /// which is necessary for ergonomic mutation via closures.
    #[test]
    fn test_statestore_object_safety_limitation() {
        // This test documents why StateStore cannot be used as dyn StateStore
        // The following code will NOT compile:
        //
        // let store: Box<dyn StateStore> = Box::new(JsonFileStateStore::new(...).unwrap());
        //
        // Error: the trait `StateStore` cannot be made into an object
        // Reason: method `update_file_state` has generic signature

        // Instead, we use the StateBackend enum which wraps all implementations
        let _temp_dir = TempDir::new().unwrap();
        let temp_dir = TempDir::new().unwrap();

        // Create both backends
        let json_file = temp_dir.path().join("scrape-state.json");
        let json_backend = StateBackend::Json(JsonFileStateStore::new(json_file).unwrap());

        let sqlite_backend = StateBackend::Sqlite(
            SqliteStateStore::new(temp_dir.path(), DEFAULT_LOCK_TIMEOUT).unwrap(),
        );

        // Test that both can be used through the enum
        let test_file = "/test/object-safety.jsonl";

        // Both backends work through the StateBackend enum
        for backend in &[json_backend, sqlite_backend] {
            backend
                .update_file_state(test_file, |s| {
                    s.last_byte_offset = 1000;
                    s.plugin = "test-plugin".to_string();
                })
                .unwrap();

            let state = backend.get_file_state(test_file).unwrap().unwrap();
            assert_eq!(state.last_byte_offset, 1000);
            assert_eq!(state.plugin, "test-plugin");
        }

        // Verify we can store backend in a collection (dyn StateStore wouldn't work)
        let backends: Vec<StateBackend> = vec![
            StateBackend::Json(
                JsonFileStateStore::new(temp_dir.path().join("state1.json")).unwrap(),
            ),
            StateBackend::Sqlite(
                SqliteStateStore::new(temp_dir.path(), DEFAULT_LOCK_TIMEOUT).unwrap(),
            ),
        ];

        // All backends in the collection implement the trait
        for backend in backends {
            backend.set_offset("/test/file1.jsonl", 500).unwrap();
            backend.set_offset("/test/file2.jsonl", 600).unwrap();
        }
    }
}

//! SQLite-based scrape state tracking for O(1) incremental updates.
//!
//! This module replaces the JSON-file-based StateManager with a SQLite-backed
//! implementation that provides O(1) incremental updates instead of O(n) full
//! rewrites. Each source file's state is stored as a separate row, allowing
//! updates to touch only the file being modified rather than the entire corpus.
//!
//! # Migration from JSON
//!
//! On first load, if the legacy JSON state file exists but the SQLite database
//! doesn't, data is automatically imported from JSON. This is a one-time migration.
//!
//! # Schema
//!
//! ```sql
//! CREATE TABLE file_state (
//!     file_path TEXT PRIMARY KEY,
//!     plugin TEXT NOT NULL,
//!     last_byte_offset INTEGER NOT NULL DEFAULT 0,
//!     last_modified TEXT NOT NULL,
//!     last_scraped TEXT NOT NULL,
//!     session_ids TEXT NOT NULL,
//!     last_delimiter_offset INTEGER
//! )
//! ```
//!
//! # Concurrency
//!
//! SQLite handles concurrent access via built-in locking. Multiple readers can
//! proceed simultaneously, while writers are serialized. This is sufficient for
//! AgentScribe's usage pattern (infrequent writes, frequent reads).

use crate::error::Result;
use crate::event::{ScrapeState, SourceFileState};
use chrono::Utc;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// SQLite database filename
const DB_FILENAME: &str = "scrape-state.db";

/// Schema version for future migrations
const SCHEMA_VERSION: i64 = 1;

/// SQLite-based state manager with O(1) incremental updates
pub struct SqliteStateManager {
    /// Path to the SQLite database
    db_path: PathBuf,
    /// Maximum time to wait for database lock (for busy timeout)
    busy_timeout: Duration,
}

impl SqliteStateManager {
    /// Create a new SQLite state manager.
    ///
    /// Automatically initializes the database and migrates data from the legacy
    /// JSON file if it exists.
    pub fn new(state_dir: &Path, busy_timeout: Duration) -> Result<Self> {
        let db_path = state_dir.join(DB_FILENAME);

        // Ensure state directory exists
        if !state_dir.exists() {
            std::fs::create_dir_all(state_dir)?;
        }

        let mut manager = SqliteStateManager {
            db_path,
            busy_timeout,
        };

        manager.initialize()?;

        Ok(manager)
    }

    /// Get the path to the legacy JSON state file
    fn legacy_json_path(&self) -> PathBuf {
        let mut json_path = self.db_path.clone();
        json_path.set_extension("json");
        json_path
    }

    /// Initialize the database: create schema, set pragmas, migrate from JSON
    fn initialize(&mut self) -> Result<()> {
        let first_init = !self.db_path.exists();

        let mut conn = self.open_connection()?;

        if first_init {
            // Create schema
            conn.execute(
                "CREATE TABLE file_state (
                    file_path TEXT PRIMARY KEY,
                    plugin TEXT NOT NULL,
                    last_byte_offset INTEGER NOT NULL DEFAULT 0,
                    last_modified TEXT NOT NULL,
                    last_scraped TEXT NOT NULL,
                    session_ids TEXT NOT NULL DEFAULT '[]',
                    last_delimiter_offset INTEGER
                )",
                [],
            )?;

            // Create index for plugin-based queries
            conn.execute("CREATE INDEX idx_plugin ON file_state(plugin)", [])?;

            // Store schema version
            conn.execute(
                "CREATE TABLE schema_version (version INTEGER PRIMARY KEY)",
                [],
            )?;
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![SCHEMA_VERSION],
            )?;

            // Migrate from JSON if it exists
            let legacy_json = self.legacy_json_path();
            if legacy_json.exists() {
                self.migrate_from_json(&mut conn, &legacy_json)?;
                // Backup the migrated JSON file
                let backup_path = legacy_json.with_extension("json.migrated");
                std::fs::rename(&legacy_json, &backup_path)?;
                tracing::info!(
                    legacy = %legacy_json.display(),
                    backup = %backup_path.display(),
                    "Migrated scrape state from JSON to SQLite"
                );
            }
        } else {
            // Verify schema version matches
            let version: i64 =
                conn.query_row("SELECT version FROM schema_version", [], |row| row.get(0))?;

            if version != SCHEMA_VERSION {
                tracing::warn!(
                    current = version,
                    expected = SCHEMA_VERSION,
                    "Schema version mismatch - may need migration"
                );
            }
        }

        Ok(())
    }

    /// Open a connection with appropriate pragmas and busy timeout
    fn open_connection(&self) -> Result<Connection> {
        let conn = Connection::open(&self.db_path)?;

        // Set busy timeout (how long to wait when DB is locked)
        conn.busy_timeout(self.busy_timeout)?;

        // Use WAL mode for better concurrency (query returns "wal" as result)
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;

        // Optimize for our workload (infrequent writes, frequent reads)
        conn.execute("PRAGMA synchronous = NORMAL", [])?;

        // Small cache (we don't need large cache for our workload)
        conn.execute("PRAGMA cache_size = -2000", [])?; // ~2MB

        Ok(conn)
    }

    /// Migrate data from legacy JSON state file
    fn migrate_from_json(&self, conn: &mut Connection, json_path: &Path) -> Result<()> {
        tracing::info!(
            path = %json_path.display(),
            "Migrating scrape state from JSON to SQLite"
        );

        let json_content = std::fs::read_to_string(json_path)?;
        let scrape_state: ScrapeState = serde_json::from_str(&json_content)?;

        let tx = conn.transaction()?;

        for (file_path, file_state) in &scrape_state.sources {
            let session_ids_json = serde_json::to_string(&file_state.session_ids)?;
            let last_modified = file_state.last_modified.to_rfc3339();
            let last_scraped = file_state.last_scraped.to_rfc3339();

            tx.execute(
                "INSERT INTO file_state (
                    file_path, plugin, last_byte_offset, last_modified,
                    last_scraped, session_ids, last_delimiter_offset
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    file_path,
                    &file_state.plugin,
                    file_state.last_byte_offset,
                    last_modified,
                    last_scraped,
                    session_ids_json,
                    file_state.last_delimiter_offset,
                ],
            )?;
        }

        tx.commit()?;

        tracing::info!(
            count = scrape_state.sources.len(),
            "Migrated {} source file states from JSON",
            scrape_state.sources.len()
        );

        Ok(())
    }

    /// Get state for a single file
    pub fn get_file_state(&self, file_path: &str) -> Result<Option<SourceFileState>> {
        let conn = self.open_connection()?;

        let mut stmt = conn.prepare(
            "SELECT plugin, last_byte_offset, last_modified, last_scraped,
                    session_ids, last_delimiter_offset
             FROM file_state WHERE file_path = ?1",
        )?;

        let result = stmt.query_row(params![file_path], |row| {
            let plugin: String = row.get(0)?;
            let last_byte_offset: u64 = row.get(1)?;
            let last_modified: String = row.get(2)?;
            let last_scraped: String = row.get(3)?;
            let session_ids_json: String = row.get(4)?;
            let last_delimiter_offset: Option<u64> = row.get(5)?;

            let last_modified = chrono::DateTime::parse_from_rfc3339(&last_modified)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
                .with_timezone(&Utc);
            let last_scraped = chrono::DateTime::parse_from_rfc3339(&last_scraped)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
                .with_timezone(&Utc);
            let session_ids: Vec<String> = serde_json::from_str(&session_ids_json)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

            Ok(SourceFileState {
                plugin,
                last_byte_offset,
                last_modified,
                last_scraped,
                session_ids,
                last_delimiter_offset,
            })
        });

        match result {
            Ok(state) => Ok(Some(state)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Update state for a single file (O(1) operation)
    pub fn update_file_state<F>(&self, file_path: &str, update: F) -> Result<()>
    where
        F: FnOnce(&mut SourceFileState),
    {
        let mut file_state = match self.get_file_state(file_path)? {
            Some(state) => state,
            None => {
                // Try to infer plugin from existing data or default
                SourceFileState::new("unknown".to_string())
            }
        };

        // Apply the update function
        update(&mut file_state);
        file_state.last_scraped = Utc::now();

        // Serialize to SQLite
        let conn = self.open_connection()?;
        let session_ids_json = serde_json::to_string(&file_state.session_ids)?;
        let last_modified = file_state.last_modified.to_rfc3339();
        let last_scraped = file_state.last_scraped.to_rfc3339();

        conn.execute(
            "INSERT INTO file_state (
                file_path, plugin, last_byte_offset, last_modified,
                last_scraped, session_ids, last_delimiter_offset
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(file_path) DO UPDATE SET
                plugin = excluded.plugin,
                last_byte_offset = excluded.last_byte_offset,
                last_modified = excluded.last_modified,
                last_scraped = excluded.last_scraped,
                session_ids = excluded.session_ids,
                last_delimiter_offset = excluded.last_delimiter_offset",
            params![
                file_path,
                &file_state.plugin,
                file_state.last_byte_offset,
                last_modified,
                last_scraped,
                session_ids_json,
                file_state.last_delimiter_offset,
            ],
        )?;

        Ok(())
    }

    /// Set the last byte offset for a file
    pub fn set_offset(&self, file_path: &str, offset: u64) -> Result<()> {
        self.update_file_state(file_path, |state| {
            state.last_byte_offset = offset;
        })
    }

    /// Set the last delimiter offset for a file
    pub fn set_delimiter_offset(&self, file_path: &str, offset: u64) -> Result<()> {
        self.update_file_state(file_path, |state| {
            state.last_delimiter_offset = Some(offset);
        })
    }

    /// Update the modification time for a file
    pub fn set_modified(&self, file_path: &str, modified: chrono::DateTime<Utc>) -> Result<()> {
        self.update_file_state(file_path, |state| {
            state.last_modified = modified;
        })
    }

    /// Add a session ID to a file's state
    pub fn add_session(&self, file_path: &str, session_id: String) -> Result<()> {
        self.update_file_state(file_path, |state| {
            if !state.session_ids.contains(&session_id) {
                state.session_ids.push(session_id);
            }
        })
    }

    /// Remove a file from the state
    pub fn remove_file(&self, file_path: &str) -> Result<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "DELETE FROM file_state WHERE file_path = ?1",
            params![file_path],
        )?;
        Ok(())
    }

    /// Get all files for a plugin
    pub fn files_for_plugin(&self, plugin: &str) -> Result<Vec<String>> {
        let conn = self.open_connection()?;

        let mut stmt = conn.prepare("SELECT file_path FROM file_state WHERE plugin = ?1")?;

        let files = stmt
            .query_map(params![plugin], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(files)
    }

    /// Get all state (for backward compatibility)
    pub fn get_all(&self) -> Result<ScrapeState> {
        let conn = self.open_connection()?;

        let mut stmt = conn.prepare(
            "SELECT file_path, plugin, last_byte_offset, last_modified,
                    last_scraped, session_ids, last_delimiter_offset
             FROM file_state",
        )?;

        let sources = stmt
            .query_map([], |row| {
                let file_path: String = row.get(0)?;
                let plugin: String = row.get(1)?;
                let last_byte_offset: u64 = row.get(2)?;
                let last_modified: String = row.get(3)?;
                let last_scraped: String = row.get(4)?;
                let session_ids_json: String = row.get(5)?;
                let last_delimiter_offset: Option<u64> = row.get(6)?;

                let last_modified = chrono::DateTime::parse_from_rfc3339(&last_modified)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
                    .with_timezone(&Utc);
                let last_scraped = chrono::DateTime::parse_from_rfc3339(&last_scraped)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
                    .with_timezone(&Utc);
                let session_ids: Vec<String> = serde_json::from_str(&session_ids_json)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

                Ok((
                    file_path,
                    SourceFileState {
                        plugin,
                        last_byte_offset,
                        last_modified,
                        last_scraped,
                        session_ids,
                        last_delimiter_offset,
                    },
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .collect();

        Ok(ScrapeState { sources })
    }

    /// Check if a file needs re-scraping based on modification time
    pub fn needs_rescrape(&self, file_path: &Path, _plugin: &str) -> Result<bool> {
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

        if let Some(file_state) = self.get_file_state(path_str)? {
            if modified > file_state.last_modified {
                if metadata.len() < file_state.last_byte_offset {
                    return Ok(true);
                }
                return Ok(true);
            }
            return Ok(false);
        }

        Ok(true)
    }

    /// Check for truncated files and remove their state
    pub fn detect_truncation(&self) -> Result<Vec<String>> {
        let conn = self.open_connection()?;
        let mut truncated = Vec::new();

        let mut stmt = conn.prepare("SELECT file_path, last_byte_offset FROM file_state")?;

        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
        })?;

        for row_result in rows.flatten() {
            let (path, last_byte_offset) = row_result;
            if let Ok(metadata) = std::fs::metadata(&path) {
                if metadata.len() < last_byte_offset {
                    truncated.push(path.clone());
                }
            }
        }

        // Remove truncated files from state
        for path in &truncated {
            self.remove_file(path)?;
        }

        Ok(truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_sqlite_basic_operations() {
        let temp_dir = TempDir::new().unwrap();
        let manager = SqliteStateManager::new(temp_dir.path(), Duration::from_secs(30)).unwrap();

        // Update file state
        manager
            .update_file_state("/test/file.jsonl", |state| {
                state.plugin = "claude-code".to_string();
                state.last_byte_offset = 1000;
                state.session_ids.push("test-session".to_string());
            })
            .unwrap();

        // Retrieve file state
        let file_state = manager.get_file_state("/test/file.jsonl").unwrap();
        assert!(file_state.is_some());
        let file_state = file_state.unwrap();
        assert_eq!(file_state.plugin, "claude-code");
        assert_eq!(file_state.last_byte_offset, 1000);
        assert_eq!(file_state.session_ids.len(), 1);

        // Set offset
        manager.set_offset("/test/file.jsonl", 2000).unwrap();
        let file_state = manager.get_file_state("/test/file.jsonl").unwrap().unwrap();
        assert_eq!(file_state.last_byte_offset, 2000);

        // Add session
        manager
            .add_session("/test/file.jsonl", "another-session".to_string())
            .unwrap();
        let file_state = manager.get_file_state("/test/file.jsonl").unwrap().unwrap();
        assert_eq!(file_state.session_ids.len(), 2);

        // Remove file
        manager.remove_file("/test/file.jsonl").unwrap();
        let file_state = manager.get_file_state("/test/file.jsonl").unwrap();
        assert!(file_state.is_none());
    }

    #[test]
    fn test_migration_from_json() {
        let temp_dir = TempDir::new().unwrap();

        // Create legacy JSON state file
        let json_path = temp_dir.path().join("scrape-state.json");
        let mut scrape_state = ScrapeState::new();
        scrape_state.sources.insert(
            "/test/file1.jsonl".to_string(),
            SourceFileState::new("claude-code".to_string()),
        );
        scrape_state.sources.insert(
            "/test/file2.jsonl".to_string(),
            SourceFileState::new("aider".to_string()),
        );
        std::fs::write(&json_path, serde_json::to_string(&scrape_state).unwrap()).unwrap();

        // Create SQLite manager (should migrate from JSON)
        let manager = SqliteStateManager::new(temp_dir.path(), Duration::from_secs(30)).unwrap();

        // Verify migration
        assert!(manager
            .get_file_state("/test/file1.jsonl")
            .unwrap()
            .is_some());
        assert!(manager
            .get_file_state("/test/file2.jsonl")
            .unwrap()
            .is_some());

        // Verify JSON was backed up
        assert!(json_path.with_extension("json.migrated").exists());
        assert!(!json_path.exists()); // Original was moved
    }

    #[test]
    fn test_get_all() {
        let temp_dir = TempDir::new().unwrap();
        let manager = SqliteStateManager::new(temp_dir.path(), Duration::from_secs(30)).unwrap();

        // Add multiple files
        for i in 0..5 {
            manager
                .update_file_state(&format!("/test/file{}.jsonl", i), |state| {
                    state.plugin = "claude-code".to_string();
                    state.last_byte_offset = i as u64 * 1000;
                })
                .unwrap();
        }

        // Get all state
        let all_state = manager.get_all().unwrap();
        assert_eq!(all_state.sources.len(), 5);
    }

    #[test]
    fn test_needs_rescrape() {
        let temp_dir = TempDir::new().unwrap();
        let manager = SqliteStateManager::new(temp_dir.path(), Duration::from_secs(30)).unwrap();

        // Create a temporary file
        let test_file = temp_dir.path().join("test.jsonl");
        std::fs::write(&test_file, "test").unwrap();

        // New file should need scraping
        assert!(manager.needs_rescrape(&test_file, "test").unwrap());
    }

    #[test]
    fn test_files_for_plugin() {
        let temp_dir = TempDir::new().unwrap();
        let manager = SqliteStateManager::new(temp_dir.path(), Duration::from_secs(30)).unwrap();

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
        let claude_files = manager.files_for_plugin("claude-code").unwrap();
        assert_eq!(claude_files.len(), 2);

        let aider_files = manager.files_for_plugin("aider").unwrap();
        assert_eq!(aider_files.len(), 1);
    }
}

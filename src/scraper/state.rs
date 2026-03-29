//! Scrape state tracking for incremental scraping
//!
//! Tracks position per source file for incremental scrapes.

use crate::event::{ScrapeState, SourceFileState};
use crate::error::Result;
use fs2::FileExt;
use serde_json;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use chrono::Utc;

/// Default lock timeout (30 seconds).
const DEFAULT_LOCK_TIMEOUT: Duration = Duration::from_secs(30);

/// Poll interval when waiting for the exclusive file lock.
const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Acquire an exclusive file lock, retrying until `timeout` elapses.
///
/// Uses `try_lock_exclusive()` (non-blocking attempt) in a loop so the caller
/// never blocks indefinitely.  Returns an error if the lock cannot be obtained
/// within the allotted time.
///
/// When `timeout` is `Duration::ZERO` the function falls back to the blocking
/// `lock_exclusive()` call, effectively disabling the timeout.
fn lock_exclusive_with_timeout(file: &File, timeout: Duration) -> Result<()> {
    if timeout.is_zero() {
        // Blocking mode: wait indefinitely (legacy / opt-out behaviour)
        file.lock_exclusive()?;
        return Ok(());
    }

    let start = Instant::now();
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if start.elapsed() >= timeout {
                    return Err(crate::error::AgentScribeError::State(format!(
                        "timed out waiting for exclusive lock on state file after {}s",
                        timeout.as_secs()
                    )));
                }
                std::thread::sleep(LOCK_POLL_INTERVAL);
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// Scrape state manager
pub struct StateManager {
    state_file: PathBuf,
    state: Arc<Mutex<ScrapeState>>,
    /// Maximum time to wait for the exclusive file lock when saving.
    lock_timeout: Duration,
}

impl StateManager {
    /// Create a new state manager with the default 30-second lock timeout.
    pub fn new(state_file: PathBuf) -> Result<Self> {
        Self::new_with_timeout(state_file, DEFAULT_LOCK_TIMEOUT)
    }

    /// Create a new state manager with a configurable lock timeout.
    ///
    /// Pass `Duration::ZERO` to disable the timeout (wait indefinitely).
    pub fn new_with_timeout(state_file: PathBuf, lock_timeout: Duration) -> Result<Self> {
        let state = if state_file.exists() {
            Self::load_state(&state_file)?
        } else {
            ScrapeState::new()
        };

        Ok(StateManager {
            state_file,
            state: Arc::new(Mutex::new(state)),
            lock_timeout,
        })
    }

    /// Load state from file
    fn load_state(path: &Path) -> Result<ScrapeState> {
        let file = File::open(path)?;

        // Check if file is empty
        let metadata = file.metadata()?;
        if metadata.len() == 0 {
            return Ok(ScrapeState::new());
        }

        let reader = BufReader::new(file);
        let state = serde_json::from_reader(reader)
            .map_err(|e| crate::error::AgentScribeError::State(format!("Failed to parse state: {}", e)))?;
        Ok(state)
    }

    /// Save state to file (with file locking).
    ///
    /// The exclusive lock is acquired **before** truncating the file.  This
    /// prevents a concurrent reader from observing an empty file between the
    /// truncation and the subsequent write.
    pub fn save(&self) -> Result<()> {
        let state = self.state.lock().unwrap();
        let state_ref = &*state;

        // Ensure parent directory exists
        if let Some(parent) = self.state_file.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Open WITHOUT O_TRUNC — we must acquire the exclusive lock first so
        // that no reader can see the file in a truncated-but-not-yet-written
        // state.
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&self.state_file)?;

        // Acquire the exclusive lock before modifying the file.
        lock_exclusive_with_timeout(&file, self.lock_timeout)?;

        // Safe to truncate now that we hold the exclusive lock.
        file.set_len(0)?;

        // File position for a freshly-opened (non-append) file is always 0,
        // so writing immediately after set_len(0) is correct.
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, state_ref)
            .map_err(|e| crate::error::AgentScribeError::State(format!("Failed to write state: {}", e)))?;

        // Lock is released when the file handle inside BufWriter is dropped.
        Ok(())
    }

    /// Get state for a file
    pub fn get_file_state(&self, file_path: &str) -> Option<SourceFileState> {
        let state = self.state.lock().unwrap();
        state.sources.get(file_path).cloned()
    }

    /// Get or create state for a file
    pub fn get_or_create_file_state(&self, file_path: &str, plugin: &str) -> SourceFileState {
        let mut state = self.state.lock().unwrap();
        state
            .sources
            .entry(file_path.to_string())
            .or_insert_with(|| SourceFileState::new(plugin.to_string()))
            .clone()
    }

    /// Update state for a file after scraping
    pub fn update_file_state<F>(&self, file_path: &str, mut update: F) -> Result<()>
    where
        F: FnMut(&mut SourceFileState),
    {
        let mut state = self.state.lock().unwrap();
        let file_state = state
            .sources
            .entry(file_path.to_string())
            .or_insert_with(|| SourceFileState::new("unknown".to_string()));

        update(file_state);
        file_state.last_scraped = Utc::now();

        Ok(())
    }

    /// Set the last byte offset for a file
    pub fn set_offset(&self, file_path: &str, offset: u64) -> Result<()> {
        self.update_file_state(file_path, |state| {
            state.last_byte_offset = offset;
        })
    }

    /// Set the last delimiter offset for a file (for markdown delimiter-based parsing)
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
                state.session_ids.push(session_id.clone());
            }
        })
    }

    /// Remove a file from the state
    pub fn remove_file(&self, file_path: &str) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.sources.remove(file_path);
        Ok(())
    }

    /// Get all files for a plugin
    pub fn files_for_plugin(&self, plugin: &str) -> Vec<String> {
        let state = self.state.lock().unwrap();
        state
            .sources
            .iter()
            .filter(|(_, s)| s.plugin == plugin)
            .map(|(p, _)| p.clone())
            .collect()
    }

    /// Get all state (clone)
    pub fn get_all(&self) -> ScrapeState {
        let state = self.state.lock().unwrap();
        state.clone()
    }

    /// Check if a file needs re-scraping based on modification time
    pub fn needs_rescrape(&self, file_path: &Path, _plugin: &str) -> Result<bool> {
        let path_str = file_path.to_str().ok_or_else(|| {
            crate::error::AgentScribeError::FileNotFound(file_path.to_path_buf())
        })?;

        let metadata = std::fs::metadata(file_path)?;
        let system_time = metadata.modified()?;
        // Convert SystemTime to DateTime<Utc> using duration since epoch
        let duration = system_time
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| crate::error::AgentScribeError::State("Invalid file modification time".to_string()))?;
        let modified = chrono::DateTime::from_timestamp(duration.as_secs() as i64, duration.subsec_nanos())
            .ok_or_else(|| crate::error::AgentScribeError::State("Invalid timestamp".to_string()))?;

        if let Some(file_state) = self.get_file_state(path_str) {
            // Check if file was modified since last scrape
            if modified > file_state.last_modified {
                // Check for truncation (file size decreased)
                if metadata.len() < file_state.last_byte_offset {
                    // File was truncated - need full rescan
                    return Ok(true);
                }
                // File was appended to - can do incremental scrape
                return Ok(true);
            }
            return Ok(false);
        }

        // New file - needs scraping
        Ok(true)
    }

    /// Check for truncated files and remove their state
    pub fn detect_truncation(&self) -> Result<Vec<String>> {
        let mut truncated = Vec::new();
        let state = self.state.lock().unwrap();

        for (path, file_state) in &state.sources {
            if let Ok(metadata) = std::fs::metadata(path) {
                if metadata.len() < file_state.last_byte_offset {
                    truncated.push(path.clone());
                }
            }
        }

        drop(state);

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
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    #[test]
    fn test_state_save_load() {
        let temp_file = NamedTempFile::new().unwrap();
        let state_path = temp_file.path().to_path_buf();

        // Create and save state
        let manager = StateManager::new(state_path.clone()).unwrap();
        manager.update_file_state("/test/file.jsonl", |state| {
            state.last_byte_offset = 1000;
            state.session_ids.push("test-session".to_string());
        }).unwrap();
        manager.save().unwrap();

        // Load state in new manager
        let manager2 = StateManager::new(state_path).unwrap();
        let file_state = manager2.get_file_state("/test/file.jsonl").unwrap();

        assert_eq!(file_state.last_byte_offset, 1000);
        assert_eq!(file_state.session_ids.len(), 1);
    }

    #[test]
    fn test_needs_rescrape() {
        let temp_file = NamedTempFile::new().unwrap();
        let state_path = temp_file.path().to_path_buf();

        let manager = StateManager::new(state_path).unwrap();

        // New file should need scraping
        assert!(manager.needs_rescrape(temp_file.path(), "test").unwrap());
    }

    /// Two concurrent saves must not corrupt the state file.
    ///
    /// Both managers write different offsets for the same key.  After both
    /// complete, the file must be valid JSON with a parseable ScrapeState.
    #[test]
    fn test_concurrent_saves_no_corruption() {
        let temp_file = NamedTempFile::new().unwrap();
        let state_path = temp_file.path().to_path_buf();

        let m1 = Arc::new(
            StateManager::new_with_timeout(state_path.clone(), Duration::from_secs(10)).unwrap(),
        );
        let m2 = Arc::new(
            StateManager::new_with_timeout(state_path.clone(), Duration::from_secs(10)).unwrap(),
        );

        m1.update_file_state("/test/file.jsonl", |s| s.last_byte_offset = 111).unwrap();
        m2.update_file_state("/test/file.jsonl", |s| s.last_byte_offset = 222).unwrap();

        let m1c = m1.clone();
        let m2c = m2.clone();

        let t1 = std::thread::spawn(move || m1c.save().unwrap());
        let t2 = std::thread::spawn(move || m2c.save().unwrap());

        t1.join().unwrap();
        t2.join().unwrap();

        // The file must be valid, parseable JSON — not a truncated/empty blob.
        let content = std::fs::read_to_string(&state_path).unwrap();
        assert!(!content.is_empty(), "state file must not be empty after concurrent saves");
        let _: ScrapeState = serde_json::from_str(&content)
            .expect("state file must be valid JSON after concurrent saves");
    }

    /// When a process already holds the exclusive lock, save() must time out
    /// and return an error rather than hanging forever.
    #[test]
    fn test_lock_timeout() {
        let temp_file = NamedTempFile::new().unwrap();
        let state_path = temp_file.path().to_path_buf();

        // Seed the file with valid initial state so the manager can load it.
        {
            let seed = StateManager::new(state_path.clone()).unwrap();
            seed.save().unwrap();
        }

        // Hold an exclusive lock on the state file via a separate fd.
        let lock_fd = OpenOptions::new()
            .write(true)
            .open(&state_path)
            .unwrap();
        lock_fd.lock_exclusive().unwrap();

        // A manager with a very short timeout should fail quickly.
        let manager =
            StateManager::new_with_timeout(state_path.clone(), Duration::from_millis(300)).unwrap();
        let result = manager.save();

        // Release the external lock
        lock_fd.unlock().unwrap();

        assert!(result.is_err(), "expected timeout error when lock is held externally");
        let err_str = result.unwrap_err().to_string();
        assert!(
            err_str.contains("timed out"),
            "error message should mention timeout, got: {}",
            err_str
        );
    }
}

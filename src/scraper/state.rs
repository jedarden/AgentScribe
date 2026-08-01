//! Scrape state tracking for incremental scraping
//!
//! Tracks position per source file for incremental scrapes.

use crate::error::Result;
use crate::event::{ScrapeState, SourceFileState};
use chrono::Utc;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Default lock timeout (30 seconds).
#[allow(dead_code)]
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
    #[allow(dead_code)]
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

    /// Load state from file.
    ///
    /// Corruption is recoverable, not fatal (ADR-1): a state file that fails
    /// to parse is renamed to `<path>.corrupt-<timestamp>` so the evidence is
    /// preserved for debugging, and loading proceeds from empty state rather
    /// than propagating the error and aborting scraper construction. This
    /// degrades incremental scraping to a one-time full rescan of already-known
    /// files rather than blocking forever.
    fn load_state(path: &Path) -> Result<ScrapeState> {
        let file = File::open(path)?;

        // Check if file is empty
        let metadata = file.metadata()?;
        if metadata.len() == 0 {
            return Ok(ScrapeState::new());
        }

        let reader = BufReader::new(file);
        match serde_json::from_reader(reader) {
            Ok(state) => Ok(state),
            Err(e) => {
                let quarantine_path = Self::quarantine_path(path);
                tracing::error!(
                    path = %path.display(),
                    quarantine = %quarantine_path.display(),
                    error = %e,
                    "scrape state file is corrupt; quarantining and starting from empty state"
                );
                if let Err(rename_err) = std::fs::rename(path, &quarantine_path) {
                    tracing::error!(
                        path = %path.display(),
                        error = %rename_err,
                        "failed to quarantine corrupt scrape state file"
                    );
                }
                Ok(ScrapeState::new())
            }
        }
    }

    /// Path of the sibling lock file used to serialize `save()` calls.
    fn lock_path(state_file: &Path) -> PathBuf {
        let mut s = state_file.as_os_str().to_owned();
        s.push(".lock");
        PathBuf::from(s)
    }

    /// Path of the pid-scoped temp file `save()` writes before renaming it
    /// into place.
    fn tmp_path(state_file: &Path) -> PathBuf {
        let mut s = state_file.as_os_str().to_owned();
        s.push(format!(".tmp-{}", std::process::id()));
        PathBuf::from(s)
    }

    /// Path a corrupt state file is renamed to before `load_state` resets to
    /// empty state.
    fn quarantine_path(state_file: &Path) -> PathBuf {
        let mut s = state_file.as_os_str().to_owned();
        s.push(format!(".corrupt-{}", Utc::now().format("%Y%m%dT%H%M%SZ")));
        PathBuf::from(s)
    }

    /// Save state to file: atomic write via temp-file-plus-rename, guarded by
    /// an exclusive lock on a sibling lock file (ADR-1).
    ///
    /// We lock a separate `<state_file>.lock` file rather than `state_file`
    /// itself because `rename()` swaps in a brand-new inode — a lock held on
    /// an open handle to the old data file would not follow the path across
    /// the rename, so a second writer opening `state_file` fresh right after
    /// the swap would silently see no lock at all. Locking a stable sibling
    /// path that is never replaced keeps the mutual exclusion real across the
    /// whole write-then-rename critical section.
    ///
    /// The rename itself is atomic on the same filesystem: a crash at any
    /// point before it completes leaves the previous, valid state file
    /// untouched — never a truncated or torn one.
    pub fn save(&self) -> Result<()> {
        let state = self.state.lock().unwrap();
        let state_ref = &*state;

        // Ensure parent directory exists
        if let Some(parent) = self.state_file.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Serialize up front so a failure here never touches disk at all.
        let json = serde_json::to_vec_pretty(state_ref).map_err(|e| {
            crate::error::AgentScribeError::State(format!("Failed to serialize state: {}", e))
        })?;

        let lock_path = Self::lock_path(&self.state_file);
        let lock_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;
        lock_exclusive_with_timeout(&lock_file, self.lock_timeout)?;

        let tmp_path = Self::tmp_path(&self.state_file);
        {
            let tmp_file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp_path)?;
            let mut writer = BufWriter::new(&tmp_file);
            writer.write_all(&json).map_err(|e| {
                crate::error::AgentScribeError::State(format!("Failed to write state: {}", e))
            })?;
            writer.flush()?;
            tmp_file.sync_all()?;
        }
        std::fs::rename(&tmp_path, &self.state_file)?;

        // Lock is released when `lock_file` is dropped here.
        Ok(())
    }

    /// Get state for a file
    pub fn get_file_state(&self, file_path: &str) -> Option<SourceFileState> {
        let state = self.state.lock().unwrap();
        state.sources.get(file_path).cloned()
    }

    /// Get or create state for a file
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
        let path_str = file_path
            .to_str()
            .ok_or_else(|| crate::error::AgentScribeError::FileNotFound(file_path.to_path_buf()))?;

        let metadata = std::fs::metadata(file_path)?;
        let system_time = metadata.modified()?;
        // Convert SystemTime to DateTime<Utc> using duration since epoch
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
    #[allow(dead_code)]
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
        manager
            .update_file_state("/test/file.jsonl", |state| {
                state.last_byte_offset = 1000;
                state.session_ids.push("test-session".to_string());
            })
            .unwrap();
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

        // The file must be valid, parseable JSON — not a truncated/empty blob.
        let content = std::fs::read_to_string(&state_path).unwrap();
        assert!(
            !content.is_empty(),
            "state file must not be empty after concurrent saves"
        );
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

        // save() locks a sibling `.lock` file, not the data file itself (see
        // save()'s doc comment for why) — hold that lock via a separate fd.
        let lock_path = StateManager::lock_path(&state_path);
        let lock_fd = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();
        lock_fd.lock_exclusive().unwrap();

        // A manager with a very short timeout should fail quickly.
        let manager =
            StateManager::new_with_timeout(state_path.clone(), Duration::from_millis(300)).unwrap();
        let result = manager.save();

        // Release the external lock
        lock_fd.unlock().unwrap();

        assert!(
            result.is_err(),
            "expected timeout error when lock is held externally"
        );
        let err_str = result.unwrap_err().to_string();
        assert!(
            err_str.contains("timed out"),
            "error message should mention timeout, got: {}",
            err_str
        );
    }

    /// A state file that fails to parse must not abort construction — it
    /// should be quarantined and loading should proceed from empty state.
    #[test]
    fn test_load_corrupt_state_is_quarantined_not_fatal() {
        let temp_file = NamedTempFile::new().unwrap();
        let state_path = temp_file.path().to_path_buf();

        std::fs::write(&state_path, b"{ this is not valid json").unwrap();

        let manager = StateManager::new(state_path.clone()).unwrap();
        assert!(manager.get_all().sources.is_empty());

        // The corrupt content must no longer live at the original path...
        let still_corrupt = state_path.exists()
            && std::fs::read(&state_path).unwrap() == b"{ this is not valid json";
        assert!(!still_corrupt);

        // ...and must have been preserved in a `*.corrupt-<timestamp>` sibling.
        let parent = state_path.parent().unwrap();
        let stem = state_path.file_name().unwrap().to_str().unwrap().to_owned();
        let quarantined = std::fs::read_dir(parent)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| {
                let name = e.file_name();
                let name = name.to_str().unwrap_or("");
                name.starts_with(&stem) && name.contains(".corrupt-")
            });
        assert!(quarantined, "expected a quarantined *.corrupt-* file");
    }
}

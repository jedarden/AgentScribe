//! Config file change detection and correlation with sessions.
//!
//! This module provides two mechanisms:
//! 1. In-session detection via tool_params enrichment (config_writes in BehavioralSignals)
//! 2. Post-session correlation via filesystem scanning
//!
//! The post-session tracker scans for recently modified config files and correlates
//! them with sessions that ended within a configurable time window before the change.

use crate::error::{AgentScribeError, Result};
use crate::event::Event;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use glob::Pattern;

/// Config file change correlated with sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChange {
    /// Path to the config file that was modified
    pub config_file: PathBuf,
    /// When the file was last modified (filesystem mtime)
    pub modified_at: DateTime<Utc>,
    /// Sessions that ended within 2 hours before this config change
    pub correlated_sessions: Vec<CorrelatedSession>,
}

/// A session correlated with a config change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelatedSession {
    /// Session ID (e.g., "claude-code/20250101-120000")
    pub session_id: String,
    /// When the session ended
    pub ended_at: DateTime<Utc>,
    /// Time delta between session end and config modification
    pub seconds_delta: i64,
    /// Session outcome (if available)
    pub outcome: Option<String>,
    /// Session summary (if available)
    pub summary: Option<String>,
}

/// Config change tracking options
#[derive(Debug, Clone)]
pub struct ConfigTrackerOptions {
    /// Config file patterns to watch (glob patterns)
    pub config_patterns: Vec<String>,
    /// Maximum age of config changes to consider (hours)
    pub max_age_hours: i64,
    /// Correlation window: sessions ending within this many hours before a change count
    pub correlation_window_hours: i64,
}

impl Default for ConfigTrackerOptions {
    fn default() -> Self {
        Self {
            config_patterns: vec![
                "**/CLAUDE.md".to_string(),
                "**/AGENTS.md".to_string(),
                "**/.claude/CLAUDE.md".to_string(),
                "**/memory/*.md".to_string(),
                "**/docs/notes/*.md".to_string(),
                "**/.needle/**".to_string(),
                "**/MEMORY.md".to_string(),
            ],
            max_age_hours: 24,
            correlation_window_hours: 2,
        }
    }
}

/// Storage for config change correlations
pub struct ConfigChangeStore {
    /// Directory where change data is stored
    store_dir: PathBuf,
}

impl ConfigChangeStore {
    /// Create a new store with the given data directory
    pub fn new(data_dir: &Path) -> Result<Self> {
        let store_dir = data_dir.join("config-changes");
        fs::create_dir_all(&store_dir)?;
        Ok(Self { store_dir })
    }

    /// Store config changes for a specific date
    pub fn store_changes(&self, date: chrono::NaiveDate, changes: &[ConfigChange]) -> Result<()> {
        let filename = format!("{}.json", date.format("%Y-%m-%d"));
        let path = self.store_dir.join(&filename);
        let json = serde_json::to_string_pretty(changes)
            .map_err(|e| AgentScribeError::DataDir(format!("Failed to serialize: {}", e)))?;
        fs::write(&path, json)?;
        Ok(())
    }

    /// Load config changes for a specific date range
    pub fn load_changes_since(&self, since: DateTime<Utc>) -> Result<Vec<ConfigChange>> {
        let mut all_changes = Vec::new();

        if !self.store_dir.exists() {
            return Ok(all_changes);
        }

        for entry in fs::read_dir(&self.store_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            // Parse date from filename
            let stem = path.file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| AgentScribeError::DataDir("Invalid filename".to_string()))?;

            if let Ok(date) = chrono::NaiveDate::parse_from_str(stem, "%Y-%m-%d") {
                let file_date = date.and_hms_opt(0, 0, 0)
                    .and_then(|dt| dt.and_utc())
                    .ok_or_else(|| AgentScribeError::DataDir("Invalid date".to_string()))?;

                if file_date >= since {
                    let content = fs::read_to_string(&path)?;
                    let changes: Vec<ConfigChange> = serde_json::from_str(&content)
                        .map_err(|e| AgentScribeError::DataDir(format!("Failed to parse: {}", e)))?;
                    all_changes.extend(changes);
                }
            }
        }

        // Sort by modification time (newest first)
        all_changes.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));

        Ok(all_changes)
    }

    /// Find config changes for a specific file pattern
    pub fn find_changes_for_file(&self, file_pattern: &str) -> Result<Vec<ConfigChange>> {
        let all_changes = self.load_changes_since(Utc::now() - Duration::days(30))?;

        let pattern = Pattern::new(file_pattern)
            .map_err(|e| AgentScribeError::DataDir(format!("Invalid glob pattern: {}", e)))?;

        let matching: Vec<_> = all_changes
            .into_iter()
            .filter(|change| {
                change.config_file.to_str()
                    .map(|p| pattern.matches(p))
                    .unwrap_or(false)
            })
            .collect();

        Ok(matching)
    }
}

/// Detect config file changes and correlate with sessions
pub struct ConfigChangeDetector {
    options: ConfigTrackerOptions,
    store: ConfigChangeStore,
}

impl ConfigChangeDetector {
    /// Create a new detector with the given options
    pub fn new(data_dir: &Path, options: ConfigTrackerOptions) -> Result<Self> {
        let store = ConfigChangeStore::new(data_dir)?;
        Ok(Self { options, store })
    }

    /// Scan for recently modified config files and correlate with sessions
    pub fn scan_and_correlate(
        &self,
        project_paths: &[PathBuf],
        sessions: &HashMap<String, SessionInfo>,
    ) -> Result<Vec<ConfigChange>> {
        let mut changes = Vec::new();
        let cutoff = Utc::now() - Duration::hours(self.options.max_age_hours);

        for project_path in project_paths {
            if !project_path.exists() {
                continue;
            }

            for pattern in &self.options.config_patterns {
                let base_path = if pattern.starts_with("**/") {
                    // Pattern is relative to any subdirectory
                    project_path
                } else if pattern.starts_with("**") {
                    // Pattern like "**.md" - search recursively from project root
                    project_path
                } else {
                    project_path
                };

                let glob_pattern = if let Some(stripped) = pattern.strip_prefix("**/") {
                    base_path.join(stripped).to_string_lossy().to_string()
                } else if pattern.starts_with("**") {
                    base_path.join(&pattern[2..]).to_string_lossy().to_string()
                } else {
                    base_path.join(pattern).to_string_lossy().to_string()
                };

                // Convert ** to glob pattern for glob crate
                let glob_pattern = glob_pattern.replace("**", "*");

                if let Ok(matches) = glob::glob(&glob_pattern) {
                    for entry in matches {
                        if let Ok(path) = entry {
                            if let Ok(metadata) = path.metadata() {
                                if !metadata.is_file() {
                                    continue;
                                }

                                let modified = metadata.modified()
                                    .ok()
                                    .and_then(|t| t.try_into().ok())
                                    .map(|dt: DateTime<Utc>| dt);

                                if let Some(modified_at) = modified {
                                    if modified_at < cutoff {
                                        continue;
                                    }

                                    // Find correlated sessions
                                    let correlation_cutoff = modified_at - Duration::hours(self.options.correlation_window_hours);
                                    let correlated: Vec<_> = sessions
                                        .iter()
                                        .filter(|(_, info)| {
                                            info.ended.is_some_and(|ended| {
                                                ended >= correlation_cutoff && ended <= modified_at
                                            })
                                        })
                                        .map(|(id, info)| CorrelatedSession {
                                            session_id: id.clone(),
                                            ended_at: info.ended.unwrap_or(modified_at),
                                            seconds_delta: (modified_at - info.ended.unwrap_or(modified_at)).num_seconds(),
                                            outcome: info.outcome.clone(),
                                            summary: info.summary.clone(),
                                        })
                                        .collect();

                                    if !correlated.is_empty() {
                                        changes.push(ConfigChange {
                                            config_file: path.clone(),
                                            modified_at,
                                            correlated_sessions: correlated,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(changes)
    }

    /// Store detected changes to disk
    pub fn persist_changes(&self, changes: &[ConfigChange]) -> Result<()> {
        let today = Utc::now().date_naive();
        self.store.store_changes(today, changes)
    }

    /// Get all changes since a given time
    pub fn get_changes_since(&self, since: DateTime<Utc>) -> Result<Vec<ConfigChange>> {
        self.store.load_changes_since(since)
    }

    /// Get changes for a specific file pattern
    pub fn get_changes_for_file(&self, file_pattern: &str) -> Result<Vec<ConfigChange>> {
        self.store.find_changes_for_file(file_pattern)
    }
}

/// Session information for correlation
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// When the session ended
    pub ended: Option<DateTime<Utc>>,
    /// Session outcome
    pub outcome: Option<String>,
    /// Session summary
    pub summary: Option<String>,
}

impl SessionInfo {
    /// Extract session info from events
    pub fn from_events(events: &[Event]) -> Self {
        let ended = events.iter()
            .filter_map(|e| e.timestamp)
            .max();

        let outcome = events.iter()
            .find_map(|e| e.session_manifest.as_ref())
            .and_then(|m| m.outcome.clone());

        let summary = events.iter()
            .find_map(|e| e.session_manifest.as_ref())
            .and_then(|m| m.summary.clone());

        Self { ended, outcome, summary }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_config_tracker_options_default() {
        let opts = ConfigTrackerOptions::default();
        assert_eq!(opts.config_patterns.len(), 7);
        assert!(opts.config_patterns.contains(&"**/CLAUDE.md".to_string()));
        assert_eq!(opts.max_age_hours, 24);
        assert_eq!(opts.correlation_window_hours, 2);
    }

    #[test]
    fn test_config_change_store() {
        let temp_dir = TempDir::new().unwrap();
        let data_dir = temp_dir.path();
        let store = ConfigChangeStore::new(data_dir).unwrap();

        let date = chrono::NaiveDate::from_ymd_opt(2025, 1, 15).unwrap();
        let changes = vec![
            ConfigChange {
                config_file: PathBuf::from("/test/CLAUDE.md"),
                modified_at: Utc::now(),
                correlated_sessions: vec![],
            }
        ];

        store.store_changes(date, &changes).unwrap();

        let loaded = store.load_changes_since(Utc::now() - Duration::days(1)).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].config_file, PathBuf::from("/test/CLAUDE.md"));
    }

    #[test]
    fn test_config_change_store_find_by_pattern() {
        let temp_dir = TempDir::new().unwrap();
        let data_dir = temp_dir.path();
        let store = ConfigChangeStore::new(data_dir).unwrap();

        let date = chrono::NaiveDate::from_ymd_opt(2025, 1, 15).unwrap();
        let changes = vec![
            ConfigChange {
                config_file: PathBuf::from("/home/user/project/CLAUDE.md"),
                modified_at: Utc::now(),
                correlated_sessions: vec![],
            },
            ConfigChange {
                config_file: PathBuf::from("/home/user/project/README.md"),
                modified_at: Utc::now(),
                correlated_sessions: vec![],
            }
        ];

        store.store_changes(date, &changes).unwrap();

        let claude_changes = store.find_changes_for_file("**/CLAUDE.md").unwrap();
        assert_eq!(claude_changes.len(), 1);
    }

    #[test]
    fn test_session_info_from_events() {
        let now = Utc::now();
        let events = vec![
            Event {
                timestamp: Some(now - Duration::seconds(100)),
                session_manifest: Some(crate::event::SessionManifest {
                    outcome: Some("success".to_string()),
                    summary: Some("Test summary".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            Event {
                timestamp: Some(now),
                ..Default::default()
            },
        ];

        let info = SessionInfo::from_events(&events);
        assert_eq!(info.ended, Some(now));
        assert_eq!(info.outcome, Some("success".to_string()));
        assert_eq!(info.summary, Some("Test summary".to_string()));
    }

    #[test]
    fn test_scan_and_correlate() {
        let temp_dir = TempDir::new().unwrap();
        let data_dir = temp_dir.path();
        let project_dir = temp_dir.path().join("project");

        fs::create_dir_all(&project_dir).unwrap();

        // Create a CLAUDE.md file
        let claude_path = project_dir.join("CLAUDE.md");
        let mut file = fs::File::create(&claude_path).unwrap();
        file.write_all(b"# Test").unwrap();

        let options = ConfigTrackerOptions {
            config_patterns: vec!["**/CLAUDE.md".to_string()],
            max_age_hours: 24,
            correlation_window_hours: 2,
        };

        let detector = ConfigChangeDetector::new(data_dir, options).unwrap();

        let mut sessions = HashMap::new();
        let now = Utc::now();

        // Add a session that ended 1 hour ago
        sessions.insert(
            "test-session".to_string(),
            SessionInfo {
                ended: Some(now - Duration::hours(1)),
                outcome: Some("success".to_string()),
                summary: Some("Test session".to_string()),
            }
        );

        let changes = detector.scan_and_correlate(&[project_dir], &sessions).unwrap();

        // Should correlate the session with the config file
        assert!(!changes.is_empty());
    }
}

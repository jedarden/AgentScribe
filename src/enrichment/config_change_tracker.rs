//! Config change tracker: correlates file modifications with recent sessions.
//!
//! This module implements filesystem-level tracking that detects when config/memory
//! files are modified and finds sessions that may have prompted those changes.
//!
//! # Workflow
//!
//! 1. After each scrape, scan for config files modified in the last 24h
//! 2. For each modified file, find sessions that ended within 2h before modification
//! 3. Store correlations in ~/.agentscribe/config-changes/`<date>`.json
//!
//! # Data Format
//!
//! ```json
//! [
//!   {
//!     "config_file": "/home/user/project/CLAUDE.md",
//!     "modified_at": "2026-03-16T12:00:00Z",
//!     "correlated_sessions": [
//!       {
//!         "session_id": "claude-code/abc123",
//!         "outcome": "success",
//!         "summary": "Added project conventions to CLAUDE.md"
//!       }
//!     ]
//!   }
//! ]
//! ```

use crate::error::{AgentScribeError, Result};
use crate::event::SessionManifest;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Config/memory file glob patterns used for detection.
/// Must match patterns in behavioral_signals.rs to ensure consistency.
pub static CONFIG_FILE_PATTERNS: &[&str] = &[
    "CLAUDE.md",
    "AGENTS.md",
    ".claude/",
    ".needle/",
    "memory/",
    "docs/notes/",
    "MEMORY.md",
];

/// Window for scanning recently modified config files (24 hours)
const CONFIG_SCAN_WINDOW_HOURS: i64 = 24;

/// Window for correlating sessions with config changes (2 hours before modification)
const SESSION_CORRELATION_WINDOW_HOURS: i64 = 2;

/// A single config file change correlation record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChangeRecord {
    /// Absolute path to the config file that was modified
    pub config_file: String,
    /// Timestamp when the file was last modified
    pub modified_at: DateTime<Utc>,
    /// Sessions that ended within 2h before this modification and may have caused it
    pub correlated_sessions: Vec<CorrelatedSession>,
}

/// Type alias for ConfigChangeRecord to match task specification.
pub type ConfigChange = ConfigChangeRecord;

/// A session correlated with a config change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelatedSession {
    /// Session ID
    pub session_id: String,
    /// Session outcome
    pub outcome: String,
    /// Session summary (first sentence)
    pub summary: String,
    /// When the session ended
    pub ended_at: DateTime<Utc>,
}

/// Type alias for CorrelatedSession to match task specification.
pub type ConfigChangeSession = CorrelatedSession;

/// Tracker for correlating config file changes with sessions.
pub struct ConfigChangeTracker {
    /// Data directory
    data_dir: PathBuf,
    /// Known projects to scan (from session manifests)
    projects: HashMap<String, ProjectInfo>,
}

/// Metadata about a tracked project.
#[derive(Debug, Clone)]
struct ProjectInfo {
    /// Project path
    #[allow(dead_code)]
    path: PathBuf,
    /// Last scan timestamp
    last_scanned: Option<DateTime<Utc>>,
}

impl ConfigChangeTracker {
    /// Create a new tracker.
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            projects: HashMap::new(),
        }
    }

    /// Track config changes after scraping.
    ///
    /// This should be called after each scrape to detect newly modified
    /// config files and correlate them with recent sessions.
    ///
    /// # Arguments
    ///
    /// * `session_manifests` - All session manifests from the scrape (used to discover projects and correlate)
    ///
    /// # Returns
    ///
    /// Number of config change correlations found
    pub fn track_after_scrape(&mut self, session_manifests: &[SessionManifest]) -> Result<usize> {
        let now = Utc::now();
        let scan_threshold = now - Duration::hours(CONFIG_SCAN_WINDOW_HOURS);

        // Discover projects from session manifests
        self.update_projects_from_manifests(session_manifests);

        // Scan for modified config files
        let modified_configs = self.scan_modified_configs(&scan_threshold)?;

        if modified_configs.is_empty() {
            debug!("No recently modified config files found");
            return Ok(0);
        }

        // Correlate with sessions
        let mut correlations = Vec::new();
        for (config_path, modified_at) in modified_configs {
            let correlated =
                self.find_correlated_sessions(&config_path, modified_at, session_manifests)?;

            if !correlated.is_empty() {
                correlations.push(ConfigChangeRecord {
                    config_file: config_path,
                    modified_at,
                    correlated_sessions: correlated,
                });
            }
        }

        if !correlations.is_empty() {
            self.store_correlations(&correlations)?;
            info!(
                count = correlations.len(),
                "Tracked config change correlations"
            );
        }

        Ok(correlations.len())
    }

    /// Update tracked projects from session manifests.
    fn update_projects_from_manifests(&mut self, manifests: &[SessionManifest]) {
        for manifest in manifests {
            if let Some(ref project) = manifest.project {
                let path = project.clone();
                let info = self
                    .projects
                    .entry(path.clone())
                    .or_insert_with(|| ProjectInfo {
                        path: PathBuf::from(&path),
                        last_scanned: None,
                    });
                // Mark as scanned now
                info.last_scanned = Some(Utc::now());
            }
        }
    }

    /// Scan for config files modified within the time window.
    ///
    /// Returns list of (config_file_path, modified_at) tuples.
    fn scan_modified_configs(
        &self,
        threshold: &DateTime<Utc>,
    ) -> Result<Vec<(String, DateTime<Utc>)>> {
        let mut modified = Vec::new();

        for project_path in self.projects.keys() {
            // Scan each pattern within the project
            for pattern in CONFIG_FILE_PATTERNS {
                let config_candidates = self.find_config_files(project_path, pattern);

                for config_path in config_candidates {
                    if let Ok(metadata) = fs::metadata(&config_path) {
                        if let Ok(modified_ts) = metadata.modified() {
                            let modified_at: DateTime<Utc> = modified_ts.into();
                            if modified_at > *threshold {
                                modified
                                    .push((config_path.to_string_lossy().to_string(), modified_at));
                            }
                        }
                    }
                }
            }
        }

        modified.sort_by_key(|(_, ts)| *ts);
        modified.reverse(); // Most recent first

        Ok(modified)
    }

    /// Find config files matching a pattern within a project.
    fn find_config_files(&self, project_path: &str, pattern: &str) -> Vec<PathBuf> {
        let project = Path::new(project_path);
        let mut matches = Vec::new();

        // Handle different pattern types
        if pattern.ends_with('/') {
            // Directory pattern: scan for files inside
            let dir = if pattern.starts_with('.') {
                // Hidden directory like .claude/, .needle/
                project.join(pattern)
            } else {
                // Regular directory like memory/, docs/notes/
                project.join(pattern)
            };

            if dir.is_dir() {
                if let Ok(entries) = fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        matches.push(entry.path());
                    }
                }
            }
        } else if pattern.contains('.') {
            // File pattern like CLAUDE.md, MEMORY.md, AGENTS.md
            let file = project.join(pattern);
            if file.exists() {
                matches.push(file);
            }
        }

        matches
    }

    /// Find sessions that ended within 2h before a config modification.
    fn find_correlated_sessions(
        &self,
        config_path: &str,
        modified_at: DateTime<Utc>,
        manifests: &[SessionManifest],
    ) -> Result<Vec<CorrelatedSession>> {
        let correlation_threshold = modified_at - Duration::hours(SESSION_CORRELATION_WINDOW_HOURS);
        let mut correlated = Vec::new();

        for manifest in manifests {
            // Check if session ended within correlation window
            if let Some(ended) = manifest.ended {
                if ended >= correlation_threshold && ended <= modified_at {
                    // Check if session touched this config file or worked in the same project
                    let touched_config = self.session_touched_config(manifest, config_path);

                    if touched_config || self.session_in_same_project(manifest, config_path) {
                        correlated.push(CorrelatedSession {
                            session_id: manifest.session_id.clone(),
                            outcome: manifest.outcome.as_deref().unwrap_or("unknown").to_string(),
                            summary: manifest.summary.clone().unwrap_or_default(),
                            ended_at: ended,
                        });
                    }
                }
            }
        }

        // Sort by recency (most recent first)
        correlated.sort_by_key(|b| std::cmp::Reverse(b.ended_at));

        Ok(correlated)
    }

    /// Check if a session's events touched a specific config file.
    fn session_touched_config(&self, manifest: &SessionManifest, config_path: &str) -> bool {
        manifest
            .files_touched
            .iter()
            .any(|f| f.as_str() == config_path || f.as_str() == config_path.replace('\\', "/"))
    }

    /// Check if a session worked in the same project as a config file.
    fn session_in_same_project(&self, manifest: &SessionManifest, config_path: &str) -> bool {
        if let Some(ref project) = manifest.project {
            // Config file is in the session's project directory
            config_path.starts_with(project) || config_path.starts_with(&project.replace('\\', "/"))
        } else {
            false
        }
    }

    /// Store config change correlations to disk.
    fn store_correlations(&self, correlations: &[ConfigChangeRecord]) -> Result<()> {
        let changes_dir = self.data_dir.join("config-changes");
        fs::create_dir_all(&changes_dir)?;

        let date = Utc::now().format("%Y-%m-%d").to_string();
        let file_path = changes_dir.join(format!("{}.json", date));

        // Load existing correlations for today
        let mut existing: Vec<ConfigChangeRecord> = if file_path.exists() {
            let content = fs::read_to_string(&file_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Vec::new()
        };

        // Merge new correlations (avoid duplicates by config_file + modified_at)
        for new_corr in correlations {
            let is_duplicate = existing.iter().any(|existing| {
                existing.config_file == new_corr.config_file
                    && existing.modified_at == new_corr.modified_at
            });

            if !is_duplicate {
                existing.push(new_corr.clone());
            }
        }

        // Sort by modification time (most recent first)
        existing.sort_by_key(|b| std::cmp::Reverse(b.modified_at));

        // Write back
        let json = serde_json::to_string_pretty(&existing).map_err(|e| {
            AgentScribeError::Config(format!("Failed to serialize config changes: {}", e))
        })?;
        fs::write(&file_path, json)?;

        debug!(
            path = %file_path.display(),
            count = existing.len(),
            "Stored config change correlations"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manifest(
        session_id: &str,
        project: Option<&str>,
        ended: DateTime<Utc>,
        outcome: &str,
        summary: &str,
    ) -> SessionManifest {
        SessionManifest {
            session_id: session_id.to_string(),
            source_agent: "claude-code".to_string(),
            project: project.map(|p| p.to_string()),
            started: ended - chrono::Duration::hours(1),
            ended: Some(ended),
            turns: 10,
            summary: Some(summary.to_string()),
            outcome: Some(outcome.to_string()),
            tags: vec![],
            files_touched: vec![],
            model: None,
            parent_session_id: None,
        }
    }

    #[test]
    fn test_config_file_patterns_exist() {
        // Verify patterns match expectations from behavioral_signals.rs
        assert!(CONFIG_FILE_PATTERNS.contains(&"CLAUDE.md"));
        assert!(CONFIG_FILE_PATTERNS.contains(&"AGENTS.md"));
        assert!(CONFIG_FILE_PATTERNS.contains(&".claude/"));
        assert!(CONFIG_FILE_PATTERNS.contains(&".needle/"));
        assert!(CONFIG_FILE_PATTERNS.contains(&"memory/"));
        assert!(CONFIG_FILE_PATTERNS.contains(&"docs/notes/"));
        assert!(CONFIG_FILE_PATTERNS.contains(&"MEMORY.md"));
    }

    #[test]
    fn test_find_config_files_file_pattern() {
        let tracker = ConfigChangeTracker::new(PathBuf::from("/tmp/test-data"));

        // This test verifies the pattern matching logic; actual file existence
        // depends on test setup, so we just verify the logic doesn't crash
        let _results = tracker.find_config_files("/fake/project", "CLAUDE.md");
        // Result depends on whether /fake/project/CLAUDE.md exists
        // We're just checking the function doesn't panic
    }

    #[test]
    fn test_session_correlation_time_window() {
        let now = Utc::now();
        let tracker = ConfigChangeTracker::new(PathBuf::from("/tmp/data"));

        let manifests = vec![
            // Session 3 hours ago - outside correlation window
            make_manifest(
                "test/old",
                Some("/project"),
                now - Duration::hours(3),
                "success",
                "Old session",
            ),
            // Session 1 hour ago - inside correlation window
            make_manifest(
                "test/recent",
                Some("/project"),
                now - Duration::hours(1),
                "success",
                "Recent session",
            ),
        ];

        // Config modified now
        let config_path = "/project/CLAUDE.md";
        let correlated = tracker
            .find_correlated_sessions(config_path, now, &manifests)
            .unwrap();

        // Should only find the recent session
        assert_eq!(correlated.len(), 1);
        assert_eq!(correlated[0].session_id, "test/recent");
    }

    #[test]
    fn test_session_touched_config() {
        let manifest = SessionManifest {
            session_id: "test/1".to_string(),
            source_agent: "claude-code".to_string(),
            project: Some("/project".to_string()),
            started: Utc::now() - chrono::Duration::hours(1),
            ended: Some(Utc::now()),
            turns: 5,
            summary: Some("Edited CLAUDE.md".to_string()),
            outcome: Some("success".to_string()),
            tags: vec![],
            files_touched: vec!["/project/CLAUDE.md".to_string()],
            model: None,
            parent_session_id: None,
        };

        let tracker = ConfigChangeTracker::new(PathBuf::from("/tmp/data"));

        // Same path
        assert!(tracker.session_touched_config(&manifest, "/project/CLAUDE.md"));

        // Different file
        assert!(!tracker.session_touched_config(&manifest, "/project/README.md"));
    }

    #[test]
    fn test_session_in_same_project() {
        let manifest = SessionManifest {
            session_id: "test/1".to_string(),
            source_agent: "claude-code".to_string(),
            project: Some("/project".to_string()),
            started: Utc::now() - chrono::Duration::hours(1),
            ended: Some(Utc::now()),
            turns: 5,
            summary: Some("Some work".to_string()),
            outcome: Some("success".to_string()),
            tags: vec![],
            files_touched: vec![],
            model: None,
            parent_session_id: None,
        };

        let tracker = ConfigChangeTracker::new(PathBuf::from("/tmp/data"));

        // Config in same project
        assert!(tracker.session_in_same_project(&manifest, "/project/CLAUDE.md"));
        assert!(tracker.session_in_same_project(&manifest, "/project/.claude/settings.json"));

        // Config in different project
        assert!(!tracker.session_in_same_project(&manifest, "/other/CLAUDE.md"));
    }

    #[test]
    fn test_correlation_record_serialization() {
        let record = ConfigChangeRecord {
            config_file: "/project/CLAUDE.md".to_string(),
            modified_at: Utc::now(),
            correlated_sessions: vec![CorrelatedSession {
                session_id: "claude-code/abc123".to_string(),
                outcome: "success".to_string(),
                summary: "Added conventions".to_string(),
                ended_at: Utc::now(),
            }],
        };

        let json = serde_json::to_string(&record).unwrap();
        let deser: ConfigChangeRecord = serde_json::from_str(&json).unwrap();

        assert_eq!(deser.config_file, record.config_file);
        assert_eq!(deser.correlated_sessions.len(), 1);
        assert_eq!(
            deser.correlated_sessions[0].session_id,
            "claude-code/abc123"
        );
    }
}

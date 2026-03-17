//! Scraping orchestration
//!
//! Coordinates plugin loading, file discovery, parsing, and state management.

mod file_path_extractor;
mod state;

pub use state::StateManager;

use crate::event::Event;
use crate::parser::{FormatParser, JsonlParser, MarkdownParser, JsonTreeParser};
use crate::plugin::{LogFormat, Plugin, PluginManager};
use crate::error::{AgentScribeError, Result};
use chrono::Utc;
use glob::glob;
use shellexpand;
use std::path::{Path, PathBuf};
use std::fs::File;
use std::io::{BufWriter, Write};

/// Scraping result
#[derive(Debug, Clone)]
pub struct ScrapeResult {
    pub sessions_scraped: usize,
    pub events_written: usize,
    pub errors: Vec<ScrapeError>,
    pub files_processed: usize,
    pub files_skipped: usize,
}

/// Error that occurred during scraping (non-fatal)
#[derive(Debug, Clone)]
pub struct ScrapeError {
    pub file: String,
    pub line: Option<usize>,
    pub message: String,
}

/// Scraper - main orchestration
pub struct Scraper {
    plugin_manager: PluginManager,
    data_dir: PathBuf,
    sessions_dir: PathBuf,
    state_manager: StateManager,
}

impl Scraper {
    /// Create a new scraper
    pub fn new(data_dir: PathBuf) -> Result<Self> {
        let plugin_dir = data_dir.join("plugins");
        let state_file = data_dir.join("state").join("scrape-state.json");
        let sessions_dir = data_dir.join("sessions");

        // Create directories
        std::fs::create_dir_all(&plugin_dir)?;
        std::fs::create_dir_all(&sessions_dir)?;
        std::fs::create_dir_all(state_file.parent().unwrap())?;

        let plugin_manager = PluginManager::new(plugin_dir);
        let state_manager = StateManager::new(state_file)?;

        Ok(Scraper {
            plugin_manager,
            data_dir,
            sessions_dir,
            state_manager,
        })
    }

    /// Load all plugins
    pub fn load_plugins(&mut self) -> Result<Vec<String>> {
        self.plugin_manager.load_all()
    }

    /// Get the plugin manager
    pub fn plugin_manager(&self) -> &PluginManager {
        &self.plugin_manager
    }

    /// Get the state manager
    pub fn state_manager(&self) -> &StateManager {
        &self.state_manager
    }

    /// Discover log files for a plugin
    pub fn discover_files(&self, plugin: &Plugin) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();

        for pattern in &plugin.source.paths {
            // Expand ~ and environment variables
            let expanded = shellexpand::full(pattern)
                .map_err(|e| AgentScribeError::Glob(format!("Expansion error: {}", e)))?;

            // Use glob to find matching files
            let glob_result = glob(&expanded)
                .map_err(|e| AgentScribeError::Glob(format!("Invalid glob: {}", e)))?;

            for entry in glob_result.filter_map(|e| e.ok()) {
                let path = entry.as_path();

                // Skip if it matches exclude patterns
                let mut excluded = false;
                for exclude_pattern in &plugin.source.exclude {
                    let exclude_expanded = shellexpand::full(exclude_pattern)
                        .unwrap_or_default().into_owned();
                    if let Ok(exclude_glob) = glob(&exclude_expanded) {
                        if exclude_glob.filter_map(|e| e.ok()).any(|p| p == path) {
                            excluded = true;
                            break;
                        }
                    }
                }

                if !excluded && path.is_file() {
                    files.push(path.to_path_buf());
                }
            }
        }

        Ok(files)
    }

    /// Scrape all plugins
    pub fn scrape_all(&mut self) -> Result<ScrapeResult> {
        let mut total_result = ScrapeResult {
            sessions_scraped: 0,
            events_written: 0,
            errors: Vec::new(),
            files_processed: 0,
            files_skipped: 0,
        };

        let plugin_names: Vec<String> = self.plugin_manager.names()
            .into_iter()
            .map(String::from)
            .collect();

        for plugin_name in plugin_names {
            if let Some(plugin) = self.plugin_manager.get(&plugin_name).cloned() {
                let result = self.scrape_plugin(&plugin)?;
                total_result.sessions_scraped += result.sessions_scraped;
                total_result.events_written += result.events_written;
                total_result.errors.extend(result.errors);
                total_result.files_processed += result.files_processed;
                total_result.files_skipped += result.files_skipped;
            }
        }

        // Save updated state
        self.state_manager.save()?;

        Ok(total_result)
    }

    /// Scrape a single plugin
    pub fn scrape_plugin(&mut self, plugin: &Plugin) -> Result<ScrapeResult> {
        let files = self.discover_files(plugin)?;

        let mut result = ScrapeResult {
            sessions_scraped: 0,
            events_written: 0,
            errors: Vec::new(),
            files_processed: 0,
            files_skipped: 0,
        };

        for file_path in files {
            // Check if file needs scraping
            match self.state_manager.needs_rescrape(&file_path, &plugin.plugin.name) {
                Ok(true) => {
                    // Check if truncated
                    let file_state = self.state_manager.get_file_state(
                        file_path.to_str().unwrap_or("")
                    );
                    if let Some(state) = file_state {
                        let metadata = std::fs::metadata(&file_path)?;
                        if metadata.len() < state.last_byte_offset {
                            // File was truncated - remove state and rescan fully
                            self.state_manager.remove_file(file_path.to_str().unwrap_or(""))?;
                        }
                    }

                    match self.scrape_file(&file_path, plugin) {
                        Ok(file_result) => {
                            result.sessions_scraped += file_result.sessions_scraped;
                            result.events_written += file_result.events_written;
                            result.errors.extend(file_result.errors);
                            result.files_processed += 1;
                        }
                        Err(e) => {
                            result.errors.push(ScrapeError {
                                file: file_path.display().to_string(),
                                line: None,
                                message: e.to_string(),
                            });
                        }
                    }
                }
                Ok(false) => {
                    result.files_skipped += 1;
                }
                Err(e) => {
                    result.errors.push(ScrapeError {
                        file: file_path.display().to_string(),
                        line: None,
                        message: format!("State check error: {}", e),
                    });
                }
            }
        }

        Ok(result)
    }

    /// Scrape a single file
    pub fn scrape_file(&mut self, file_path: &Path, plugin: &Plugin) -> Result<ScrapeResult> {
        let parser: Box<dyn FormatParser> = match plugin.source.format {
            LogFormat::Jsonl => Box::new(JsonlParser),
            LogFormat::Markdown => Box::new(MarkdownParser),
            LogFormat::JsonTree => Box::new(JsonTreeParser),
            LogFormat::Sqlite => {
                return Err(AgentScribeError::InvalidPlugin(
                    "SQLite format not implemented in Phase 1".to_string(),
                ));
            }
        };

        // Detect sessions in the file
        let sessions = parser.detect_sessions(file_path, plugin)?;

        let mut result = ScrapeResult {
            sessions_scraped: 0,
            events_written: 0,
            errors: Vec::new(),
            files_processed: 1,
            files_skipped: 0,
        };

        for session_info in sessions {
            // Parse events for this session
            match parser.parse(file_path, plugin) {
                Ok(events) => {
                    if events.is_empty() {
                        continue;
                    }

                    // Write session to file
                    let prefixed_session_id = format!("{}/{}", plugin.plugin.name, session_info.session_id);
                    let session_path = self.sessions_dir
                        .join(&plugin.plugin.name)
                        .join(format!("{}.jsonl", session_info.session_id));

                    // Create plugin directory if needed
                    std::fs::create_dir_all(session_path.parent().unwrap())?;

                    match Self::write_session(&session_path, &events, plugin) {
                        Ok(_) => {
                            result.sessions_scraped += 1;
                            result.events_written += events.len();

                            // Update state
                            let path_str = file_path.to_str().unwrap_or("");
                            self.state_manager.add_session(path_str, prefixed_session_id)?;
                        }
                        Err(e) => {
                            result.errors.push(ScrapeError {
                                file: file_path.display().to_string(),
                                line: None,
                                message: format!("Write error: {}", e),
                            });
                        }
                    }
                }
                Err(e) => {
                    if e.is_skippable() {
                        result.errors.push(ScrapeError {
                            file: file_path.display().to_string(),
                            line: None,
                            message: e.to_string(),
                        });
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        // Update file offset state
        let metadata = std::fs::metadata(file_path)?;
        self.state_manager.set_offset(
            file_path.to_str().unwrap_or(""),
            metadata.len()
        )?;
        self.state_manager.set_modified(
            file_path.to_str().unwrap_or(""),
            Utc::now()
        )?;

        Ok(result)
    }

    /// Write a session to disk
    fn write_session(path: &Path, events: &[Event], _plugin: &Plugin) -> Result<()> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        for event in events {
            let jsonl = event.to_jsonl()
                .map_err(|e| AgentScribeError::State(format!("JSON error: {}", e)))?;
            writeln!(writer, "{}", jsonl)?;
        }

        writer.flush()?;
        Ok(())
    }

    /// Get session file path for a session ID
    pub fn session_path(&self, session_id: &str) -> Option<PathBuf> {
        // Parse session ID as "<plugin>/<id>"
        let parts: Vec<&str> = session_id.splitn(2, '/').collect();
        if parts.len() == 2 {
            let plugin = parts[0];
            let id = parts[1];
            Some(self.sessions_dir.join(plugin).join(format!("{}.jsonl", id)))
        } else {
            None
        }
    }

    /// Read a session from disk
    pub fn read_session(&self, session_id: &str) -> Result<Vec<Event>> {
        let path = self.session_path(session_id)
            .ok_or_else(|| AgentScribeError::FileNotFound(PathBuf::from(session_id)))?;

        let content = std::fs::read_to_string(&path)?;
        let mut events = Vec::new();

        for (line_num, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }

            match Event::from_jsonl(line) {
                Ok(event) => events.push(event),
                Err(e) => {
                    eprintln!("Warning: Invalid JSON at line {}: {}", line_num + 1, e);
                }
            }
        }

        Ok(events)
    }

    /// List all sessions for a plugin
    pub fn list_sessions(&self, plugin_name: &str) -> Result<Vec<String>> {
        let plugin_dir = self.sessions_dir.join(plugin_name);

        if !plugin_dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();

        for entry in std::fs::read_dir(&plugin_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    sessions.push(format!("{}/{}", plugin_name, stem));
                }
            }
        }

        Ok(sessions)
    }

    /// Get all session IDs
    pub fn all_sessions(&self) -> Result<Vec<String>> {
        let mut all = Vec::new();

        for plugin_name in self.plugin_manager.names() {
            all.extend(self.list_sessions(plugin_name)?);
        }

        Ok(all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_path() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(".agentscribe");
        let sessions_dir = data_dir.join("sessions");

        let scraper = Scraper::new(data_dir).unwrap();

        assert_eq!(
            scraper.session_path("test-agent/session-123"),
            Some(sessions_dir.join("test-agent").join("session-123.jsonl"))
        );

        assert_eq!(scraper.session_path("invalid"), None);
    }
}

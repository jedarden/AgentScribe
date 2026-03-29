//! Scraping orchestration
//!
//! Coordinates plugin loading, file discovery, parsing, and state management.

mod file_path_extractor;
mod state;

pub use state::StateManager;
pub use file_path_extractor::FilePathExtractor;

use crate::event::Event;
use crate::index::{build_manifest_from_events, IndexManager};
use crate::parser::{FormatParser, JsonlParser, MarkdownParser, JsonTreeParser, SqliteParser};
use crate::plugin::{LogFormat, Plugin, PluginManager, ProjectDetection, ModelDetection};
use crate::error::{AgentScribeError, Result};
use chrono::Utc;
use glob::glob;
use serde_json::Value;
use shellexpand;
use std::path::{Path, PathBuf};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::process::Command;
use tracing::{debug, info, warn};

/// Scraping result
#[derive(Debug, Clone)]
pub struct ScrapeResult {
    pub sessions_scraped: usize,
    pub sessions_indexed: usize,
    pub events_written: usize,
    pub errors: Vec<ScrapeError>,
    pub files_processed: usize,
    pub files_skipped: usize,
    /// Agent types (plugin names) that contributed at least one session.
    pub agent_types: Vec<String>,
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
    index_manager: Option<IndexManager>,
    index_write_depth: usize,
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

        // Initialize index manager (best-effort — scraping continues without indexing if it fails)
        let index_manager = match IndexManager::open(&data_dir) {
            Ok(mgr) => Some(mgr),
            Err(e) => {
                eprintln!("Warning: Index not available: {}. Scraping without indexing.", e);
                None
            }
        };

        Ok(Scraper {
            plugin_manager,
            data_dir,
            sessions_dir,
            state_manager,
            index_manager,
            index_write_depth: 0,
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

    /// Begin an index write session. Uses depth tracking so nested scrape calls
    /// (scrape_all → scrape_plugin → scrape_file) only commit at the outermost level.
    fn begin_index_write(&mut self) {
        if self.index_write_depth == 0 {
            if let Some(ref mut mgr) = self.index_manager {
                if let Err(e) = mgr.begin_write() {
                    warn!(error = %e, "failed to open index writer; disabling indexing");
                    self.index_manager = None;
                }
            }
        }
        self.index_write_depth += 1;
    }

    /// End an index write session. Commits and releases the writer only when depth
    /// returns to zero, making indexed documents visible to concurrent readers.
    fn end_index_write(&mut self) {
        if self.index_write_depth > 0 {
            self.index_write_depth -= 1;
        }
        if self.index_write_depth == 0 {
            if let Some(ref mut mgr) = self.index_manager {
                if let Err(e) = mgr.finish() {
                    warn!(error = %e, "failed to commit index");
                }
            }
        }
    }

    /// Index a session if the index manager is available.
    /// Returns true if the session was indexed.
    fn index_session_events(
        &mut self,
        events: &[Event],
        session_id: &str,
        source_agent: &str,
        project: Option<&str>,
        model: Option<&str>,
    ) -> bool {
        if let Some(ref mut mgr) = self.index_manager {
            let manifest = build_manifest_from_events(events, session_id, source_agent, project, model);
            match mgr.index_session(events, &manifest) {
                Ok(()) => true,
                Err(e) => {
                    warn!(session_id = %session_id, error = %e, "failed to index session");
                    false
                }
            }
        } else {
            false
        }
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
        self.begin_index_write();

        let mut total_result = ScrapeResult {
            sessions_scraped: 0,
            sessions_indexed: 0,
            events_written: 0,
            errors: Vec::new(),
            files_processed: 0,
            files_skipped: 0,
            agent_types: Vec::new(),
        };

        let plugin_names: Vec<String> = self.plugin_manager.names()
            .into_iter()
            .map(String::from)
            .collect();

        info!(plugins = plugin_names.len(), "starting scrape_all");

        for plugin_name in plugin_names {
            if let Some(plugin) = self.plugin_manager.get(&plugin_name).cloned() {
                let result = self.scrape_plugin(&plugin)?;
                total_result.sessions_scraped += result.sessions_scraped;
                total_result.sessions_indexed += result.sessions_indexed;
                total_result.events_written += result.events_written;
                total_result.errors.extend(result.errors);
                total_result.files_processed += result.files_processed;
                total_result.files_skipped += result.files_skipped;
                for agent in result.agent_types {
                    if !total_result.agent_types.contains(&agent) {
                        total_result.agent_types.push(agent);
                    }
                }
            }
        }

        // Save updated state
        self.state_manager.save()?;

        self.end_index_write();

        info!(
            sessions_scraped = total_result.sessions_scraped,
            sessions_indexed = total_result.sessions_indexed,
            "scrape_all complete"
        );

        Ok(total_result)
    }

    /// Scrape a single plugin
    pub fn scrape_plugin(&mut self, plugin: &Plugin) -> Result<ScrapeResult> {
        self.begin_index_write();

        let files = self.discover_files(plugin)?;

        let mut result = ScrapeResult {
            sessions_scraped: 0,
            sessions_indexed: 0,
            events_written: 0,
            errors: Vec::new(),
            files_processed: 0,
            files_skipped: 0,
            agent_types: Vec::new(),
        };

        for file_path in files {
            let path_str = file_path.to_str().unwrap_or("");

            // Sources with a rolling-window truncation_limit (e.g. Windsurf's 20-conversation
            // cap) can silently overwrite old conversations without shrinking the file.  Clear
            // the per-file state before each scrape so we always get a fresh full-read and
            // never leave stale session files from overwritten conversations.
            if plugin.source.truncation_limit.is_some() {
                let _ = self.state_manager.remove_file(path_str);
            }

            // Check if file needs scraping
            match self.state_manager.needs_rescrape(&file_path, &plugin.plugin.name) {
                Ok(true) => {
                    // Check if truncated (physical file shrink)
                    let file_state = self.state_manager.get_file_state(path_str);
                    if let Some(state) = file_state {
                        let metadata = std::fs::metadata(&file_path)?;
                        if metadata.len() < state.last_byte_offset {
                            // File was truncated - remove state and rescan fully
                            self.state_manager.remove_file(path_str)?;
                        }
                    }

                    match self.scrape_file(&file_path, plugin) {
                        Ok(file_result) => {
                            result.sessions_scraped += file_result.sessions_scraped;
                            result.sessions_indexed += file_result.sessions_indexed;
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

        // Populate agent type if any sessions were scraped for this plugin
        if result.sessions_scraped > 0 {
            result.agent_types.push(plugin.plugin.name.clone());
        }

        self.end_index_write();

        Ok(result)
    }

    /// Scrape a single file
    pub fn scrape_file(&mut self, file_path: &Path, plugin: &Plugin) -> Result<ScrapeResult> {
        self.begin_index_write();

        let parser: Box<dyn FormatParser> = match plugin.source.format {
            LogFormat::Jsonl => Box::new(JsonlParser),
            LogFormat::Markdown => Box::new(MarkdownParser),
            LogFormat::JsonTree => Box::new(JsonTreeParser),
            LogFormat::Sqlite => Box::new(SqliteParser),
        };

        // Detect sessions in the file
        let sessions = parser.detect_sessions(file_path, plugin)?;

        // Detect project path for this file
        let project = self.detect_project(file_path, plugin)?;

        let path_str = file_path.to_str().unwrap_or("");

        let mut result = ScrapeResult {
            sessions_scraped: 0,
            sessions_indexed: 0,
            events_written: 0,
            errors: Vec::new(),
            files_processed: 1,
            files_skipped: 0,
            agent_types: Vec::new(),
        };

        // Parse all events once.  For multi-session files (e.g. Cursor/Windsurf
        // with key_session_id_regex) each event already carries the correct
        // session_id set by the parser; we filter below rather than re-parsing
        // the source for every session.
        let all_events: Vec<Event> = match parser.parse(file_path, plugin) {
            Ok(events) => events,
            Err(e) => {
                if e.is_skippable() {
                    result.errors.push(ScrapeError {
                        file: file_path.display().to_string(),
                        line: None,
                        message: e.to_string(),
                    });
                    Vec::new()
                } else {
                    self.end_index_write();
                    return Err(e);
                }
            }
        };

        let multi_session = sessions.len() > 1;

        for session_info in sessions {
            let prefixed_session_id = format!("{}/{}", plugin.plugin.name, session_info.session_id);

            // Detect model for this session
            let model = self.detect_model(file_path, &session_info, plugin)?;

            // Select events that belong to this session.
            // For single-session sources every event goes to the one session.
            // For multi-session sources (key_session_id_regex) filter by session_id.
            let mut events: Vec<Event> = if multi_session {
                all_events
                    .iter()
                    .filter(|e| e.session_id == session_info.session_id)
                    .cloned()
                    .collect()
            } else {
                all_events.clone()
            };

            if events.is_empty() {
                continue;
            }

            // Enrich events with project, model, and file paths
            for event in &mut events {
                // Set project
                if event.project.is_none() {
                    event.project = project.clone();
                }

                // Set model
                if event.model.is_none() {
                    event.model = model.clone();
                }

                // Extract file paths
                if event.file_paths.is_empty() {
                    event.file_paths = FilePathExtractor::extract_from_event(event, plugin);
                }
            }

            // Write session to file
            let session_path = self.sessions_dir
                .join(&plugin.plugin.name)
                .join(format!("{}.jsonl", session_info.session_id));

            // Create plugin directory if needed
            std::fs::create_dir_all(session_path.parent().unwrap())?;

            match Self::write_session(&session_path, &events, plugin) {
                Ok(_) => {
                    result.sessions_scraped += 1;
                    result.events_written += events.len();

                    // Track session in state
                    self.state_manager.add_session(path_str, prefixed_session_id.clone())?;

                    if self.index_session_events(
                        &events,
                        &prefixed_session_id,
                        &plugin.plugin.name,
                        project.as_deref(),
                        model.as_deref(),
                    ) {
                        result.sessions_indexed += 1;
                    }
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

        // Update file offset state
        let metadata = std::fs::metadata(file_path)?;
        self.state_manager.set_offset(path_str, metadata.len())?;
        self.state_manager.set_modified(path_str, Utc::now())?;

        if result.sessions_scraped > 0 {
            info!(
                file = %file_path.display(),
                sessions_scraped = result.sessions_scraped,
                sessions_indexed = result.sessions_indexed,
                "scrape complete"
            );
        }

        self.end_index_write();

        Ok(result)
    }

    /// Detect project path for a file
    fn detect_project(&self, file_path: &Path, plugin: &Plugin) -> Result<Option<String>> {
        let detection = plugin.parser.project.as_ref()
            .unwrap_or(&crate::plugin::ProjectDetection::ParentDir);

        match detection {
            ProjectDetection::ParentDir => {
                // Get parent directory of the log file
                if let Some(parent) = file_path.parent() {
                    Ok(Some(parent.to_string_lossy().to_string()))
                } else {
                    Ok(None)
                }
            }
            ProjectDetection::GitRoot => {
                // Use git rev-parse to find the git root
                if let Ok(output) = Command::new("git")
                    .args(["rev-parse", "--show-toplevel"])
                    .current_dir(file_path.parent().unwrap_or(file_path))
                    .output()
                {
                    if output.status.success() {
                        let git_root = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        return Ok(Some(git_root));
                    }
                }
                // Fallback to parent dir
                if let Some(parent) = file_path.parent() {
                    Ok(Some(parent.to_string_lossy().to_string()))
                } else {
                    Ok(None)
                }
            }
            ProjectDetection::Field { field: _ } => {
                // For field-based detection, we need to extract from the first event
                // This is handled in the parser, return None here
                Ok(None)
            }
        }
    }

    /// Detect model for a session
    fn detect_model(&self, _file_path: &Path, session_info: &crate::parser::SessionInfo, plugin: &Plugin) -> Result<Option<String>> {
        let detection = plugin.parser.model.as_ref()
            .unwrap_or(&crate::plugin::ModelDetection::None);

        match detection {
            ModelDetection::Static { value } => {
                Ok(Some(value.clone()))
            }
            ModelDetection::None => Ok(None),
            ModelDetection::Metadata { field } | ModelDetection::Event { field } => {
                // Try to extract from session metadata
                if let Some(ref metadata) = session_info.metadata {
                    if let Some(value) = self.extract_field_recursive(metadata, field) {
                        if let Some(s) = value.as_str() {
                            return Ok(Some(s.to_string()));
                        }
                    }
                }

                // For metadata files, try to read them
                if let ModelDetection::Metadata { .. } = detection {
                    if let Some(ref metadata_config) = plugin.metadata {
                        let session_id = &session_info.session_id;
                        let meta_path_str = metadata_config.session_meta.as_ref()
                            .map(|p| p.replace("{session_id}", session_id))
                            .unwrap_or_else(|| String::new());

                        if !meta_path_str.is_empty() {
                            let expanded = shellexpand::full(&meta_path_str)
                                .unwrap_or_default().into_owned();
                            let meta_path = PathBuf::from(expanded.as_str());

                            if meta_path.exists() {
                                if let Ok(content) = std::fs::read_to_string(&meta_path) {
                                    if let Ok(json) = serde_json::from_str::<Value>(&content) {
                                        if let Some(value) = self.extract_field_recursive(&json, field) {
                                            if let Some(s) = value.as_str() {
                                                return Ok(Some(s.to_string()));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                Ok(None)
            }
        }
    }

    /// Extract a field from JSON using dot notation (recursive helper)
    fn extract_field_recursive(&self, value: &Value, path: &str) -> Option<Value> {
        let mut current = value;
        for part in path.split('.') {
            current = current.get(part)?;
        }
        Some(current.clone())
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

/// Attempt to git-commit newly scraped sessions.
///
/// Called from the CLI after a successful scrape when `[scrape] git_auto_commit = true`.
/// Silently skips if the data directory is not inside a git repository or nothing new was
/// scraped. Returns `Ok(true)` when a commit was created, `Ok(false)` when skipped.
pub fn git_auto_commit(data_dir: &Path, result: &ScrapeResult) -> Result<bool> {
    if result.sessions_scraped == 0 {
        return Ok(false);
    }

    // Resolve git root — skip silently if data_dir is not tracked by git.
    let git_top = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(data_dir)
        .output();

    let git_root = match git_top {
        Ok(out) if out.status.success() => {
            PathBuf::from(String::from_utf8_lossy(&out.stdout).trim().to_string())
        }
        _ => {
            debug!("git_auto_commit: data_dir is not inside a git repo, skipping");
            return Ok(false);
        }
    };

    let sessions_dir = data_dir.join("sessions");

    // Stage the sessions directory (use absolute path so it works regardless of cwd).
    let add_out = Command::new("git")
        .args(["add", sessions_dir.to_str().unwrap_or("sessions")])
        .current_dir(&git_root)
        .output()?;

    if !add_out.status.success() {
        warn!(
            stderr = %String::from_utf8_lossy(&add_out.stderr),
            "git_auto_commit: git add failed"
        );
        return Ok(false);
    }

    // Build a descriptive commit message.
    let agents = if result.agent_types.is_empty() {
        "unknown".to_string()
    } else {
        result.agent_types.join(", ")
    };
    let msg = format!(
        "agentscribe: scraped {} session(s) ({})",
        result.sessions_scraped, agents
    );

    let commit_out = Command::new("git")
        .args(["commit", "-m", &msg])
        .current_dir(&git_root)
        .output()?;

    if commit_out.status.success() {
        info!(message = %msg, "git auto-commit created");
        return Ok(true);
    }

    let stderr = String::from_utf8_lossy(&commit_out.stderr);
    if stderr.contains("nothing to commit") || stderr.contains("nothing added to commit") {
        debug!("git_auto_commit: nothing new to commit");
    } else {
        warn!(stderr = %stderr, "git_auto_commit: git commit failed");
    }
    Ok(false)
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

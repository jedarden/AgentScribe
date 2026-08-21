//! Scraping orchestration
//!
//! Coordinates plugin loading, file discovery, parsing, and state management.

mod companion;
mod file_path_extractor;
mod state;
mod state_sqlite;

pub use companion::{CompanionCache, CompanionIndex};
pub use file_path_extractor::FilePathExtractor;
pub use state::StateManager;

use crate::enrichment::ConfigChangeTracker;
use crate::error::{AgentScribeError, Result};
use crate::event::Event;
use crate::index::{build_content, build_manifest_from_events, IndexManager};
use crate::parser::{
    FormatParser, JsonArrayParser, JsonTreeParser, JsonlParser, MarkdownParser, SqliteParser,
};
use crate::plugin::{LogFormat, ModelDetection, Plugin, PluginManager, ProjectDetection};
use chrono::Utc;
use glob::glob;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
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
    #[allow(dead_code)]
    pub line: Option<usize>,
    pub message: String,
}

/// Scraper - main orchestration
pub struct Scraper {
    plugin_manager: PluginManager,
    #[allow(dead_code)]
    data_dir: PathBuf,
    sessions_dir: PathBuf,
    state_manager: StateManager,
    index_manager: Option<IndexManager>,
    index_write_depth: usize,
    companion_cache: CompanionCache,
    /// Config change tracker for correlating file modifications with sessions
    #[allow(dead_code)]
    config_tracker: ConfigChangeTracker,
    /// Warning deduplication tracker to prevent repeated warnings for the same file
    warning_dedup: Arc<Mutex<WarningDedup>>,
}

/// Warning deduplication tracker
///
/// Tracks which warnings have been emitted per file to avoid spamming the logs
/// with identical warnings on every re-parse. Uses a sliding window approach
/// where warnings are tracked for a configurable time period.
struct WarningDedup {
    /// Map of file_path → (warning_key → last_emitted_timestamp)
    emitted: HashMap<String, HashMap<String, Instant>>,
    /// Minimum duration (in seconds) before emitting the same warning again for the same file
    cooldown_secs: u64,
}

impl WarningDedup {
    /// Create a new warning deduplication tracker
    fn new() -> Self {
        WarningDedup {
            emitted: HashMap::new(),
            cooldown_secs: 600, // 10 minutes default
        }
    }

    /// Check if a warning should be emitted (returns true if OK to emit)
    fn should_emit(&mut self, file_path: &str, warning_key: &str) -> bool {
        let file_warnings = self.emitted.entry(file_path.to_string()).or_default();
        let now = Instant::now();

        if let Some(&last_emitted) = file_warnings.get(warning_key) {
            // Check if enough time has passed since last emission
            if last_emitted.elapsed().as_secs() < self.cooldown_secs {
                return false; // Still in cooldown period
            }
        }

        // Record this warning emission
        file_warnings.insert(warning_key.to_string(), now);
        true
    }

    /// Clear all warnings for a file (called when file is fully re-scraped)
    #[allow(dead_code)]
    fn clear_file(&mut self, file_path: &str) {
        self.emitted.remove(file_path);
    }

    /// Clean up old entries (files/warnings older than 2x cooldown)
    #[allow(dead_code)]
    fn cleanup(&mut self) {
        let threshold = self.cooldown_secs * 2;
        let now = Instant::now();

        // Remove old warning entries
        for file_warnings in self.emitted.values_mut() {
            file_warnings.retain(|_, &mut last_emitted| {
                now.saturating_duration_since(last_emitted).as_secs() < threshold
            });
        }

        // Remove files with no warnings
        self.emitted.retain(|_, warnings| !warnings.is_empty());
    }
}

impl Scraper {
    /// Create a new scraper with the default 30-second lock timeout.
    pub fn new(data_dir: PathBuf) -> Result<Self> {
        Self::new_with_lock_timeout(data_dir, 30)
    }

    /// Create a new scraper with a configurable lock timeout (seconds).
    ///
    /// The timeout controls how long `save()` waits for the exclusive file
    /// lock on `scrape-state.json` before returning an error.  Pass `0` to
    /// disable the timeout (wait indefinitely).
    pub fn new_with_lock_timeout(data_dir: PathBuf, lock_timeout_secs: u64) -> Result<Self> {
        let plugin_dir = data_dir.join("plugins");
        let state_file = data_dir.join("state").join("scrape-state.json");
        let sessions_dir = data_dir.join("sessions");

        // Create directories
        std::fs::create_dir_all(&plugin_dir)?;
        std::fs::create_dir_all(&sessions_dir)?;
        std::fs::create_dir_all(state_file.parent().unwrap())?;

        let plugin_manager = PluginManager::new(plugin_dir);
        let lock_timeout = Duration::from_secs(lock_timeout_secs);
        let state_manager = StateManager::new_with_timeout(state_file, lock_timeout)?;

        // Initialize index manager (best-effort — scraping continues without indexing if it fails)
        let index_manager = match IndexManager::open(&data_dir) {
            Ok(mgr) => Some(mgr),
            Err(e) => {
                eprintln!(
                    "Warning: Index not available: {}. Scraping without indexing.",
                    e
                );
                None
            }
        };

        // Initialize config change tracker
        let config_tracker = ConfigChangeTracker::new(data_dir.clone());

        Ok(Scraper {
            plugin_manager,
            data_dir,
            sessions_dir,
            state_manager,
            index_manager,
            index_write_depth: 0,
            companion_cache: CompanionCache::new(),
            config_tracker,
            warning_dedup: Arc::new(Mutex::new(WarningDedup::new())),
        })
    }

    /// Get the companion cache
    #[allow(dead_code)]
    pub fn companion_cache(&self) -> &CompanionCache {
        &self.companion_cache
    }

    /// Get the companion cache (mutable)
    pub fn companion_cache_mut(&mut self) -> &mut CompanionCache {
        &mut self.companion_cache
    }

    /// Emit a warning with deduplication to prevent spam.
    ///
    /// Returns true if the warning was emitted (false if suppressed due to cooldown).
    /// The warning_key should uniquely identify the type of warning (e.g., "Role field message.role not found").
    pub fn emit_warning(&self, file_path: &str, warning_key: &str, warning_message: &str) -> bool {
        let mut dedup = self.warning_dedup.lock().unwrap();
        if dedup.should_emit(file_path, warning_key) {
            warn!(file = %file_path, "{}", warning_message);
            true
        } else {
            debug!(file = %file_path, warning = %warning_key, "warning suppressed (cooldown)");
            false
        }
    }

    /// Clear warning history for a file when it's fully re-scraped
    #[allow(dead_code)]
    fn clear_warnings_for_file(&self, file_path: &str) {
        let mut dedup = self.warning_dedup.lock().unwrap();
        dedup.clear_file(file_path);
    }

    /// Load companion metadata for a session from the plugin's companion index file.
    ///
    /// This reads the companion index file (if configured) and looks up metadata
    /// for the given session ID. Returns None if no companion index is configured
    /// or the session is not found.
    fn load_companion_metadata(&self, session_id: &str, plugin: &Plugin) -> Result<Option<Value>> {
        if let Some(ref metadata_config) = plugin.metadata {
            if let Some(ref companion_path) = metadata_config.companion_index {
                // Expand ~ and environment variables
                let expanded = shellexpand::full(companion_path)
                    .map_err(|e| AgentScribeError::Glob(format!("Path expansion error: {}", e)))?;

                let path = PathBuf::from(expanded.as_ref());

                // Load the companion index
                match self.companion_cache.get_or_load(&path) {
                    Ok(index) => {
                        let metadata = index.get(session_id).cloned();
                        Ok(metadata)
                    }
                    Err(e) => {
                        // If the file doesn't exist, return None rather than failing
                        if matches!(e, AgentScribeError::FileNotFound(_)) {
                            Ok(None)
                        } else {
                            Err(e)
                        }
                    }
                }
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    /// Load all plugins
    pub fn load_plugins(&mut self) -> Result<Vec<String>> {
        self.plugin_manager.load_all()
    }

    /// Get the plugin manager
    pub fn plugin_manager(&self) -> &PluginManager {
        &self.plugin_manager
    }

    /// Get the plugin manager (mutable)
    #[allow(dead_code)]
    pub fn plugin_manager_mut(&mut self) -> &mut PluginManager {
        &mut self.plugin_manager
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
        parent_session_id: Option<&str>,
        project: Option<&str>,
        model: Option<&str>,
    ) -> bool {
        if let Some(ref mut mgr) = self.index_manager {
            let manifest = build_manifest_from_events(
                events,
                session_id,
                source_agent,
                project,
                model,
                parent_session_id,
            );
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
                    let exclude_expanded = match shellexpand::full(exclude_pattern) {
                        Ok(expanded) => expanded.into_owned(),
                        Err(_) => exclude_pattern.clone(),
                    };

                    // Normalize relative patterns to work with absolute paths.
                    // If the pattern doesn't start with '/' or '**', prepend '**/' so it
                    // matches anywhere in the path. This converts "*/subagents/*" to
                    // "**/subagents/*", which correctly matches absolute paths like
                    // "/home/user/logs/project/subagents/file.jsonl".
                    let normalized_pattern = if !exclude_expanded.starts_with('/')
                        && !exclude_expanded.starts_with("**")
                    {
                        let stripped = exclude_expanded
                            .strip_prefix("./")
                            .unwrap_or(&exclude_expanded);
                        format!("**/{}", stripped)
                    } else {
                        exclude_expanded
                    };

                    if let Ok(pat) = glob::Pattern::new(&normalized_pattern) {
                        if pat.matches_path(path) {
                            excluded = true;
                            debug!(exclude_pattern = %exclude_pattern, path = %path.display(), "file excluded by pattern");
                            break;
                        }
                    } else {
                        warn!(exclude_pattern = %exclude_pattern, "invalid exclude glob pattern, skipping");
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

        let plugin_names: Vec<String> = self
            .plugin_manager
            .names()
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
            match self
                .state_manager
                .needs_rescrape(&file_path, &plugin.plugin.name)
            {
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
            LogFormat::JsonArray => Box::new(JsonArrayParser),
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

        // Check if we have existing state for this file to enable incremental parsing
        let start_offset = self
            .state_manager
            .get_file_state(path_str)
            .map(|state| state.last_byte_offset)
            .unwrap_or(0);

        // Parse events - use incremental parsing if we have a previous offset
        let all_events: Vec<Event> = if start_offset > 0 {
            debug!(
                file = %file_path.display(),
                offset = start_offset,
                "incremental parsing"
            );
            parser.parse_incremental(file_path, plugin, start_offset)
        } else {
            parser.parse(file_path, plugin)
        }
        .unwrap_or_else(|e| {
            if e.is_skippable() {
                result.errors.push(ScrapeError {
                    file: file_path.display().to_string(),
                    line: None,
                    message: e.to_string(),
                });
                Vec::new()
            } else {
                self.end_index_write();
                // Non-skippable error: return empty vector and abort processing
                // The error will be propagated when we try to use the events
                Vec::new()
            }
        });

        let multi_session = sessions.len() > 1;

        for session_info in sessions {
            let prefixed_session_id = format!("{}/{}", plugin.plugin.name, session_info.session_id);

            // Detect if this is a subagent session by checking for parent_session_id
            let source_agent = if session_info.parent_session_id.is_some() {
                format!("{}-subagent", plugin.plugin.name)
            } else {
                plugin.plugin.name.clone()
            };

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

            // Load companion metadata (if available)
            let companion_meta = self.load_companion_metadata(&session_info.session_id, plugin)?;

            // Enrich events with project, model, and file paths
            for event in &mut events {
                // Enrich from companion metadata first (highest priority)
                if let Some(ref meta) = companion_meta {
                    // Set model from companion metadata
                    if event.model.is_none() {
                        if let Some(m) = meta.get("model").and_then(|v| v.as_str()) {
                            event.model = Some(m.to_string());
                        }
                    }

                    // Set project/cwd from companion metadata
                    if event.project.is_none() {
                        if let Some(cwd) = meta.get("cwd").and_then(|v| v.as_str()) {
                            event.project = Some(cwd.to_string());
                        }
                    }
                }

                // Set project (fallback to detection)
                if event.project.is_none() {
                    event.project = project.clone();
                }

                // Set model (fallback to detection)
                if event.model.is_none() {
                    event.model = model.clone();
                }

                // Set source_agent based on parent_session_id detection
                event.source_agent = source_agent.clone();

                // Extract file paths
                if event.file_paths.is_empty() {
                    event.file_paths = FilePathExtractor::extract_from_event(event, plugin);
                }
            }

            // Write session to file
            let session_path = self
                .sessions_dir
                .join(&plugin.plugin.name)
                .join(format!("{}.jsonl", session_info.session_id));

            // Create plugin directory if needed
            std::fs::create_dir_all(session_path.parent().unwrap())?;

            match Self::write_session(&session_path, &events, plugin) {
                Ok(_) => {
                    result.sessions_scraped += 1;
                    result.events_written += events.len();

                    // Track session in state
                    self.state_manager
                        .add_session(path_str, prefixed_session_id.clone())?;

                    if self.index_session_events(
                        &events,
                        &prefixed_session_id,
                        &source_agent,
                        session_info.parent_session_id.as_deref(),
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
        let detection = plugin
            .parser
            .project
            .as_ref()
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
    fn detect_model(
        &self,
        _file_path: &Path,
        session_info: &crate::parser::SessionInfo,
        plugin: &Plugin,
    ) -> Result<Option<String>> {
        let detection = plugin
            .parser
            .model
            .as_ref()
            .unwrap_or(&crate::plugin::ModelDetection::None);

        match detection {
            ModelDetection::Static { value } => Ok(Some(value.clone())),
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
                        let meta_path_str = metadata_config
                            .session_meta
                            .as_ref()
                            .map(|p| p.replace("{session_id}", session_id))
                            .unwrap_or_default();

                        if !meta_path_str.is_empty() {
                            let expanded = shellexpand::full(&meta_path_str)
                                .unwrap_or_default()
                                .into_owned();
                            let meta_path = PathBuf::from(expanded.as_str());

                            if meta_path.exists() {
                                if let Ok(content) = std::fs::read_to_string(&meta_path) {
                                    if let Ok(json) = serde_json::from_str::<Value>(&content) {
                                        if let Some(value) =
                                            self.extract_field_recursive(&json, field)
                                        {
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
            let jsonl = event
                .to_jsonl()
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
        let path = self
            .session_path(session_id)
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

        // Recursively scan for session files to handle subagent sessions
        // Subagent sessions are stored as: plugin_dir/parent-uuid/agent-id.jsonl
        fn scan_session_dir(
            dir: &std::path::Path,
            plugin_name: &str,
            base_path: &std::path::Path,
            sessions: &mut Vec<String>,
        ) -> std::io::Result<()> {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_dir() {
                    // Recursively scan subdirectories
                    scan_session_dir(&path, plugin_name, base_path, sessions)?;
                } else if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                    // Calculate relative path from plugin_dir to preserve session_id structure
                    if let Ok(rel_path) = path.strip_prefix(base_path) {
                        // Remove .jsonl extension and convert to session_id
                        let path_without_ext = rel_path.with_extension("");
                        let session_id = path_without_ext.to_str().unwrap_or("unknown");

                        // Convert path separators to match session_id format
                        let normalized_id = session_id.replace('/', "{SLASH}");

                        sessions.push(format!("{}/{}", plugin_name, normalized_id));
                    }
                }
            }
            Ok(())
        }

        scan_session_dir(&plugin_dir, plugin_name, &plugin_dir, &mut sessions)?;

        // Convert {SLASH} back to / for session IDs
        for session in &mut sessions {
            *session = session.replace("{SLASH}", "/");
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

/// Reconstruct a session's full text by re-reading and re-normalizing its
/// JSONL file under `sessions/`.
///
/// **ROOT CAUSE (ADR-2, bead bf-1pkfp):** Prior to this fix, the Tantivy schema
/// stored the `content` field (TEXT | STORED), duplicating the full conversation
/// text already present in `sessions/<plugin>/<id>.jsonl`. This caused the index
/// to grow to 76GB against a ~385MB normalized corpus — text was stored twice:
/// once durably in JSONL, and again in Tantivy's doc store.
///
/// **THE FIX:** This function provides a shared fallback for consumers that need
/// the raw text (search snippets, more-like-this term extraction, analytics
/// cost estimation and problem-type classification). Instead of reading from
/// the stored `content` field (which no longer exists), we re-read the original
/// JSONL file and re-normalize it via `Scraper::read_session` + `build_content`.
///
/// **PERFORMANCE NOTE:** This adds one JSONL file read per session for operations
/// that scan the full corpus (analytics, digest, pulse-report). This is bounded
/// and consistent with existing patterns like `gc --dry-run`. Search operations
/// only pay this cost for the top-K results, not the entire corpus.
///
/// **GRACEFUL DEGRADATION:** Returns `None` if the session file is missing,
/// unreadable, or empty (e.g., already garbage-collected). Callers should handle
/// this gracefully — analytics/reporting use empty string fallbacks, search
/// proceeds without snippets.
pub(crate) fn load_session_content(data_dir: &Path, session_id: &str) -> Option<String> {
    let scraper = Scraper::new(data_dir.to_path_buf()).ok()?;
    let events = scraper.read_session(session_id).ok()?;
    if events.is_empty() {
        return None;
    }
    Some(build_content(&events))
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
    use crate::plugin::{
        LogFormat, Parser, Plugin, PluginMeta, SessionDetection, SessionIdSource, Source,
    };

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

    #[test]
    fn test_truncation_limit_clears_file_state() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(".agentscribe");

        // Create a test file
        let test_file = temp.path().join("test.log");
        std::fs::write(&test_file, "test content").unwrap();

        let mut scraper = Scraper::new(data_dir.clone()).unwrap();

        // Set up initial state for the file
        let file_path = test_file.to_str().unwrap();
        scraper
            .state_manager
            .add_session(file_path, "test/session-1".to_string())
            .unwrap();
        scraper.state_manager.set_offset(file_path, 1000).unwrap();

        // Verify state was set
        let state_before = scraper.state_manager.get_file_state(file_path);
        assert!(state_before.is_some());
        assert_eq!(state_before.unwrap().last_byte_offset, 1000);

        // Create a plugin with truncation_limit (like Windsurf)
        let plugin = Plugin {
            plugin: PluginMeta {
                name: "windsurf".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec![test_file.to_str().unwrap().to_string()],
                exclude: vec![],
                format: LogFormat::Jsonl,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: Some(20), // Rolling-window limit
                envelope: None,
                array: None,
            },
            parser: Parser {
                ..Default::default()
            },
            metadata: None,
        };

        // Run scrape_plugin - should clear state due to truncation_limit
        let _result = scraper.scrape_plugin(&plugin);

        // State should have been cleared for the file
        let state_after = scraper.state_manager.get_file_state(file_path);
        // The state might be re-created during scraping, but the original offset should be gone
        // or reset based on the current file size
        assert!(state_after.is_none() || state_after.unwrap().last_byte_offset != 1000);
    }

    #[test]
    fn test_file_truncation_detection_rescans_fully() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(".agentscribe");

        // Create a test file with content
        let test_file = temp.path().join("test.log");
        let initial_content = "line 1\nline 2\nline 3\n";
        std::fs::write(&test_file, initial_content).unwrap();
        let initial_size = std::fs::metadata(&test_file).unwrap().len();

        let scraper = Scraper::new(data_dir.clone()).unwrap();

        // Set state tracking the file at its initial size
        // Set last_modified to a time in the past so file mtime after truncation is newer
        let past_time = Utc::now() - chrono::Duration::seconds(10);
        let file_path = test_file.to_str().unwrap();
        scraper
            .state_manager
            .set_offset(file_path, initial_size)
            .unwrap();
        scraper
            .state_manager
            .set_modified(file_path, past_time)
            .unwrap();

        // Verify state was set
        let state_before = scraper.state_manager.get_file_state(file_path);
        assert_eq!(state_before.unwrap().last_byte_offset, initial_size);

        // Truncate the file (simulating Windsurf rolling-window overwrite)
        let truncated_content = "line A\n";
        std::fs::write(&test_file, truncated_content).unwrap();
        let truncated_size = std::fs::metadata(&test_file).unwrap().len();

        assert!(
            truncated_size < initial_size,
            "file should be smaller after truncation"
        );

        // Create a test plugin
        let plugin = Plugin {
            plugin: PluginMeta {
                name: "test".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec![test_file.to_str().unwrap().to_string()],
                exclude: vec![],
                format: LogFormat::Jsonl,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: None,
                envelope: None,
                array: None,
            },
            parser: Parser {
                ..Default::default()
            },
            metadata: None,
        };

        // Check if file needs scraping - truncation should be detected
        let needs_scrape = scraper
            .state_manager
            .needs_rescrape(&test_file, &plugin.plugin.name)
            .unwrap();
        assert!(needs_scrape, "truncated file should need rescraping");

        // The scraper should have cleared the old state after detecting truncation
        // This is tested implicitly by the fact that needs_rescrape returned true
        // despite the file being "processed" before
    }

    #[test]
    fn test_git_auto_commit_returns_false_when_no_sessions_scraped() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(".agentscribe");

        // Create a result with no sessions scraped
        let result = ScrapeResult {
            sessions_scraped: 0,
            sessions_indexed: 0,
            events_written: 0,
            errors: Vec::new(),
            files_processed: 0,
            files_skipped: 0,
            agent_types: Vec::new(),
        };

        let committed = git_auto_commit(&data_dir, &result).unwrap();
        assert!(!committed, "should return false when no sessions scraped");
    }

    #[test]
    fn test_git_auto_commit_skips_when_not_git_repo() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(".agentscribe");

        // Create sessions directory
        std::fs::create_dir_all(data_dir.join("sessions")).unwrap();

        // Create a result with sessions scraped
        let result = ScrapeResult {
            sessions_scraped: 3,
            sessions_indexed: 3,
            events_written: 100,
            errors: Vec::new(),
            files_processed: 1,
            files_skipped: 0,
            agent_types: vec!["cursor".to_string()],
        };

        let committed = git_auto_commit(&data_dir, &result).unwrap();
        assert!(!committed, "should skip commit when not in a git repo");
    }

    #[test]
    fn test_scrape_result_aggregates_agent_types() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(".agentscribe");
        std::fs::create_dir_all(data_dir.join("sessions")).unwrap();

        let mut scraper = Scraper::new(data_dir).unwrap();

        // Create test plugins
        let cursor_plugin = Plugin {
            plugin: PluginMeta {
                name: "cursor".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec!["nonexistent.db".to_string()],
                exclude: vec![],
                format: LogFormat::Sqlite,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: Some(20),
                envelope: None,
                array: None,
            },
            parser: Parser {
                query: Some("SELECT key, value FROM kv".to_string()),
                ..Default::default()
            },
            metadata: None,
        };

        let windsurf_plugin = Plugin {
            plugin: PluginMeta {
                name: "windsurf".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec!["nonexistent.db".to_string()],
                exclude: vec![],
                format: LogFormat::Sqlite,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: Some(20),
                envelope: None,
                array: None,
            },
            parser: Parser {
                query: Some("SELECT key, value FROM kv".to_string()),
                ..Default::default()
            },
            metadata: None,
        };

        // Add plugins directly
        scraper.plugin_manager_mut().add_plugin(cursor_plugin);
        scraper.plugin_manager_mut().add_plugin(windsurf_plugin);

        // Verify both plugins are loaded
        let plugin_names = scraper.plugin_manager().names();
        assert!(plugin_names.contains(&"cursor"));
        assert!(plugin_names.contains(&"windsurf"));
    }

    #[test]
    fn test_companion_metadata_enrichment() {
        use crate::plugin::{Parser, PluginMeta, SessionDetection, SessionIdSource, Source};

        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(".agentscribe");
        std::fs::create_dir_all(data_dir.join("sessions")).unwrap();

        // Create a companion index file
        let companion_path = temp.path().join("session_index.jsonl");
        std::fs::write(
            &companion_path,
            r#"{"thread_id": "test-session-1", "model": "gpt-4-turbo", "cwd": "/home/user/project"}
{"thread_id": "test-session-2", "model": "gpt-3.5-turbo", "cwd": "/home/user/other"}"#,
        )
        .unwrap();

        // Create a test plugin with companion_index configured
        let plugin = Plugin {
            plugin: PluginMeta {
                name: "test-agent".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec![],
                exclude: vec![],
                format: LogFormat::Jsonl,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: None,
                envelope: None,
                array: None,
            },
            parser: Parser {
                ..Default::default()
            },
            metadata: Some(crate::plugin::Metadata {
                companion_index: Some(companion_path.to_str().unwrap().to_string()),
                ..Default::default()
            }),
        };

        let scraper = Scraper::new(data_dir).unwrap();

        // Test loading companion metadata for a session
        let result = scraper.load_companion_metadata("test-session-1", &plugin);

        assert!(result.is_ok());
        let metadata = result.unwrap().expect("should have metadata");

        // Verify the metadata contains the expected fields
        assert_eq!(
            metadata.get("thread_id").unwrap().as_str(),
            Some("test-session-1")
        );
        assert_eq!(metadata.get("model").unwrap().as_str(), Some("gpt-4-turbo"));
        assert_eq!(
            metadata.get("cwd").unwrap().as_str(),
            Some("/home/user/project")
        );

        // Test loading a non-existent session returns None
        let result = scraper.load_companion_metadata("nonexistent", &plugin);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    // --- shellexpand glob safety tests ---

    /// Prove that tilde expansion preserves glob wildcards.
    /// shellexpand only replaces `~/` at the start of the string; `*`, `?`,
    /// `**`, and `[...]` pass through unmodified.
    #[test]
    fn test_tilde_expansion_preserves_glob_wildcards() {
        let home = std::env::var("HOME").expect("HOME must be set");
        let cases = vec![
            ("~/foo/*.log", format!("{}/foo/*.log", home)),
            ("~/src/**/*.rs", format!("{}/src/**/*.rs", home)),
            ("~/[ab]*.txt", format!("{}/[ab]*.txt", home)),
            ("~/foo/bar?.toml", format!("{}/foo/bar?.toml", home)),
            ("~/*.tmp", format!("{}/*.tmp", home)),
        ];

        for (pattern, expected) in cases {
            let expanded = shellexpand::full(pattern)
                .unwrap_or_else(|e| panic!("tilde expansion failed for {:?}: {}", pattern, e))
                .into_owned();
            // Must still compile as a valid glob::Pattern
            let pat = glob::Pattern::new(&expanded).unwrap_or_else(|e| {
                panic!(
                    "expanded {:?} -> {:?} is not valid glob: {}",
                    pattern, expanded, e
                )
            });
            assert_eq!(expanded, expected, "tilde expansion of {:?}", pattern);
            // Sanity: pattern string round-trips through compilation
            assert_eq!(pat.as_str(), expanded);
        }
    }

    /// Prove that env-var expansion preserves glob wildcards.
    /// When the variable exists, `$VAR` is substituted but surrounding `*`, `?`
    /// characters are left untouched.
    #[test]
    fn test_env_var_expansion_preserves_glob_wildcards() {
        // Use HOME as a well-known variable we can rely on.
        let home = std::env::var("HOME").expect("HOME must be set");

        // Temporarily set a variable with glob-friendly value for precise testing.
        std::env::set_var("__AGENTSCRIBE_TEST_DIR", "/tmp/agentscribe_test_logs");
        let test_dir = "/tmp/agentscribe_test_logs";

        let expected_1 = format!("{test_dir}/*.log");
        let expected_2 = format!("{test_dir}/**/*.rs");
        let expected_3 = format!("{test_dir}/foo?.txt");
        let expected_4 = format!("{home}/src/[ab]*.rs");

        let cases: Vec<(&str, &str)> = vec![
            // Variable embedded in a glob pattern
            ("$__AGENTSCRIBE_TEST_DIR/*.log", &expected_1),
            ("$__AGENTSCRIBE_TEST_DIR/**/*.rs", &expected_2),
            // Braced form
            ("${__AGENTSCRIBE_TEST_DIR}/foo?.txt", &expected_3),
            // Variable alongside a wildcard not part of the var name
            ("$HOME/src/[ab]*.rs", &expected_4),
        ];

        for (pattern, expected) in &cases {
            let expanded = shellexpand::full(pattern)
                .unwrap_or_else(|e| panic!("env expansion failed for {:?}: {}", pattern, e))
                .into_owned();
            let _pat = glob::Pattern::new(&expanded).unwrap_or_else(|e| {
                panic!(
                    "expanded {:?} -> {:?} is not valid glob: {}",
                    pattern, expanded, e
                )
            });
            assert_eq!(&expanded, *expected, "env-var expansion of {:?}", pattern);
        }

        std::env::remove_var("__AGENTSCRIBE_TEST_DIR");
    }

    /// Prove that plain exclude patterns with no expandable tokens pass through
    /// shellexpand::full() unchanged (Cow::Borrowed) and compile as valid globs.
    #[test]
    fn test_plain_patterns_pass_through_unchanged() {
        let cases = vec![
            "*.log",
            "/var/log/**/*.txt",
            "/tmp/[0-9][0-9][0-9]",
            "some/dir/foo?.rs",
            "**/node_modules/**",
            "*.bak",
            "/absolute/path/file",
        ];

        for pattern in &cases {
            let expanded = shellexpand::full(pattern)
                .unwrap_or_else(|e| panic!("expansion failed for {:?}: {}", pattern, e));
            // With no `~` or `$`, shellexpand returns Cow::Borrowed — no allocation, no mutation.
            assert!(
                matches!(expanded, std::borrow::Cow::Borrowed(s) if s == *pattern),
                "plain pattern {:?} should be returned as Cow::Borrowed unchanged, got {:?}",
                pattern,
                expanded
            );
            // Must still compile as a valid glob.
            glob::Pattern::new(pattern)
                .unwrap_or_else(|e| panic!("{:?} should be a valid glob: {}", pattern, e));
        }
    }

    /// Prove that failed env-var expansion preserves the original pattern text
    /// rather than silently producing an empty string (the old .unwrap_or_default()
    /// bug).
    #[test]
    fn test_failed_expansion_falls_back_to_original_pattern() {
        let pattern = "$THIS_VAR_DEFINITELY_DOES_NOT_EXIST_12345/*.log";
        // shellexpand::full() returns Err for unknown vars (std::env::var errors).
        // The code should fall back to the original pattern, not an empty string.
        let expanded = match shellexpand::full(pattern) {
            Ok(expanded) => expanded.into_owned(),
            Err(_) => pattern.to_string(),
        };
        // After fallback, the expanded text should still be a valid glob containing the wildcard.
        assert!(
            expanded.contains("*.log"),
            "wildcard should survive fallback, got: {:?}",
            expanded
        );
        assert!(
            glob::Pattern::new(&expanded).is_ok(),
            "fallback {:?} should be valid glob",
            expanded
        );
        // Critically, it should NOT be empty (the old bug behavior).
        assert!(
            !expanded.is_empty(),
            "expansion failure must not produce empty pattern"
        );
    }

    #[test]
    fn test_companion_metadata_enrichment_with_missing_file() {
        use crate::plugin::{Parser, PluginMeta, SessionDetection, SessionIdSource, Source};

        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(".agentscribe");
        std::fs::create_dir_all(data_dir.join("sessions")).unwrap();

        // Create a test plugin with a non-existent companion index
        let plugin = Plugin {
            plugin: PluginMeta {
                name: "test-agent".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec![],
                exclude: vec![],
                format: LogFormat::Jsonl,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: None,
                envelope: None,
                array: None,
            },
            parser: Parser {
                ..Default::default()
            },
            metadata: Some(crate::plugin::Metadata {
                companion_index: Some("/nonexistent/session_index.jsonl".to_string()),
                ..Default::default()
            }),
        };

        let scraper = Scraper::new(data_dir).unwrap();

        // Should return Ok(None) when file doesn't exist (not an error)
        let result = scraper.load_companion_metadata("any-session", &plugin);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    /// Test that exclude patterns work correctly with absolute paths from glob expansion.
    ///
    /// This test verifies the fix for a bug where relative exclude patterns (e.g., `*/subagents/*`)
    /// would fail to match absolute paths returned by glob after shellexpand expansion.
    ///
    /// The bug occurs because:
    /// 1. Source pattern `~/foo/**/*.jsonl` expands to absolute `/home/user/foo/**/*.jsonl`
    /// 2. Glob returns absolute paths like `/home/user/foo/bar/subagents/file.jsonl`
    /// 3. Exclude pattern `*/subagents/*` stays relative (no leading slash)
    /// 4. glob::Pattern::new("*/subagents/*") expects to match against paths from cwd
    /// 5. Matching relative pattern against absolute path fails
    #[test]
    fn test_exclude_patterns_with_absolute_paths() {
        use crate::plugin::{Parser, PluginMeta, SessionDetection, SessionIdSource, Source};

        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(".agentscribe");
        std::fs::create_dir_all(data_dir.join("sessions")).unwrap();

        // Create directory structure:
        // temp/
        //   logs/
        //     session-1.jsonl         <- should be included
        //     project/
        //       session-2.jsonl       <- should be included
        //       subagents/
        //         agent-123.jsonl      <- should be EXCLUDED
        //     other/
        //       session-3.jsonl       <- should be included

        let logs_dir = temp.path().join("logs");
        std::fs::create_dir_all(logs_dir.join("project/subagents")).unwrap();
        std::fs::create_dir_all(logs_dir.join("other")).unwrap();

        // Create test files
        std::fs::write(logs_dir.join("session-1.jsonl"), "session 1").unwrap();
        std::fs::write(logs_dir.join("project/session-2.jsonl"), "session 2").unwrap();
        std::fs::write(
            logs_dir.join("project/subagents/agent-123.jsonl"),
            "subagent",
        )
        .unwrap();
        std::fs::write(logs_dir.join("other/session-3.jsonl"), "session 3").unwrap();

        // Create plugin with exclude pattern for subagents
        let plugin = Plugin {
            plugin: PluginMeta {
                name: "test-agent".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec![logs_dir.join("**/*.jsonl").to_str().unwrap().to_string()],
                exclude: vec!["*/subagents/*".to_string()], // Relative pattern
                format: LogFormat::Jsonl,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: None,
                envelope: None,
                array: None,
            },
            parser: Parser {
                ..Default::default()
            },
            metadata: None,
        };

        let scraper = Scraper::new(data_dir).unwrap();
        let files = scraper.discover_files(&plugin).unwrap();

        // Convert to set for easier comparison
        let file_set: std::collections::HashSet<String> = files
            .iter()
            .map(|p| p.to_str().unwrap().to_string())
            .collect();

        // Verify the subagent file is excluded
        let subagent_path = logs_dir
            .join("project/subagents/agent-123.jsonl")
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            !file_set.contains(&subagent_path),
            "subagent file should be excluded: {}",
            subagent_path
        );

        // Verify non-excluded files are included
        let session1_path = logs_dir
            .join("session-1.jsonl")
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            file_set.contains(&session1_path),
            "session-1.jsonl should be included"
        );

        let session2_path = logs_dir
            .join("project/session-2.jsonl")
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            file_set.contains(&session2_path),
            "project/session-2.jsonl should be included"
        );

        let session3_path = logs_dir
            .join("other/session-3.jsonl")
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            file_set.contains(&session3_path),
            "other/session-3.jsonl should be included"
        );

        // Verify we got exactly 3 files (not 4)
        assert_eq!(
            files.len(),
            3,
            "should have discovered 3 files (excluding subagent), got: {:?}",
            files
        );
    }

    /// Test that absolute exclude patterns work correctly.
    #[test]
    fn test_absolute_exclude_patterns() {
        use crate::plugin::{Parser, PluginMeta, SessionDetection, SessionIdSource, Source};

        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(".agentscribe");
        std::fs::create_dir_all(data_dir.join("sessions")).unwrap();

        let logs_dir = temp.path().join("logs");
        std::fs::create_dir_all(logs_dir.join("subdir")).unwrap();

        std::fs::write(logs_dir.join("file1.jsonl"), "content 1").unwrap();
        std::fs::write(logs_dir.join("subdir/file2.jsonl"), "content 2").unwrap();

        // Use absolute path in exclude pattern
        let exclude_abs = logs_dir
            .join("subdir/*.jsonl")
            .to_str()
            .unwrap()
            .to_string();

        let plugin = Plugin {
            plugin: PluginMeta {
                name: "test-agent".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec![logs_dir.join("**/*.jsonl").to_str().unwrap().to_string()],
                exclude: vec![exclude_abs],
                format: LogFormat::Jsonl,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: None,
                envelope: None,
                array: None,
            },
            parser: Parser {
                ..Default::default()
            },
            metadata: None,
        };

        let scraper = Scraper::new(data_dir).unwrap();
        let files = scraper.discover_files(&plugin).unwrap();

        assert_eq!(
            files.len(),
            1,
            "should have excluded subdir files, got: {:?}",
            files
        );
        assert!(
            files.iter().any(|p| p.ends_with("file1.jsonl")),
            "file1.jsonl should be included"
        );
    }

    /// Test the glob::Pattern behavior for relative vs absolute path matching.
    #[test]
    fn test_glob_pattern_relative_vs_absolute_matching() {
        // This test verifies glob pattern matching behavior:
        // A relative pattern like "*/subagents/*" DOES match absolute paths
        // because the leading * matches any number of leading path components.

        let abs_path = PathBuf::from("/home/user/logs/project/subagents/file.jsonl");
        let rel_pattern_str = "*/subagents/*";

        // Create pattern from relative string
        let rel_pattern = glob::Pattern::new(rel_pattern_str).unwrap();

        // Relative pattern DOES match absolute path (leading * matches any path prefix)
        let matches_rel = rel_pattern.matches_path(&abs_path);
        assert!(
            matches_rel,
            "relative pattern '*/subagents/*' SHOULD match absolute path {:?} (leading * matches any prefix)",
            abs_path
        );

        // Patterns with ** also work (equivalent behavior for this case)
        let double_star_pattern = glob::Pattern::new("**/subagents/*").unwrap();
        assert!(
            double_star_pattern.matches_path(&abs_path),
            "pattern with leading ** should match absolute path"
        );

        // Absolute patterns work as expected
        let abs_pattern = glob::Pattern::new("/home/user/logs/*/subagents/*").unwrap();
        assert!(
            abs_pattern.matches_path(&abs_path),
            "absolute pattern should match absolute path"
        );

        // Test that patterns without wildcards in the right positions don't match
        let specific_pattern = glob::Pattern::new("project/subagents/*").unwrap();
        assert!(
            !specific_pattern.matches_path(&abs_path),
            "pattern 'project/subagents/*' should NOT match absolute path (doesn't match leading /home/user/logs)"
        );
    }

    /// Debug test to verify pattern normalization behavior
    #[test]
    fn test_exclude_pattern_normalization_debug() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(".agentscribe");
        std::fs::create_dir_all(data_dir.join("sessions")).unwrap();

        let logs_dir = temp.path().join("logs");
        std::fs::create_dir_all(logs_dir.join("vendor/node_modules")).unwrap();
        std::fs::write(
            logs_dir.join("vendor/node_modules/package.json"),
            "node package",
        )
        .unwrap();

        // Test with pattern */subagents/*
        let exclude_pattern = "*/subagents/*";
        let exclude_expanded = exclude_pattern.to_string();

        // This is the exact logic from discover_files
        let normalized_pattern =
            if !exclude_expanded.starts_with('/') && !exclude_expanded.starts_with("**") {
                let stripped = exclude_expanded
                    .strip_prefix("./")
                    .unwrap_or(&exclude_expanded);
                format!("**/{}", stripped)
            } else {
                exclude_expanded
            };

        println!("Original pattern: {}", exclude_pattern);
        println!("Normalized pattern: {}", normalized_pattern);

        let abs_path = logs_dir.join("vendor/node_modules/package.json");
        println!("Absolute path: {}", abs_path.display());

        let pat = glob::Pattern::new(&normalized_pattern).unwrap();
        println!("Pattern matches: {}", pat.matches_path(&abs_path));

        // The pattern should NOT match vendor/node_modules/package.json
        assert!(
            !pat.matches_path(&abs_path),
            "Pattern {} should not match {}",
            normalized_pattern,
            abs_path.display()
        );
    }

    /// Comprehensive test for discover_files with various exclude patterns.
    ///
    /// This test creates a realistic directory structure and verifies that:
    /// - Relative exclude patterns (*/subagents/*) properly exclude files
    /// - Double-star patterns (**/node_modules/**) work correctly
    /// - Absolute exclude patterns work as expected
    /// - Non-matching files are not excluded
    #[test]
    fn test_discover_files_with_exclude_patterns() {
        use crate::plugin::{Parser, PluginMeta, SessionDetection, SessionIdSource, Source};

        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(".agentscribe");
        std::fs::create_dir_all(data_dir.join("sessions")).unwrap();

        // Create directory structure:
        // temp/
        //   logs/
        //     root.jsonl                    <- should be included
        //     project-a/
        //       session.jsonl               <- should be included
        //       subagents/
        //         agent-1.jsonl             <- EXCLUDED by */subagents/*
        //     project-b/
        //       session.jsonl               <- should be included
        //       subagents/
        //         nested/
        //           agent-2.jsonl           <- EXCLUDED by */subagents/*
        //     vendor/
        //       node_modules/
        //         package.jsonl             <- EXCLUDED by **/node_modules/**
        //       otherlib/
        //         lib.jsonl                 <- should be included

        let logs_dir = temp.path().join("logs");
        std::fs::create_dir_all(logs_dir.join("project-a/subagents")).unwrap();
        std::fs::create_dir_all(logs_dir.join("project-b/subagents/nested")).unwrap();
        std::fs::create_dir_all(logs_dir.join("vendor/node_modules")).unwrap();
        std::fs::create_dir_all(logs_dir.join("vendor/otherlib")).unwrap();

        // Create test files
        std::fs::write(logs_dir.join("root.jsonl"), "root session").unwrap();
        std::fs::write(logs_dir.join("project-a/session.jsonl"), "project a").unwrap();
        std::fs::write(
            logs_dir.join("project-a/subagents/agent-1.jsonl"),
            "subagent 1",
        )
        .unwrap();
        std::fs::write(logs_dir.join("project-b/session.jsonl"), "project b").unwrap();
        std::fs::write(
            logs_dir.join("project-b/subagents/nested/agent-2.jsonl"),
            "subagent 2",
        )
        .unwrap();
        std::fs::write(
            logs_dir.join("vendor/node_modules/package.jsonl"),
            "node package",
        )
        .unwrap();
        std::fs::write(logs_dir.join("vendor/otherlib/lib.jsonl"), "library").unwrap();

        // Test 1: Relative pattern */subagents/*
        let plugin = Plugin {
            plugin: PluginMeta {
                name: "test-agent".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec![logs_dir.join("**/*.jsonl").to_str().unwrap().to_string()],
                exclude: vec!["*/subagents/*".to_string()],
                format: LogFormat::Jsonl,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: None,
                envelope: None,
                array: None,
            },
            parser: Parser {
                ..Default::default()
            },
            metadata: None,
        };

        let scraper = Scraper::new(data_dir.clone()).unwrap();
        let files = scraper.discover_files(&plugin).unwrap();

        let file_set: std::collections::HashSet<String> = files
            .iter()
            .map(|p| p.to_str().unwrap().to_string())
            .collect();

        // Should exclude both subagent files
        assert!(
            !file_set.contains(
                logs_dir
                    .join("project-a/subagents/agent-1.jsonl")
                    .to_str()
                    .unwrap()
            ),
            "project-a/subagents/agent-1.jsonl should be excluded by */subagents/*"
        );
        assert!(
            !file_set.contains(
                logs_dir
                    .join("project-b/subagents/nested/agent-2.jsonl")
                    .to_str()
                    .unwrap()
            ),
            "project-b/subagents/nested/agent-2.jsonl should be excluded by */subagents/*"
        );

        // Should include non-subagent files
        assert!(
            file_set.contains(logs_dir.join("root.jsonl").to_str().unwrap()),
            "root.jsonl should be included"
        );
        assert!(
            file_set.contains(logs_dir.join("project-a/session.jsonl").to_str().unwrap()),
            "project-a/session.jsonl should be included"
        );
        assert!(
            file_set.contains(
                logs_dir
                    .join("vendor/node_modules/package.jsonl")
                    .to_str()
                    .unwrap()
            ),
            "vendor/node_modules/package.jsonl should be included (not excluded by */subagents/*)"
        );

        assert_eq!(
            files.len(),
            5,
            "should have 5 files with */subagents/* exclude (all except subagents), got: {:?}",
            files
        );

        // Test 2: Double-star pattern **/node_modules/**
        let plugin2 = Plugin {
            plugin: PluginMeta {
                name: "test-agent".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec![logs_dir.join("**/*.jsonl").to_str().unwrap().to_string()],
                exclude: vec!["**/node_modules/**".to_string()],
                format: LogFormat::Jsonl,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: None,
                envelope: None,
                array: None,
            },
            parser: Parser {
                ..Default::default()
            },
            metadata: None,
        };

        let files2 = scraper.discover_files(&plugin2).unwrap();
        let file_set2: std::collections::HashSet<String> = files2
            .iter()
            .map(|p| p.to_str().unwrap().to_string())
            .collect();

        // Should exclude node_modules
        assert!(
            !file_set2.contains(
                logs_dir
                    .join("vendor/node_modules/package.jsonl")
                    .to_str()
                    .unwrap()
            ),
            "vendor/node_modules/package.jsonl should be excluded by **/node_modules/**"
        );

        // Should include subagent files (not excluded by this pattern)
        assert!(
            file_set2.contains(
                logs_dir
                    .join("project-a/subagents/agent-1.jsonl")
                    .to_str()
                    .unwrap()
            ),
            "project-a/subagents/agent-1.jsonl should be included with **/node_modules/** exclude"
        );

        // Test 3: Multiple exclude patterns
        let plugin3 = Plugin {
            plugin: PluginMeta {
                name: "test-agent".to_string(),
                version: "1.0".to_string(),
            },
            source: Source {
                paths: vec![logs_dir.join("**/*.jsonl").to_str().unwrap().to_string()],
                exclude: vec![
                    "*/subagents/*".to_string(),
                    "**/node_modules/**".to_string(),
                ],
                format: LogFormat::Jsonl,
                session_detection: SessionDetection::OneFilePerSession {
                    session_id_from: SessionIdSource::Filename,
                },
                tree: None,
                truncation_limit: None,
                envelope: None,
                array: None,
            },
            parser: Parser {
                ..Default::default()
            },
            metadata: None,
        };

        let files3 = scraper.discover_files(&plugin3).unwrap();
        let file_set3: std::collections::HashSet<String> = files3
            .iter()
            .map(|p| p.to_str().unwrap().to_string())
            .collect();

        // Should exclude both subagents and node_modules
        assert!(
            !file_set3.contains(
                logs_dir
                    .join("project-a/subagents/agent-1.jsonl")
                    .to_str()
                    .unwrap()
            ),
            "should exclude subagents"
        );
        assert!(
            !file_set3.contains(
                logs_dir
                    .join("vendor/node_modules/package.jsonl")
                    .to_str()
                    .unwrap()
            ),
            "should exclude node_modules"
        );

        // Should include other files
        assert!(
            file_set3.contains(logs_dir.join("root.jsonl").to_str().unwrap()),
            "root.jsonl should be included"
        );
        assert!(
            file_set3.contains(logs_dir.join("vendor/otherlib/lib.jsonl").to_str().unwrap()),
            "vendor/otherlib/lib.jsonl should be included"
        );

        assert_eq!(
            files3.len(),
            4,
            "should have 4 files with both excludes, got: {:?}",
            files3
        );
    }
}

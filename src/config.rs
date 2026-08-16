//! Configuration management for AgentScribe.
//!
//! This module handles loading, validating, and providing access to AgentScribe's global
//! configuration. Configuration is stored in `~/.agentscribe/config.toml` and controls
//! all aspects of AgentScribe's behavior: data directories, scraping behavior, indexing
//! parameters, feature flags, and enrichment settings.
//!
//! # Configuration File
//!
//! The config file is TOML-formatted with sections for each component:
//!
//! ```toml
//! [general]
//! data_dir = "~/.agentscribe"
//! log_level = "info"
//!
//! [scrape]
//! debounce_seconds = 5
//! max_session_age_days = 0
//!
//! [index]
//! tantivy_heap_size_mb = 50
//!
//! [search]
//! default_max_results = 10
//! default_snippet_length = 200
//!
//! [daemon]
//! mcp_enabled = false
//! pid_file = "~/.agentscribe/agentscribe.pid"
//!
//! [outcome.weights]
//! success_confirmation = 3
//! success_clean_exit = 2
//! # ... other weights
//!
//! [cost.models]
//! "claude-sonnet-4-20250514" = { input = 3.0, output = 15.0 }
//! # ... other models
//! ```
//!
//! # Data Directory
//!
//! The data directory (`~/.agentscribe` by default) contains:
//! - `config.toml` - Global configuration
//! - `plugins/` - Scraper plugin definitions
//! - `sessions/` - Normalized conversation logs (JSONL)
//! - `index/` - Tantivy search index and vector index
//! - `state/` - Scrape state and daemon state
//!
//! # Environment Variables
//!
//! Configuration can be overridden via environment variables:
//! - `AGENTSCRIBE_DATA_DIR` - Override data directory location
//! - `AGENTSCRIBE_LOG_LEVEL` - Override logging level
//!
//! # Defaults
//!
//! Most configuration has sensible defaults. Only create a config file if you need to
//! customize behavior. The daemon and CLI use the same configuration source.
//!
//! # Validation
//!
//! Configuration is validated at load time. Invalid values (negative heap sizes, unknown
//! log levels, malformed paths) cause startup errors with clear messages.

use crate::enrichment::outcome::OutcomeConfig;
use crate::error::{AgentScribeError, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Default data directory name
const DATA_DIR_NAME: &str = ".agentscribe";

/// Model pricing for cost estimation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    pub input_per_1m: f64,
    pub output_per_1m: f64,
}

/// Cost configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostConfig {
    #[serde(default)]
    pub models: HashMap<String, ModelPricing>,
}

#[allow(clippy::derivable_impls)]
impl Default for CostConfig {
    fn default() -> Self {
        CostConfig {
            models: HashMap::new(),
        }
    }
}

/// Shell hook configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellHookConfig {
    /// Whether to run search in a background subprocess (recommended; false = blocking)
    #[serde(default = "default_true")]
    pub background: bool,
    /// Whether to capture stderr of the failed command (fragile, not recommended)
    #[serde(default)]
    pub stderr_capture: bool,
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

impl Default for ShellHookConfig {
    fn default() -> Self {
        ShellHookConfig {
            background: true,
            stderr_capture: false,
        }
    }
}

/// Daemon configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// Enable the MCP server when the daemon starts (default: false)
    #[serde(default)]
    pub mcp_enabled: bool,
    /// Unix socket path for the MCP server (default: ~/.agentscribe/mcp.sock)
    pub mcp_socket_path: Option<String>,
    /// Log rotation mode (default: "size")
    /// - "size": Rotate when log file exceeds `log_max_size_bytes` (recommended)
    /// - "daily": Time-based rotation at midnight (may grow unbounded within a day)
    /// - "hourly": Time-based rotation every hour
    /// - "daily+size": Hybrid - rotates at midnight AND when exceeding size limit
    #[serde(default = "default_log_rotation")]
    pub log_rotation: String,
    /// Maximum size of a single log file before rotation in bytes (default: 10MB)
    /// Only used when rotation mode is "size" or "daily+size"
    #[serde(default = "default_log_max_size_bytes")]
    pub log_max_size_bytes: u64,
    /// Number of rotated log files to retain (default: 7)
    #[serde(default = "default_log_retention_count")]
    pub log_retention_count: usize,
}

fn default_log_rotation() -> String {
    "size".to_string()
}

fn default_log_max_size_bytes() -> u64 {
    10 * 1024 * 1024 // 10MB
}

fn default_log_retention_count() -> usize {
    7
}

#[allow(clippy::derivable_impls)]
impl Default for DaemonConfig {
    fn default() -> Self {
        DaemonConfig {
            mcp_enabled: false,
            mcp_socket_path: None,
            log_rotation: default_log_rotation(),
            log_max_size_bytes: default_log_max_size_bytes(),
            log_retention_count: default_log_retention_count(),
        }
    }
}

/// A single user-defined normalization rule: strips a variable part from error strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizerRule {
    /// Regex pattern to match (e.g. `r"request_id=\w+"`)
    pub pattern: String,
    /// Replacement string (e.g. `"request_id={id}"`)
    pub replacement: String,
}

/// User-extensible error pattern configuration.
///
/// Maps to `[error_patterns.custom]` in `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ErrorPatternsConfig {
    /// Additional regex patterns that identify error lines (appended to built-ins).
    #[serde(default)]
    pub matchers: Vec<String>,
    /// Additional normalization rules applied after the built-in normalizers.
    #[serde(default)]
    pub normalizers: Vec<NormalizerRule>,
}

/// Whisper transcription configuration.
///
/// Maps to `[whisper]` in `config.toml`. The whisper executable must be in
/// PATH or configured explicitly. Supports whisper.cpp and OpenAI Whisper CLI.
///
/// Example (whisper.cpp):
/// ```toml
/// [whisper]
/// enabled = true
/// model_path = "~/.agentscribe/models/ggml-base.bin"
/// backend = "whisper_cpp"
/// word_timestamps = true
/// language = "en"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperConfig {
    /// Enable transcription support (default: false).
    #[serde(default)]
    pub enabled: bool,

    /// Path to the Whisper model file (required for whisper.cpp).
    pub model_path: Option<String>,

    /// Path or name of the whisper executable (default: "whisper").
    pub executable: Option<String>,

    /// Backend style: "whisper_cpp", "openai_whisper", or "auto" (default).
    /// "auto" detects the backend from the output JSON structure.
    pub backend: Option<String>,

    /// Maximum retry attempts on transcription failure (default: 3).
    #[serde(default = "default_whisper_max_retries")]
    pub max_retries: u32,

    /// Per-attempt timeout in seconds (default: 300).
    #[serde(default = "default_whisper_timeout")]
    pub timeout_seconds: u64,

    /// Request word-level timestamps (default: true).
    /// Falls back to utterance-level if the backend does not support it.
    #[serde(default = "default_true")]
    pub word_timestamps: bool,

    /// Language code passed to Whisper (e.g. "en"). Auto-detected if unset.
    pub language: Option<String>,
}

fn default_whisper_max_retries() -> u32 {
    3
}
fn default_whisper_timeout() -> u64 {
    300
}

impl Default for WhisperConfig {
    fn default() -> Self {
        WhisperConfig {
            enabled: false,
            model_path: None,
            executable: None,
            backend: None,
            max_retries: 3,
            timeout_seconds: 300,
            word_timestamps: true,
            language: None,
        }
    }
}

/// Behavioral signals configuration.
///
/// Maps to `[behavioral_signals]` in `config.toml`. Controls what patterns
/// are recognized as config/memory files for write detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehavioralSignalsConfig {
    /// File glob patterns that identify config/memory files.
    /// Matches filenames (e.g., "CLAUDE.md"), directory prefixes (e.g., ".claude/"),
    /// or directory paths (e.g., "memory/", "docs/notes/").
    #[serde(default = "default_config_patterns")]
    pub config_patterns: Vec<String>,
}

fn default_config_patterns() -> Vec<String> {
    vec![
        "CLAUDE.md".to_string(),
        "AGENTS.md".to_string(),
        ".claude/".to_string(),
        ".needle/".to_string(),
        "memory/".to_string(),
        "docs/notes/".to_string(),
        "MEMORY.md".to_string(),
    ]
}

impl Default for BehavioralSignalsConfig {
    fn default() -> Self {
        BehavioralSignalsConfig {
            config_patterns: default_config_patterns(),
        }
    }
}

/// Vector index configuration.
///
/// Maps to `[vector]` in `config.toml`. Controls semantic search using
/// quantized vector embeddings (turbovec).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorConfig {
    /// Enable semantic vector index (default: false).
    #[serde(default)]
    pub enabled: bool,

    /// Quantization bit width: 2 or 4 (default: 4).
    /// 4-bit provides better accuracy, 2-bit uses half the memory.
    #[serde(default = "default_vector_bit_width")]
    pub bit_width: u8,

    /// Embedding model to use.
    /// - Local: "nomic-embed-text" (via Ollama)
    /// - Cloud: "openai:text-embedding-3-small"
    #[serde(default = "default_vector_embedding_model")]
    pub embedding_model: String,

    /// Ollama endpoint for local embedding (default: http://localhost:11434).
    #[serde(default = "default_vector_ollama_url")]
    pub ollama_url: String,

    /// Tokens per indexed chunk (default: 512).
    #[serde(default = "default_vector_chunk_size")]
    pub chunk_size_tokens: usize,

    /// Overlap between adjacent chunks in tokens (default: 64).
    #[serde(default = "default_vector_chunk_overlap")]
    pub chunk_overlap_tokens: usize,

    /// Index session-level embeddings (default: true).
    #[serde(default = "default_true")]
    pub index_sessions: bool,

    /// Index chunk-level embeddings (default: false; ADR-2, bead bf-1pkfp).
    ///
    /// **ROOT CAUSE:** Prior to ADR-2, this defaulted to `true`, causing chunk-level
    /// embeddings (overlapping 512-token windows per session) to be built and stored
    /// alongside session-level embeddings. At 500K sessions, this adds ~1.15GB of
    /// vector data vs ~192MB for session-level alone — a 6x disk cost increase for
    /// a capability ("find the exact moment within a session") beyond the primary
    /// use case ("find a past session that solved a similar problem").
    ///
    /// **THE FIX:** Defaults to `false` post-ADR-2. Session-level embeddings are
    /// sufficient for the common case of finding relevant past sessions. Set to
    /// `true` to opt in to chunk-level retrieval when you need "which exact moment"
    /// precision.
    ///
    /// **MEMORY BUDGET:** See plan.md's Memory Budget Impact table for detailed
    /// sizing. Chunk-level indexing grows with corpus size and can dominate memory
    /// use at scale. The daemon loads it on-demand only for `context` and
    /// `search --semantic` queries.
    #[serde(default = "default_false")]
    pub index_chunks: bool,
}

fn default_vector_bit_width() -> u8 {
    4
}

fn default_vector_embedding_model() -> String {
    "nomic-embed-text".to_string()
}

fn default_vector_ollama_url() -> String {
    "http://localhost:11434".to_string()
}

fn default_vector_chunk_size() -> usize {
    512
}

fn default_vector_chunk_overlap() -> usize {
    64
}

impl Default for VectorConfig {
    fn default() -> Self {
        VectorConfig {
            enabled: false,
            bit_width: 4,
            embedding_model: "nomic-embed-text".to_string(),
            ollama_url: "http://localhost:11434".to_string(),
            chunk_size_tokens: 512,
            chunk_overlap_tokens: 64,
            index_sessions: true,
            index_chunks: false,
        }
    }
}

/// Privacy redaction configuration.
///
/// Transcripts are scanned for PII before storage and indexing.
/// Maps to `[redaction]` in `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactionConfig {
    /// Enable redaction scanning (default: true).
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Redact email addresses (default: true).
    #[serde(default = "default_true")]
    pub redact_emails: bool,

    /// Redact phone numbers (default: true).
    #[serde(default = "default_true")]
    pub redact_phones: bool,

    /// Redact credit card numbers (default: true).
    #[serde(default = "default_true")]
    pub redact_credit_cards: bool,

    /// Redact US Social Security Numbers (default: true).
    #[serde(default = "default_true")]
    pub redact_ssn: bool,

    /// Additional user-defined regex patterns to redact (replaced with \[REDACTED\]).
    #[serde(default)]
    pub custom_patterns: Vec<String>,
}

impl Default for RedactionConfig {
    fn default() -> Self {
        RedactionConfig {
            enabled: true,
            redact_emails: true,
            redact_phones: true,
            redact_credit_cards: true,
            redact_ssn: true,
            custom_patterns: Vec::new(),
        }
    }
}

/// Global configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub general: GeneralConfig,
    pub scrape: ScrapeConfig,
    pub index: IndexConfig,
    pub search: SearchConfig,
    pub outcome: OutcomeConfig,
    #[serde(default)]
    pub cost: CostConfig,
    #[serde(default)]
    pub shell_hook: ShellHookConfig,
    #[serde(default)]
    pub daemon: DaemonConfig,
    #[serde(default)]
    pub error_patterns: ErrorPatternsConfig,
    #[serde(default)]
    pub whisper: WhisperConfig,
    #[serde(default)]
    pub redaction: RedactionConfig,
    #[serde(default)]
    pub vector: VectorConfig,
    #[serde(default)]
    pub behavioral_signals: BehavioralSignalsConfig,
}

/// General configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    pub data_dir: Option<String>,
    pub log_level: String,
}

/// Scraping configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeConfig {
    pub debounce_seconds: u64,
    pub max_session_age_days: u32,
    /// Commit newly scraped sessions to git after each successful scrape (default: false).
    /// The data directory must be inside a git repository for this to take effect.
    #[serde(default)]
    pub git_auto_commit: bool,
    /// Maximum seconds to wait for the scrape-state.json file lock before giving up (default: 30).
    /// Set to 0 to disable the timeout (wait indefinitely).
    #[serde(default = "default_lock_timeout_seconds")]
    pub lock_timeout_seconds: u64,
}

fn default_lock_timeout_seconds() -> u64 {
    30
}

/// Index configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    pub tantivy_heap_size_mb: usize,
}

/// Search configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    pub default_max_results: usize,
    pub default_snippet_length: usize,
    /// Levenshtein edit distance for fuzzy term queries (default: 1)
    #[serde(default = "default_fuzzy_edit_distance")]
    pub fuzzy_edit_distance: u8,
}

fn default_fuzzy_edit_distance() -> u8 {
    1
}

impl Default for Config {
    fn default() -> Self {
        Config {
            general: GeneralConfig {
                data_dir: None,
                log_level: "info".to_string(),
            },
            scrape: ScrapeConfig {
                debounce_seconds: 5,
                max_session_age_days: 0,
                git_auto_commit: false,
                lock_timeout_seconds: 30,
            },
            index: IndexConfig {
                tantivy_heap_size_mb: 50,
            },
            search: SearchConfig {
                default_max_results: 10,
                default_snippet_length: 200,
                fuzzy_edit_distance: 1,
            },
            outcome: OutcomeConfig::default(),
            cost: CostConfig::default(),
            shell_hook: ShellHookConfig::default(),
            daemon: DaemonConfig::default(),
            error_patterns: ErrorPatternsConfig::default(),
            whisper: WhisperConfig::default(),
            redaction: RedactionConfig::default(),
            vector: VectorConfig::default(),
            behavioral_signals: BehavioralSignalsConfig::default(),
        }
    }
}

impl Config {
    /// Get the MCP socket path (defaults to <data_dir>/mcp.sock)
    pub fn mcp_socket_path(&self) -> Result<PathBuf> {
        if let Some(ref path) = self.daemon.mcp_socket_path {
            let expanded = shellexpand::tilde(path);
            Ok(PathBuf::from(expanded.as_ref()))
        } else {
            Ok(self.data_dir()?.join("mcp.sock"))
        }
    }

    /// Get the data directory path
    pub fn data_dir(&self) -> Result<PathBuf> {
        if let Some(ref dir) = self.general.data_dir {
            let expanded = shellexpand::tilde(dir);
            Ok(PathBuf::from(expanded.as_ref()))
        } else {
            // Use default: ~/.agentscribe
            let home = directories::BaseDirs::new()
                .map(|d| d.home_dir().to_path_buf())
                .ok_or_else(|| {
                    AgentScribeError::DataDir("Cannot determine home directory".to_string())
                })?;
            Ok(home.join(DATA_DIR_NAME))
        }
    }

    /// Load config from file
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Config::default());
        }

        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)
            .map_err(|e| AgentScribeError::Config(format!("Invalid TOML: {}", e)))?;

        Ok(config)
    }

    /// Save config to file
    pub fn save(&self, path: &Path) -> Result<()> {
        let toml = toml::to_string_pretty(self)
            .map_err(|e| AgentScribeError::Config(format!("Serialization error: {}", e)))?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(path, toml)?;
        Ok(())
    }

    /// Initialize the data directory structure
    pub fn init_data_dir(&self) -> Result<PathBuf> {
        let data_dir = self.data_dir()?;

        // Create directory structure
        fs::create_dir_all(&data_dir)?;
        fs::create_dir_all(data_dir.join("plugins"))?;
        fs::create_dir_all(data_dir.join("sessions"))?;
        fs::create_dir_all(data_dir.join("index"))?;
        fs::create_dir_all(data_dir.join("state"))?;

        // Create default config if it doesn't exist
        let config_path = data_dir.join("config.toml");
        if !config_path.exists() {
            self.save(&config_path)?;
        }

        Ok(data_dir)
    }

    /// Copy bundled plugins to the data directory
    pub fn install_bundled_plugins(&self) -> Result<usize> {
        let data_dir = self.data_dir()?;
        let plugin_dir = data_dir.join("plugins");

        fs::create_dir_all(&plugin_dir)?;

        // Bundled plugin definitions
        let bundled = Self::bundled_plugins();

        let mut installed = 0;
        for (name, content) in bundled {
            let target_path = plugin_dir.join(format!("{}.toml", name));
            if !target_path.exists() {
                fs::write(&target_path, content)?;
                installed += 1;
            }
        }

        Ok(installed)
    }

    /// Get bundled plugin definitions
    fn bundled_plugins() -> Vec<(&'static str, &'static str)> {
        vec![
            ("claude-code", include_str!("../plugins/claude-code.toml")),
            ("aider", include_str!("../plugins/aider.toml")),
            ("codex", include_str!("../plugins/codex.toml")),
            ("opencode", include_str!("../plugins/opencode.toml")),
            ("cursor", include_str!("../plugins/cursor.toml")),
            ("windsurf", include_str!("../plugins/windsurf.toml")),
        ]
    }
}

/// Configuration file path (respects XDG_CONFIG_DIR on Linux, ~/Library/Application Support on macOS, %APPDATA% on Windows)
pub fn config_path() -> Option<PathBuf> {
    ProjectDirs::from("com", "agentscribe", "AgentScribe")
        .map(|dirs| dirs.config_dir().join("agentscribe").join("config.toml"))
}

/// Get the default data directory
#[allow(dead_code)]
pub fn default_data_dir() -> Result<PathBuf> {
    Config::default().data_dir()
}

/// Initialize AgentScribe (create data directory, install plugins)
pub fn init(force: bool) -> Result<PathBuf> {
    let config = Config::default();
    let data_dir = config.data_dir()?;

    if data_dir.exists() && !force {
        // Check if already initialized
        let config_path = data_dir.join("config.toml");
        if config_path.exists() {
            return Ok(data_dir);
        }
    }

    config.init_data_dir()?;

    // Install bundled plugins
    let installed = config.install_bundled_plugins()?;
    eprintln!("Installed {} bundled plugins", installed);

    Ok(data_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.general.log_level, "info");
        assert_eq!(config.scrape.debounce_seconds, 5);
    }

    #[test]
    fn test_config_data_dir() {
        let config = Config::default();
        let data_dir = config.data_dir().unwrap();
        assert!(data_dir.ends_with(".agentscribe"));
    }

    #[test]
    fn test_init_creates_directories() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(".agentscribe");

        let mut config = Config::default();
        config.general.data_dir = Some(data_dir.to_str().unwrap().to_string());

        let result = config.init_data_dir().unwrap();
        assert!(result.exists());
        assert!(result.join("plugins").exists());
        assert!(result.join("sessions").exists());
        assert!(result.join("state").exists());
        assert!(result.join("config.toml").exists());
    }
}

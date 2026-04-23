//! Configuration management
//!
//! Handles global configuration and data directory initialization.

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
}

#[allow(clippy::derivable_impls)]
impl Default for DaemonConfig {
    fn default() -> Self {
        DaemonConfig {
            mcp_enabled: false,
            mcp_socket_path: None,
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

    /// Additional user-defined regex patterns to redact (replaced with [REDACTED]).
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

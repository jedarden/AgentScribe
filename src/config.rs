//! Configuration management
//!
//! Handles global configuration and data directory initialization.

use crate::enrichment::outcome::OutcomeConfig;
use crate::error::{AgentScribeError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::fs;
use directories::ProjectDirs;

/// Default data directory name
const DATA_DIR_NAME: &str = ".agentscribe";

/// Global configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub general: GeneralConfig,
    pub scrape: ScrapeConfig,
    pub index: IndexConfig,
    pub search: SearchConfig,
    pub outcome: OutcomeConfig,
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
            },
            index: IndexConfig {
                tantivy_heap_size_mb: 50,
            },
            search: SearchConfig {
                default_max_results: 10,
                default_snippet_length: 200,
            },
            outcome: OutcomeConfig::default(),
        }
    }
}

impl Config {
    /// Get the data directory path
    pub fn data_dir(&self) -> Result<PathBuf> {
        if let Some(ref dir) = self.general.data_dir {
            let expanded = shellexpand::tilde(dir);
            Ok(PathBuf::from(expanded.as_ref()))
        } else {
            // Use default: ~/.agentscribe
            let home = directories::BaseDirs::new()
                .map(|d| d.home_dir().to_path_buf())
                .ok_or_else(|| AgentScribeError::DataDir("Cannot determine home directory".to_string()))?;
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
        ]
    }
}

/// Configuration file path (respects XDG_CONFIG_DIR on Linux, ~/Library/Application Support on macOS, %APPDATA% on Windows)
pub fn config_path() -> Option<PathBuf> {
    ProjectDirs::from("com", "agentscribe", "AgentScribe")
        .map(|dirs| dirs.config_dir().join("agentscribe").join("config.toml"))
}

/// Get the default data directory
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

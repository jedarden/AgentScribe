//! CLI commands for AgentScribe

use crate::config::{self, Config};
use crate::error::Result;
use crate::plugin::validate_plugin_file;
use crate::scraper::Scraper;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tabled::{Table, Tabled};

/// AgentScribe - Archive, search, and learn from coding agent conversations
#[derive(Parser, Debug)]
#[command(name = "agentscribe")]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

/// Available CLI commands
#[derive(Subcommand, Debug)]
enum Commands {
    /// Manage global config and data directory
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Manage scraper plugin definitions
    Plugins {
        #[command(subcommand)]
        action: PluginsAction,
    },
    /// Discover and read agent log files
    Scrape {
        /// Plugin name to scrape (default: all)
        #[arg(short, long)]
        plugin: Option<String>,

        /// Specific file to scrape
        #[arg(short, long)]
        file: Option<PathBuf>,

        /// Dry run - show what would be scraped without writing
        #[arg(long)]
        dry_run: bool,

        /// Output events as JSON (for debugging)
        #[arg(long)]
        output_events: bool,

        /// JSON output
        #[arg(long)]
        json: bool,
    },
    /// Show tracked agents and session counts
    Status {
        /// JSON output
        #[arg(long)]
        json: bool,
    },
}

/// Config subcommands
#[derive(Subcommand, Debug)]
enum ConfigAction {
    /// Initialize global config and data directory
    Init {
        /// Force reinitialization
        #[arg(long)]
        force: bool,
    },
    /// Show current configuration
    Show,
    /// Set a configuration value
    Set {
        /// Configuration key (e.g., general.log_level)
        key: String,
        /// Configuration value
        value: String,
    },
    /// Get a configuration value
    Get {
        /// Configuration key
        key: String,
    },
}

/// Plugins subcommands
#[derive(Subcommand, Debug)]
enum PluginsAction {
    /// List available plugins
    List {
        /// Show detailed information
        #[arg(long)]
        verbose: bool,
    },
    /// Validate a plugin definition
    Validate {
        /// Path to plugin file
        path: PathBuf,
    },
    /// Show plugin details
    Show {
        /// Plugin name
        name: String,
    },
}

/// Table row for plugin list output
#[derive(Tabled)]
struct PluginRow {
    name: String,
    version: String,
    format: String,
}

/// Run the CLI
pub fn run() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::Config { action } => run_config(action),
        Commands::Plugins { action } => run_plugins(action),
        Commands::Scrape {
            plugin,
            file,
            dry_run,
            output_events,
            json,
        } => run_scrape(plugin, file, dry_run, output_events, json),
        Commands::Status { json } => run_status(json),
    }
}

/// Run config commands
fn run_config(action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Init { force } => {
            let data_dir = config::init(force)?;
            println!("Initialized AgentScribe data directory: {}", data_dir.display());
            Ok(())
        }
        ConfigAction::Show => {
            let config = load_config()?;
            let data_dir = config.data_dir()?;
            println!("Data directory: {}", data_dir.display());
            println!("Log level: {}", config.general.log_level);
            println!("\nScraping:");
            println!("  Debounce: {}s", config.scrape.debounce_seconds);
            println!("  Max session age: {} days", config.scrape.max_session_age_days);
            println!("\nSearch:");
            println!("  Default max results: {}", config.search.default_max_results);
            println!("  Default snippet length: {}", config.search.default_snippet_length);
            Ok(())
        }
        ConfigAction::Set { key, value } => {
            let mut config = load_config()?;
            // Simple key-path setter (could be enhanced)
            match key.split('.').collect::<Vec<_>>().as_slice() {
                ["general", "log_level"] => config.general.log_level = value.clone(),
                ["scrape", "debounce_seconds"] => {
                    config.scrape.debounce_seconds = value.parse().unwrap_or(5);
                }
                ["scrape", "max_session_age_days"] => {
                    config.scrape.max_session_age_days = value.parse().unwrap_or(0);
                }
                _ => {
                    eprintln!("Unknown configuration key: {}", key);
                    return Ok(());
                }
            }

            let data_dir = config.data_dir()?;
            let config_path = data_dir.join("config.toml");
            config.save(&config_path)?;
            println!("Set {} = {}", key, value);
            Ok(())
        }
        ConfigAction::Get { key } => {
            let config = load_config()?;
            let value = match key.split('.').collect::<Vec<_>>().as_slice() {
                ["general", "data_dir"] => config.data_dir()?.display().to_string(),
                ["general", "log_level"] => config.general.log_level.clone(),
                ["scrape", "debounce_seconds"] => config.scrape.debounce_seconds.to_string(),
                ["scrape", "max_session_age_days"] => config.scrape.max_session_age_days.to_string(),
                _ => {
                    eprintln!("Unknown configuration key: {}", key);
                    return Ok(());
                }
            };
            println!("{}", value);
            Ok(())
        }
    }
}

/// Run plugins commands
fn run_plugins(action: PluginsAction) -> Result<()> {
    match action {
        PluginsAction::List { verbose } => {
            let config = load_config()?;
            let data_dir = config.data_dir()?;
            let plugin_dir = data_dir.join("plugins");

            let mut scraper = Scraper::new(data_dir)?;
            scraper.load_plugins()?;

            let mut rows = Vec::new();

            for (name, plugin) in scraper.plugin_manager().all() {
                rows.push(PluginRow {
                    name: name.clone(),
                    version: plugin.plugin.version.clone(),
                    format: plugin.source.format.as_str().to_string(),
                });

                if verbose {
                    println!("Plugin: {}", name);
                    println!("  Version: {}", plugin.plugin.version);
                    println!("  Format: {}", plugin.source.format.as_str());
                    println!("  Paths:");
                    for path in &plugin.source.paths {
                        println!("    - {}", path);
                    }
                    println!();
                }
            }

            if !verbose {
                if rows.is_empty() {
                    println!("No plugins found in {}", plugin_dir.display());
                } else {
                    println!("{}", Table::new(rows));
                }
            }

            Ok(())
        }
        PluginsAction::Validate { path } => {
            let plugin = validate_plugin_file(&path)?;
            println!("Plugin '{}' is valid!", plugin.plugin.name);
            println!("  Version: {}", plugin.plugin.version);
            println!("  Format: {}", plugin.source.format.as_str());
            Ok(())
        }
        PluginsAction::Show { name } => {
            let config = load_config()?;
            let data_dir = config.data_dir()?;
            let mut scraper = Scraper::new(data_dir)?;
            scraper.load_plugins()?;

            if let Some(plugin) = scraper.plugin_manager().get(&name) {
                println!("Plugin: {}", plugin.plugin.name);
                println!("  Version: {}", plugin.plugin.version);
                println!("  Format: {}", plugin.source.format.as_str());
                println!("\n  Source paths:");
                for path in &plugin.source.paths {
                    println!("    - {}", path);
                }
                if !plugin.source.exclude.is_empty() {
                    println!("  Excludes:");
                    for pattern in &plugin.source.exclude {
                        println!("    - {}", pattern);
                    }
                }
                println!("\n  Session detection:");
                match &plugin.source.session_detection {
                    crate::plugin::SessionDetection::OneFilePerSession { .. } => {
                        println!("    Method: one-file-per-session");
                    }
                    crate::plugin::SessionDetection::TimestampGap { gap_threshold } => {
                        println!("    Method: timestamp-gap ({})", gap_threshold);
                    }
                    crate::plugin::SessionDetection::Delimiter { delimiter_pattern } => {
                        println!("    Method: delimiter ({})", delimiter_pattern);
                    }
                }
            } else {
                eprintln!("Plugin '{}' not found", name);
                std::process::exit(1);
            }

            Ok(())
        }
    }
}

/// Run scrape command
fn run_scrape(
    plugin: Option<String>,
    file: Option<PathBuf>,
    dry_run: bool,
    _output_events: bool,
    _json: bool,
) -> Result<()> {
    let config = load_config()?;
    let data_dir = config.data_dir()?;

    // Initialize if needed
    if !data_dir.exists() {
        println!("Data directory not found. Initializing...");
        config::init(false)?;
    }

    let mut scraper = Scraper::new(data_dir)?;
    scraper.load_plugins()?;

    if let Some(file_path) = file {
        // Scrape single file
        println!("Scraping file: {}", file_path.display());

        // Determine plugin from file or use specified
        let plugin_name = plugin.clone().unwrap_or_else(|| {
            // Try to detect from file location
            let path_str = file_path.display().to_string();
            if path_str.contains(".claude") {
                "claude-code".to_string()
            } else if path_str.contains("aider") {
                "aider".to_string()
            } else if path_str.contains("codex") {
                "codex".to_string()
            } else if path_str.contains("opencode") {
                "opencode".to_string()
            } else {
                eprintln!("Cannot auto-detect plugin. Use --plugin to specify.");
                std::process::exit(1);
            }
        });

        if let Some(p) = scraper.plugin_manager().get(&plugin_name).cloned() {
            if dry_run {
                println!("Dry run - would scrape with plugin: {}", plugin_name);
                println!("  File: {}", file_path.display());
                println!("  Format: {}", p.source.format.as_str());
                // For dry run, just show we'd process it
                println!("  Would detect sessions and parse events...");
            } else {
                let result = scraper.scrape_file(&file_path, &p)?;
                println!("Scraped {} session(s)", result.sessions_scraped);
                if !result.errors.is_empty() {
                    eprintln!("Errors: {}", result.errors.len());
                }
            }
        } else {
            eprintln!("Plugin '{}' not found", plugin_name);
            std::process::exit(1);
        }
    } else if let Some(plugin_name) = plugin {
        // Scrape specific plugin
        println!("Scraping plugin: {}", plugin_name);

        if let Some(p) = scraper.plugin_manager().get(&plugin_name).cloned() {
            if dry_run {
                println!("Dry run - would discover and scrape files:");
                let files = scraper.discover_files(&p)?;
                for file in files {
                    println!("  - {}", file.display());
                }
            } else {
                let result = scraper.scrape_plugin(&p)?;
                println!(
                    "Scraped {} session(s) from {} file(s)",
                    result.sessions_scraped,
                    result.files_processed
                );
                if result.files_skipped > 0 {
                    println!("  ({} files unchanged, skipped)", result.files_skipped);
                }
                if !result.errors.is_empty() {
                    eprintln!("Errors: {}", result.errors.len());
                    for error in &result.errors[..result.errors.len().min(5)] {
                        eprintln!("  - {}: {}", error.file, error.message);
                    }
                    if result.errors.len() > 5 {
                        eprintln!("  ... and {} more", result.errors.len() - 5);
                    }
                }
            }
        } else {
            eprintln!("Plugin '{}' not found", plugin_name);
            std::process::exit(1);
        }
    } else {
        // Scrape all plugins
        println!("Scraping all plugins...");

        if dry_run {
            for plugin_name in scraper.plugin_manager().names() {
                println!("  - {}", plugin_name);
            }
        } else {
            let result = scraper.scrape_all()?;
            println!(
                "Scraped {} session(s) total",
                result.sessions_scraped
            );
            if !result.errors.is_empty() {
                eprintln!("Errors: {}", result.errors.len());
            }
        }
    }

    Ok(())
}

/// Run status command
fn run_status(json: bool) -> Result<()> {
    let config = load_config()?;
    let data_dir = config.data_dir()?;
    let data_dir_str = data_dir.display().to_string();

    if !data_dir.exists() {
        println!("AgentScribe not initialized. Run 'agentscribe config init' to set up.");
        return Ok(());
    }

    let mut scraper = Scraper::new(data_dir)?;
    scraper.load_plugins()?;

    let plugin_names = scraper.plugin_manager().names();

    if json {
        use serde_json::json;
        let mut status = json!({
            "data_dir": data_dir_str,
            "plugins": []
        });

        for plugin_name in plugin_names {
            let sessions = scraper.list_sessions(plugin_name)?;
            status["plugins"].as_array_mut().unwrap().push(json!({
                "name": plugin_name,
                "sessions": sessions.len()
            }));
        }

        println!("{}", status);
    } else {
        println!("AgentScribe Status");
        println!("  Data directory: {}", data_dir_str);
        println!("  Plugins loaded: {}", plugin_names.len());
        println!();

        for plugin_name in plugin_names {
            let sessions = scraper.list_sessions(plugin_name)?;
            println!("  {}: {} session(s)", plugin_name, sessions.len());
        }
    }

    Ok(())
}

/// Load configuration from default location
fn load_config() -> Result<Config> {
    let config_path = config::config_path();
    if let Some(path) = config_path {
        Config::load(&path)
    } else {
        Ok(Config::default())
    }
}

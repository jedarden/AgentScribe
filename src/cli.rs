//! CLI commands for AgentScribe

use crate::config::{self, Config};
use crate::error::Result;
use crate::index::IndexManager;
use crate::plugin::validate_plugin_file;
use crate::scraper::{git_auto_commit, Scraper};
use crate::search::{self, SearchOptions, SortOrder};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use std::io;
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
#[allow(clippy::large_enum_variant)]
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
    /// Manage the Tantivy search index
    Index {
        #[command(subcommand)]
        action: IndexAction,
    },
    /// Show tracked agents and session counts
    Status {
        /// JSON output
        #[arg(long)]
        json: bool,

        /// Show status for a specific plugin only
        #[arg(short, long)]
        plugin: Option<String>,
    },
    /// Query the Tantivy index for matching sessions
    Search {
        /// Search query string (Tantivy query syntax)
        query: Option<String>,

        /// Error fingerprint pattern to search
        #[arg(long)]
        error: Option<String>,

        /// Code content query
        #[arg(long)]
        code: Option<String>,

        /// Language filter for code search
        #[arg(long)]
        lang: Option<String>,

        /// Return only extracted solutions
        #[arg(long)]
        solution_only: bool,

        /// Find sessions similar to this session ID
        #[arg(long)]
        like: Option<String>,

        /// Retrieve a specific session by ID
        #[arg(long)]
        session: Option<String>,

        /// Filter by source agent type (repeatable)
        #[arg(short, long)]
        agent: Vec<String>,

        /// Filter by project path
        #[arg(long)]
        project: Option<String>,

        /// Only match sessions after this timestamp
        #[arg(long)]
        since: Option<String>,

        /// Only match sessions before this timestamp
        #[arg(long)]
        before: Option<String>,

        /// Filter by tag (repeatable, AND logic)
        #[arg(short = 't', long)]
        tag: Vec<String>,

        /// Filter by outcome (success, failure, abandoned, unknown)
        #[arg(long)]
        outcome: Option<String>,

        /// Filter by doc type (session, code_artifact)
        #[arg(long)]
        r#type: Option<String>,

        /// Filter by model name
        #[arg(long)]
        model: Option<String>,

        /// Enable fuzzy matching on all query terms
        #[arg(long)]
        fuzzy: bool,

        /// Levenshtein edit distance for fuzzy matching (overrides config; default: 1)
        #[arg(long)]
        edit_distance: Option<u8>,

        /// Maximum number of results
        #[arg(short = 'n', long, default_value = "10")]
        max_results: usize,

        /// Maximum snippet length per result (0 to omit)
        #[arg(long, default_value = "200")]
        snippet_length: usize,

        /// Token budget for greedy knapsack context packing
        #[arg(long)]
        token_budget: Option<usize>,

        /// Skip first N results (pagination)
        #[arg(long, default_value = "0")]
        offset: usize,

        /// Sort order: relevance, newest, oldest, turns
        #[arg(short, long, default_value = "relevance")]
        sort: String,

        /// Output a single-line hint (for shell hook integration)
        #[arg(long)]
        hint: bool,

        /// JSON output
        #[arg(long)]
        json: bool,
    },
    /// Detect recurring problems from error fingerprints
    Recurring {
        /// Only consider sessions after this timestamp (ISO 8601, or relative like 30d, 12w)
        #[arg(long)]
        since: Option<String>,

        /// Minimum occurrence count to report (default: 3)
        #[arg(long, default_value = "3")]
        threshold: usize,

        /// JSON output
        #[arg(long)]
        json: bool,
    },
    /// Manage the background daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Distill session patterns into agent-specific rules files
    Rules {
        /// Project path to extract rules for
        project_path: PathBuf,

        /// Output format: claude, cursor, or aider
        #[arg(long, value_enum, default_value = "claude")]
        format: String,

        /// JSON output (print rules without writing file)
        #[arg(long)]
        json: bool,
    },
    /// Cross-agent performance comparison and analytics
    Analytics {
        /// Filter by agent name
        #[arg(short, long)]
        agent: Option<String>,

        /// Filter by project path
        #[arg(short, long)]
        project: Option<String>,

        /// Only include sessions after this date (ISO 8601, or relative like 30d, 12w)
        #[arg(long)]
        since: Option<String>,

        /// JSON structured output
        #[arg(long)]
        json: bool,
    },
    /// Garbage collect old session files and index entries
    Gc {
        /// Delete sessions older than this duration (e.g., 30d, 12w, 6mo)
        /// Default: value from config.toml scrape.max_session_age_days
        #[arg(long)]
        older_than: Option<String>,

        /// Show what would be deleted without acting
        #[arg(long)]
        dry_run: bool,

        /// JSON output
        #[arg(long)]
        json: bool,
    },
    /// Bidirectional git commit ↔ session linking
    Blame {
        /// File path with optional line number (e.g., src/auth.rs or src/auth.rs:42)
        spec: String,

        /// Maximum number of results (default: 10)
        #[arg(short = 'n', long, default_value = "10")]
        max_results: usize,

        /// JSON output
        #[arg(long)]
        json: bool,
    },
    /// Show all sessions that touched a file (chronological)
    File {
        /// File path or glob pattern (e.g., src/auth.rs or "src/auth/**")
        path: String,

        /// Maximum number of results (default: 50)
        #[arg(short = 'n', long, default_value = "50")]
        max_results: usize,

        /// JSON output
        #[arg(long)]
        json: bool,
    },
    /// Generate Markdown summaries for sessions
    Summarize {
        /// Show summary for a specific session ID
        #[arg(long)]
        session: Option<String>,

        /// Only include sessions after this timestamp (ISO 8601, or relative like 7d)
        #[arg(long)]
        since: Option<String>,

        /// Filter by project path
        #[arg(long)]
        project: Option<String>,

        /// Maximum number of sessions to summarize (default: 20)
        #[arg(short = 'n', long, default_value = "20")]
        max_results: usize,

        /// Write summaries to .md files alongside session JSONL files
        #[arg(long)]
        write: bool,

        /// JSON output
        #[arg(long)]
        json: bool,
    },
    /// Generate shell integration snippet for auto-querying on error
    ShellHook {
        /// Shell to generate snippet for (bash, zsh, fish)
        shell: String,
    },
    /// Show known gotchas, error patterns, and statistics for a file
    FileKnowledge {
        /// File path to analyze
        file_path: String,

        /// JSON output
        #[arg(long)]
        json: bool,
    },
    /// Generate an activity digest summary
    Digest {
        /// Only include sessions after this timestamp (ISO 8601, or relative like 7d, 30d)
        #[arg(long, default_value = "7d")]
        since: String,

        /// Write output to file instead of stdout
        #[arg(short, long)]
        output: Option<String>,

        /// JSON structured output
        #[arg(long)]
        json: bool,
    },
    /// Generate a quarterly Pulse Report (State of AI Coding)
    PulseReport {
        /// Quarter to report on: YYYY-Q1 through YYYY-Q4, or "current" (default: current)
        #[arg(long, default_value = "current")]
        quarter: String,

        /// Write output to file instead of stdout
        #[arg(short, long)]
        output: Option<String>,

        /// Output format: markdown (default), html, or json
        #[arg(long, default_value = "markdown")]
        format: String,
    },
    /// Show per-account Claude Code utilization (5h and 7d rolling windows)
    Capacity {
        /// Claude config directories to scan, one per account (default: ~/.claude + auto-discovered ~/.claude-* dirs)
        #[arg(long)]
        account_dir: Vec<PathBuf>,

        /// Maximum age of cached usage data in seconds before falling back to JSONL (default: 600)
        #[arg(long, default_value = "600")]
        cache_max_age: u64,

        /// JSON output
        #[arg(long)]
        json: bool,
    },
    /// Generate shell completion script
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
    /// Transcribe an audio file using the configured local Whisper model
    Transcribe {
        /// Audio file to transcribe (wav, mp3, or m4a)
        input: PathBuf,

        /// Wait for the job to complete and print the transcript (default: true)
        #[arg(long, default_value_t = true)]
        wait: bool,

        /// Maximum seconds to wait for completion (default: 600)
        #[arg(long, default_value_t = 600)]
        timeout: u64,

        /// JSON output (includes timestamps and metadata)
        #[arg(long)]
        json: bool,
    },
    /// Pre-task priming query for agent workers
    Context {
        /// Task description to query for context
        query: String,

        /// Token budget for context packing (default: 3000)
        #[arg(long, default_value = "3000")]
        token_budget: usize,

        /// Scope rules extraction to this project path
        #[arg(long)]
        project: Option<PathBuf>,

        /// JSON output (wrap sections in JSON object)
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

/// Daemon subcommands
#[derive(Subcommand, Debug)]
enum IndexAction {
    /// Drop and rebuild the index from session files
    Rebuild {
        /// Only re-index sessions from this plugin
        #[arg(short, long)]
        plugin: Option<String>,

        /// Tantivy writer heap size in MB
        #[arg(long, default_value = "50")]
        heap_size: usize,
    },
    /// Show index statistics
    Stats,
    /// Merge segments for better query performance
    Optimize,
}

/// Daemon subcommands
#[derive(Subcommand, Debug)]
enum DaemonAction {
    /// Start the daemon in the background
    Start,
    /// Run the daemon in the foreground (for systemd)
    Run,
    /// Stop a running daemon
    Stop,
    /// Show daemon status (PID, uptime, RSS, activity)
    Status {
        /// JSON output
        #[arg(long)]
        json: bool,
    },
    /// Tail the daemon log
    Logs {
        /// Follow mode (like tail -f)
        #[arg(short = 'f', long)]
        follow: bool,

        /// Number of lines to show (default: 20)
        #[arg(short = 'n', long, default_value = "20")]
        lines: usize,
    },
    /// Install systemd user service unit (~/.config/systemd/user/agentscribe.service)
    Install,
    /// Remove systemd user service unit
    Uninstall,
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
        Commands::Status { json, plugin } => run_status(json, plugin),
        Commands::Index { action } => run_index(action),
        Commands::Search {
            query,
            error,
            code,
            lang,
            solution_only,
            like,
            session,
            agent,
            project,
            since,
            before,
            tag,
            outcome,
            r#type,
            model,
            fuzzy,
            edit_distance,
            max_results,
            snippet_length,
            token_budget,
            offset,
            sort,
            hint,
            json,
        } => run_search(
            query,
            error,
            code,
            lang,
            solution_only,
            like,
            session,
            agent,
            project,
            since,
            before,
            tag,
            outcome,
            r#type,
            model,
            fuzzy,
            edit_distance,
            max_results,
            snippet_length,
            token_budget,
            offset,
            sort,
            hint,
            json,
        ),
        Commands::Recurring {
            since,
            threshold,
            json,
        } => run_recurring(since, threshold, json),
        Commands::Daemon { action } => run_daemon(action),
        Commands::Rules {
            project_path,
            format,
            json,
        } => run_rules(project_path, format, json),
        Commands::Analytics {
            agent,
            project,
            since,
            json,
        } => run_analytics(agent, project, since, json),
        Commands::Gc {
            older_than,
            dry_run,
            json,
        } => run_gc(older_than, dry_run, json),
        Commands::Blame {
            spec,
            max_results,
            json,
        } => run_blame(spec, max_results, json),
        Commands::File {
            path,
            max_results,
            json,
        } => run_file(path, max_results, json),
        Commands::Summarize {
            session,
            since,
            project,
            max_results,
            write,
            json,
        } => run_summarize(session, since, project, max_results, write, json),
        Commands::ShellHook { shell } => run_shell_hook(&shell),
        Commands::FileKnowledge { file_path, json } => run_file_knowledge(&file_path, json),
        Commands::Digest {
            since,
            output,
            json,
        } => run_digest(since, output, json),
        Commands::PulseReport {
            quarter,
            output,
            format,
        } => run_pulse_report(quarter, output, format),
        Commands::Capacity {
            account_dir,
            cache_max_age,
            json,
        } => run_capacity(account_dir, cache_max_age, json),
        Commands::Transcribe {
            input,
            wait,
            timeout,
            json,
        } => run_transcribe(input, wait, timeout, json),
        Commands::Context {
            query,
            token_budget,
            project,
            json,
        } => run_context(query, token_budget, project, json),
        Commands::Completions { shell } => {
            let mut cmd = Args::command();
            generate(shell, &mut cmd, "agentscribe", &mut io::stdout());
            Ok(())
        }
    }
}

/// Run daemon commands
fn run_daemon(action: DaemonAction) -> Result<()> {
    // install/uninstall don't need the data directory to exist
    match action {
        DaemonAction::Install => return crate::daemon::install_service(),
        DaemonAction::Uninstall => return crate::daemon::uninstall_service(),
        _ => {}
    }

    let config = load_config()?;
    let data_dir = config.data_dir()?;

    // Ensure data directory exists for all other daemon commands
    if !data_dir.exists() {
        eprintln!("AgentScribe not initialized. Run 'agentscribe config init' to set up.");
        std::process::exit(1);
    }

    match action {
        DaemonAction::Start => {
            crate::daemon::start(&data_dir)?;
            Ok(())
        }
        DaemonAction::Run => crate::daemon::run(&data_dir),
        DaemonAction::Stop => crate::daemon::stop(&data_dir),
        DaemonAction::Status { json } => {
            let info = crate::daemon::status(&data_dir)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&info).unwrap());
            } else {
                print_daemon_status(&info);
            }
            Ok(())
        }
        DaemonAction::Logs { follow, lines } => crate::daemon::logs(&data_dir, follow, lines),
        // Already handled above
        DaemonAction::Install | DaemonAction::Uninstall => unreachable!(),
    }
}

/// Format daemon status for human-readable output
fn print_daemon_status(info: &crate::daemon::DaemonInfo) {
    if !info.running {
        println!("Daemon: stopped");
        return;
    }

    println!("Daemon: running");

    if let Some(pid) = info.pid {
        println!("  PID: {}", pid);
    }

    if let Some(secs) = info.uptime_secs {
        println!("  Uptime: {}", crate::daemon::format_duration(secs));
    }

    if let Some(rss) = info.rss_bytes {
        println!("  RSS: {}", crate::daemon::format_bytes(rss));
    }

    if let Some(peak) = info.peak_rss_bytes {
        println!("  Peak RSS: {}", crate::daemon::format_bytes(peak));
    }

    if let Some(sessions) = info.sessions_indexed {
        println!("  Sessions indexed: {}", sessions);
    }

    if let Some(ts) = info.last_scrape {
        println!("  Last scrape: {}", ts.to_rfc3339());
    }

    if let Some(ts) = info.started_at {
        println!("  Started at: {}", ts.to_rfc3339());
    }
}

/// Run config commands
fn run_config(action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Init { force } => {
            let data_dir = config::init(force)?;
            println!(
                "Initialized AgentScribe data directory: {}",
                data_dir.display()
            );
            Ok(())
        }
        ConfigAction::Show => {
            let config = load_config()?;
            let data_dir = config.data_dir()?;
            println!("Data directory: {}", data_dir.display());
            println!("Log level: {}", config.general.log_level);
            println!("\nScraping:");
            println!("  Debounce: {}s", config.scrape.debounce_seconds);
            println!(
                "  Max session age: {} days",
                config.scrape.max_session_age_days
            );
            println!("  Git auto-commit: {}", config.scrape.git_auto_commit);
            println!("\nSearch:");
            println!(
                "  Default max results: {}",
                config.search.default_max_results
            );
            println!(
                "  Default snippet length: {}",
                config.search.default_snippet_length
            );
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
                ["scrape", "git_auto_commit"] => {
                    config.scrape.git_auto_commit =
                        matches!(value.to_lowercase().as_str(), "true" | "1" | "yes");
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
                ["scrape", "max_session_age_days"] => {
                    config.scrape.max_session_age_days.to_string()
                }
                ["scrape", "git_auto_commit"] => config.scrape.git_auto_commit.to_string(),
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

    let mut scraper =
        Scraper::new_with_lock_timeout(data_dir.clone(), config.scrape.lock_timeout_seconds)?;
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
                scraper.state_manager().save()?;
                println!("Scraped {} session(s)", result.sessions_scraped);
                if result.sessions_indexed > 0 {
                    println!("Indexed {} session(s)", result.sessions_indexed);
                }
                if !result.errors.is_empty() {
                    eprintln!("Errors: {}", result.errors.len());
                }
                if config.scrape.git_auto_commit && git_auto_commit(&data_dir, &result)? {
                    println!("Git: committed {} session(s)", result.sessions_scraped);
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
                scraper.state_manager().save()?;
                println!(
                    "Scraped {} session(s) from {} file(s)",
                    result.sessions_scraped, result.files_processed
                );
                if result.sessions_indexed > 0 {
                    println!("Indexed {} session(s)", result.sessions_indexed);
                }
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
                if config.scrape.git_auto_commit && git_auto_commit(&data_dir, &result)? {
                    println!("Git: committed {} session(s)", result.sessions_scraped);
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
            println!("Scraped {} session(s) total", result.sessions_scraped);
            if result.sessions_indexed > 0 {
                println!("Indexed {} session(s)", result.sessions_indexed);
            }
            if !result.errors.is_empty() {
                eprintln!("Errors: {}", result.errors.len());
            }
            if config.scrape.git_auto_commit && git_auto_commit(&data_dir, &result)? {
                println!(
                    "Git: committed {} session(s) ({})",
                    result.sessions_scraped,
                    result.agent_types.join(", ")
                );
            }
        }
    }

    Ok(())
}

/// Run search command
#[allow(clippy::too_many_arguments)]
fn run_search(
    query: Option<String>,
    error: Option<String>,
    code: Option<String>,
    lang: Option<String>,
    solution_only: bool,
    like: Option<String>,
    session: Option<String>,
    agent: Vec<String>,
    project: Option<String>,
    since: Option<String>,
    before: Option<String>,
    tag: Vec<String>,
    outcome: Option<String>,
    doc_type_filter: Option<String>,
    model: Option<String>,
    fuzzy: bool,
    edit_distance: Option<u8>,
    max_results: usize,
    snippet_length: usize,
    token_budget: Option<usize>,
    offset: usize,
    sort: String,
    hint: bool,
    json: bool,
) -> Result<()> {
    let config = load_config()?;
    let data_dir = config.data_dir()?;

    let sort_order = match sort.as_str() {
        "newest" => SortOrder::Newest,
        "oldest" => SortOrder::Oldest,
        "turns" => SortOrder::Turns,
        _ => SortOrder::Relevance,
    };

    let since_dt = since.as_deref().map(search::parse_datetime).transpose()?;

    let before_dt = before.as_deref().map(search::parse_datetime).transpose()?;

    let opts = SearchOptions {
        query,
        error_pattern: error,
        code_query: code,
        code_lang: lang,
        solution_only,
        like_session: like,
        session_id: session,
        agent,
        project,
        since: since_dt,
        before: before_dt,
        tag,
        outcome,
        doc_type_filter,
        model,
        file_path: None,
        git_commit: None,
        fuzzy,
        fuzzy_distance: edit_distance.unwrap_or(config.search.fuzzy_edit_distance),
        max_results,
        snippet_length,
        token_budget,
        offset,
        sort: sort_order,
    };

    let output = search::execute_search(&data_dir, &opts)?;

    if hint {
        // Print a single-line hint for shell hook integration
        if let Some(result) = output.results.first() {
            let text = result
                .snippet
                .as_deref()
                .or(result.summary.as_deref())
                .unwrap_or("")
                .trim();
            let one_line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if !one_line.is_empty() {
                if one_line.len() > 120 {
                    println!("{}...", &one_line[..117]);
                } else {
                    println!("{}", one_line);
                }
            }
        }
    } else if json {
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        println!("{}", search::format_human(&output, snippet_length));
    }

    Ok(())
}

/// Run shell-hook command
fn run_shell_hook(shell: &str) -> Result<()> {
    let config = load_config().unwrap_or_default();
    let snippet = crate::shell_hook::generate_hook(shell, &config.shell_hook)?;
    print!("{}", snippet);
    Ok(())
}

/// Run index subcommands
fn run_index(action: IndexAction) -> Result<()> {
    let config = load_config()?;
    let data_dir = config.data_dir()?;

    if !data_dir.exists() {
        eprintln!("AgentScribe not initialized. Run 'agentscribe config init' to set up.");
        std::process::exit(1);
    }

    match action {
        IndexAction::Rebuild { plugin, heap_size } => {
            run_index_rebuild(&data_dir, plugin.as_deref(), heap_size)
        }
        IndexAction::Stats => run_index_stats(&data_dir),
        IndexAction::Optimize => run_index_optimize(&data_dir),
    }
}

/// Run index rebuild: drop existing index and rebuild from session files.
fn run_index_rebuild(
    data_dir: &std::path::Path,
    plugin_filter: Option<&str>,
    heap_size: usize,
) -> Result<()> {
    let sessions_dir = data_dir.join("sessions");

    if !sessions_dir.exists() {
        eprintln!("No sessions directory found. Run 'agentscribe scrape' first.");
        std::process::exit(2);
    }

    // Drop existing index
    let index_path = data_dir.join("index").join("tantivy");
    if index_path.exists() {
        println!("Dropping existing index...");
        std::fs::remove_dir_all(&index_path)?;
    }

    let mut manager = IndexManager::open(data_dir)?;
    manager.set_heap_size(heap_size);
    manager.begin_write()?;

    let mut total_docs = 0usize;
    let mut total_errors = 0usize;

    // Walk session directories
    let entries = match std::fs::read_dir(&sessions_dir) {
        Ok(e) => e,
        Err(_) => {
            eprintln!("Cannot read sessions directory.");
            std::process::exit(2);
        }
    };

    for agent_entry in entries.filter_map(|e| e.ok()) {
        let agent_path = agent_entry.path();
        if !agent_path.is_dir() {
            continue;
        }

        let agent_name = match agent_path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Apply plugin filter
        if let Some(filter) = plugin_filter {
            if agent_name != filter {
                continue;
            }
        }

        let session_files = match std::fs::read_dir(&agent_path) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for session_entry in session_files.filter_map(|e| e.ok()) {
            let session_path = session_entry.path();
            if session_path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }

            let session_id = format!(
                "{}/{}",
                agent_name,
                session_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
            );

            // Read events from the normalized session file
            let file = match std::fs::File::open(&session_path) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("Error reading {}: {}", session_path.display(), e);
                    total_errors += 1;
                    continue;
                }
            };

            use std::io::BufRead;
            let reader = std::io::BufReader::new(file);
            let mut events = Vec::new();

            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        if let Ok(event) = crate::event::Event::from_jsonl(&l) {
                            events.push(event);
                        }
                    }
                    Err(_) => {
                        total_errors += 1;
                        continue;
                    }
                }
            }

            if events.is_empty() {
                continue;
            }

            // Build manifest from events
            let first = &events[0];
            let manifest = crate::index::build_manifest_from_events(
                &events,
                &session_id,
                &agent_name,
                first.project.as_deref(),
                first.model.as_deref(),
            );

            match manager.index_session(&events, &manifest) {
                Ok(_) => total_docs += 1,
                Err(e) => {
                    eprintln!("Error indexing {}: {}", session_id, e);
                    total_errors += 1;
                }
            }
        }
    }

    manager.finish()?;

    println!(
        "Rebuilt index from {} session files ({} errors)",
        total_docs, total_errors
    );
    Ok(())
}

/// Show index statistics.
fn run_index_stats(data_dir: &std::path::Path) -> Result<()> {
    let index_path = data_dir.join("index").join("tantivy");

    if !index_path.exists() {
        println!("Index not found. Run 'agentscribe scrape' first.");
        return Ok(());
    }

    let index = match search::open_index(data_dir) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Error opening index: {}", e);
            std::process::exit(1);
        }
    };

    let reader = index.reader().map_err(|e| {
        crate::error::AgentScribeError::DataDir(format!("Failed to create reader: {}", e))
    })?;
    let searcher = reader.searcher();

    let num_docs = searcher.num_docs();
    let segments = index.searchable_segment_metas().map_err(|e| {
        crate::error::AgentScribeError::DataDir(format!("Failed to get segment metas: {}", e))
    })?;
    let num_segments = segments.len();
    let schema = index.schema();
    let fields: Vec<String> = schema
        .fields()
        .map(|(field, _)| schema.get_field_name(field).to_string())
        .collect();

    let size_bytes = dir_size(&index_path);

    println!("Tantivy index at {}", index_path.display());
    println!("  Documents:    {}", num_docs);
    println!("  Segments:     {}", num_segments);
    println!("  Size on disk: {}", format_bytes(size_bytes));
    println!("  Fields:       {}", fields.join(", "));

    Ok(())
}

/// Optimize the index by merging segments.
fn run_index_optimize(data_dir: &std::path::Path) -> Result<()> {
    let index_path = data_dir.join("index").join("tantivy");

    if !index_path.exists() {
        println!("Index not found. Run 'agentscribe scrape' first.");
        return Ok(());
    }

    let pre_size = dir_size(&index_path);

    let index = search::open_index(data_dir)?;
    let segment_ids = index.searchable_segment_ids().map_err(|e| {
        crate::error::AgentScribeError::DataDir(format!("Failed to get segment IDs: {}", e))
    })?;
    let num_segments = segment_ids.len();

    println!("Merging {} segments...", num_segments);

    let mut writer = index
        .writer::<tantivy::TantivyDocument>(50_000_000)
        .map_err(|e| {
            crate::error::AgentScribeError::DataDir(format!("Failed to open writer: {}", e))
        })?;

    // Merge all segments into one and wait for completion
    writer.merge(&segment_ids).wait().map_err(|e| {
        crate::error::AgentScribeError::DataDir(format!("Failed to merge segments: {}", e))
    })?;
    writer.garbage_collect_files().wait().map_err(|e| {
        crate::error::AgentScribeError::DataDir(format!("Failed to garbage collect: {}", e))
    })?;

    let post_size = dir_size(&index_path);

    println!(
        "Merged {} segments ({} → {})",
        num_segments,
        format_bytes(pre_size),
        format_bytes(post_size)
    );

    Ok(())
}

/// Run status command
fn run_status(json: bool, plugin_filter: Option<String>) -> Result<()> {
    let config = load_config()?;
    let data_dir = config.data_dir()?;
    let data_dir_str = data_dir.display().to_string();

    if !data_dir.exists() {
        eprintln!("AgentScribe not initialized. Run 'agentscribe config init' to set up.");
        std::process::exit(1);
    }

    let mut scraper = Scraper::new(data_dir.clone())?;
    scraper.load_plugins()?;

    // Determine which plugins to show
    let plugin_names: Vec<String> = if let Some(ref filter) = plugin_filter {
        if scraper.plugin_manager().get(filter).is_some() {
            vec![filter.clone()]
        } else {
            eprintln!("Plugin '{}' not found", filter);
            std::process::exit(1);
        }
    } else {
        scraper
            .plugin_manager()
            .names()
            .into_iter()
            .map(String::from)
            .collect()
    };

    // Gather scrape state
    let scrape_state = scraper.state_manager().get_all();

    // Collect per-plugin status
    let mut plugin_statuses = Vec::new();
    let mut _total_events: u64 = 0;
    let mut total_sources: usize = 0;
    let mut total_bytes: u64 = 0;

    for plugin_name in &plugin_names {
        let sessions = scraper.list_sessions(plugin_name)?;

        // Count events across all sessions for this plugin
        let mut plugin_events: u64 = 0;
        for session_id in &sessions {
            if let Ok(events) = scraper.read_session(session_id) {
                plugin_events += events.len() as u64;
            }
        }
        _total_events += plugin_events;

        // Get source paths and truncation_limit from plugin config
        let (source_paths, truncation_limit) = scraper
            .plugin_manager()
            .get(plugin_name)
            .map(|p| (p.source.paths.clone(), p.source.truncation_limit))
            .unwrap_or_default();

        // Find last scraped time and byte totals from scrape state for this plugin
        let plugin_files: Vec<_> = scrape_state
            .sources
            .iter()
            .filter(|(_, s)| s.plugin == *plugin_name)
            .collect();

        let last_scraped = plugin_files.iter().map(|(_, s)| s.last_scraped).max();

        let mut plugin_bytes: u64 = 0;
        for (_, file_state) in &plugin_files {
            plugin_bytes += file_state.last_byte_offset;
            total_bytes += file_state.last_byte_offset;
        }
        total_sources += plugin_files.len();

        plugin_statuses.push(PluginStatus {
            name: plugin_name.to_string(),
            sessions: sessions.len(),
            events: plugin_events,
            last_scraped,
            source_paths,
            source_files: plugin_files.len(),
            bytes: plugin_bytes,
            truncation_limit,
        });
    }

    // Daemon state (Phase 4 - check PID file if it exists)
    let daemon_status = get_daemon_status(&data_dir);

    // Disk usage of data directory
    let dir_bytes = dir_size(&data_dir);

    // Index stats (Phase 2 - check if index directory exists)
    let index_dir = data_dir.join("index");
    let index_stats = get_index_stats(&index_dir);

    if json {
        use serde_json::json;
        let mut plugins_json = Vec::new();
        for ps in &plugin_statuses {
            let mut p = json!({
                "name": ps.name,
                "sessions": ps.sessions,
                "events": ps.events,
                "source_files": ps.source_files,
                "bytes": ps.bytes,
                "source_paths": ps.source_paths,
            });
            if let Some(ts) = ps.last_scraped {
                p["last_scraped"] = json!(ts.to_rfc3339());
            } else {
                p["last_scraped"] = json!(null);
            }
            plugins_json.push(p);
        }

        let status = json!({
            "version": env!("CARGO_PKG_VERSION"),
            "data_dir": data_dir_str,
            "data_dir_bytes": dir_bytes,
            "plugins": plugins_json,
            "scrape_state": {
                "tracked_sources": total_sources,
                "total_bytes": total_bytes,
            },
            "daemon": {
                "running": daemon_status.running,
                "pid": daemon_status.pid,
            },
            "index": {
                "exists": index_stats.exists,
                "documents": index_stats.documents,
                "size_bytes": index_stats.size_bytes,
            },
        });

        println!("{}", serde_json::to_string_pretty(&status).unwrap());
    } else {
        println!("AgentScribe v{}", env!("CARGO_PKG_VERSION"));
        println!("Data dir: {} ({})", data_dir_str, format_bytes(dir_bytes));

        // Daemon
        if daemon_status.running {
            if let Some(pid) = daemon_status.pid {
                println!("Daemon: running (PID {})", pid);
            } else {
                println!("Daemon: running");
            }
        } else {
            println!("Daemon: stopped");
        }

        // Plugins
        println!("\nPlugins:");
        if plugin_statuses.is_empty() {
            println!("  (none)");
        }
        for ps in &plugin_statuses {
            let last = match ps.last_scraped {
                Some(ts) => format_ago(ts),
                None => "never scraped".to_string(),
            };
            println!(
                "  {:<14} {:>4} sessions  {:>6} events  {}  ({} source files, {})",
                ps.name,
                ps.sessions,
                ps.events,
                last,
                ps.source_files,
                format_bytes(ps.bytes)
            );
        }

        // Windsurf truncation warning
        for ps in &plugin_statuses {
            if ps.name == "windsurf" {
                if let Some(limit) = ps.truncation_limit {
                    println!(
                        "\n  WARNING: Windsurf retains at most {} conversations.",
                        limit
                    );
                    println!(
                        "  Old conversations are silently overwritten. Run 'agentscribe scrape'"
                    );
                    println!(
                        "  frequently (e.g., via 'agentscribe daemon start') to avoid data loss."
                    );
                    break;
                }
            }
        }

        // Index
        if index_stats.exists {
            println!(
                "\nIndex: {} documents, {} on disk",
                index_stats.documents,
                format_bytes(index_stats.size_bytes)
            );
        } else {
            println!("\nIndex: not built (run 'agentscribe index rebuild')");
        }

        // Scrape state
        println!(
            "\nScrape state: tracking offsets for {} source paths ({} total)",
            total_sources,
            format_bytes(total_bytes)
        );
    }

    Ok(())
}

/// Per-plugin status data
struct PluginStatus {
    name: String,
    sessions: usize,
    events: u64,
    last_scraped: Option<chrono::DateTime<chrono::Utc>>,
    source_paths: Vec<String>,
    source_files: usize,
    bytes: u64,
    truncation_limit: Option<u32>,
}

/// Daemon status info
struct DaemonStatus {
    running: bool,
    pid: Option<u32>,
}

/// Index stats
struct IndexStats {
    exists: bool,
    documents: usize,
    size_bytes: u64,
}

/// Check daemon status by reading PID file
fn get_daemon_status(data_dir: &std::path::Path) -> DaemonStatus {
    let pid_file = data_dir.join("agentscribe.pid");
    if let Ok(content) = std::fs::read_to_string(&pid_file) {
        if let Ok(pid) = content.trim().parse::<u32>() {
            // Check if process is actually running
            unsafe {
                // kill(pid, 0) checks if process exists without sending a signal
                if libc::kill(pid as i32, 0) == 0 {
                    return DaemonStatus {
                        running: true,
                        pid: Some(pid),
                    };
                }
            }
        }
    }
    DaemonStatus {
        running: false,
        pid: None,
    }
}

/// Get index stats
fn get_index_stats(index_dir: &std::path::Path) -> IndexStats {
    let tantivy_dir = index_dir.join("tantivy");
    if !tantivy_dir.exists() {
        return IndexStats {
            exists: false,
            documents: 0,
            size_bytes: 0,
        };
    }

    let size_bytes = dir_size(&tantivy_dir);

    // Read actual document count from Tantivy index
    let documents = match tantivy::Index::open_in_dir(&tantivy_dir) {
        Ok(index) => match index.reader() {
            Ok(reader) => reader.searcher().num_docs() as usize,
            Err(_) => 0,
        },
        Err(_) => 0,
    };

    IndexStats {
        exists: documents > 0 || size_bytes > 0,
        documents,
        size_bytes,
    }
}

/// Format bytes as human-readable size
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Format a timestamp as relative time ago
fn format_ago(ts: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let duration = now.signed_duration_since(ts);

    if duration.num_days() > 0 {
        format!("{}d ago", duration.num_days())
    } else if duration.num_hours() > 0 {
        format!("{}h ago", duration.num_hours())
    } else if duration.num_minutes() > 0 {
        format!("{}m ago", duration.num_minutes())
    } else {
        "just now".to_string()
    }
}

/// Recursively compute directory size in bytes
fn dir_size(path: &std::path::Path) -> u64 {
    if !path.exists() {
        return 0;
    }

    if path.is_file() {
        return path.metadata().map(|m| m.len()).unwrap_or(0);
    }

    let mut total: u64 = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(m) = p.metadata() {
                total += m.len();
            }
        }
    }
    total
}

/// Count files with a given extension recursively
#[allow(dead_code)]
fn count_files_recursive(path: &std::path::Path, ext: &str) -> usize {
    if !path.is_dir() {
        return 0;
    }

    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.is_dir() {
                count += count_files_recursive(&p, ext);
            } else if p.extension().and_then(|s| s.to_str()) == Some(ext) {
                count += 1;
            }
        }
    }
    count
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

/// Run recurring problem detection
fn run_recurring(since: Option<String>, threshold: usize, json: bool) -> Result<()> {
    let config = load_config()?;
    let data_dir = config.data_dir()?;

    if !data_dir.exists() {
        eprintln!("AgentScribe not initialized. Run 'agentscribe config init' to set up.");
        std::process::exit(1);
    }

    let since_dt = match since {
        Some(ref s) => search::parse_datetime(s)?,
        None => chrono::Utc::now() - chrono::Duration::days(30),
    };

    let opts = crate::recurring::RecurringOptions {
        since: since_dt,
        threshold,
    };

    let output = crate::recurring::detect_recurring(&data_dir, &opts)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        print!("{}", crate::recurring::format_human(&output));
    }

    Ok(())
}

/// Run rules extraction command
fn run_rules(project_path: PathBuf, format: String, json: bool) -> Result<()> {
    let config = load_config()?;
    let data_dir = config.data_dir()?;

    if !data_dir.exists() {
        eprintln!("AgentScribe not initialized. Run 'agentscribe config init' to set up.");
        std::process::exit(1);
    }

    let output_format = match crate::rules::OutputFormat::from_str(&format) {
        Some(f) => f,
        None => {
            eprintln!("Unknown format: {}. Use claude, cursor, or aider.", format);
            std::process::exit(1);
        }
    };

    let output = crate::rules::extract_rules(&data_dir, &project_path)?;

    if json {
        use serde_json::json;
        let rules_json: Vec<serde_json::Value> = output
            .rules
            .iter()
            .map(|r| match r {
                crate::rules::Rule::Correction(s) => json!({"type": "correction", "rule": s}),
                crate::rules::Rule::Convention(s) => json!({"type": "convention", "rule": s}),
                crate::rules::Rule::Context(s) => json!({"type": "context", "rule": s}),
                crate::rules::Rule::Warning(s) => json!({"type": "warning", "rule": s}),
            })
            .collect();

        let result = json!({
            "project_path": output.project_path.display().to_string(),
            "sessions_analyzed": output.sessions_analyzed,
            "rules": rules_json,
        });
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
    } else {
        if output.rules.is_empty() {
            println!("{}", crate::rules::format_human(&output));
            return Ok(());
        }

        let path = crate::rules::write_rules(&output, output_format, &project_path)?;
        println!(
            "Wrote {} rules to {} ({} sessions analyzed)",
            output.rules.len(),
            path.display(),
            output.sessions_analyzed,
        );
    }

    Ok(())
}

/// Run analytics command
fn run_analytics(
    agent: Option<String>,
    project: Option<String>,
    since: Option<String>,
    json: bool,
) -> Result<()> {
    let config = load_config()?;
    let data_dir = config.data_dir()?;

    if !data_dir.exists() {
        eprintln!("AgentScribe not initialized. Run 'agentscribe config init' to set up.");
        std::process::exit(1);
    }

    let since_dt = match since {
        Some(ref s) => Some(search::parse_datetime(s)?),
        None => None,
    };

    let opts = crate::analytics::AnalyticsOptions {
        agent,
        project,
        since: since_dt,
    };

    let output = crate::analytics::compute_analytics(&data_dir, &opts, &config)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        print!("{}", crate::analytics::format_human(&output));
    }

    Ok(())
}

/// Run garbage collection command
fn run_gc(older_than: Option<String>, dry_run: bool, json: bool) -> Result<()> {
    let config = load_config()?;
    let data_dir = config.data_dir()?;

    if !data_dir.exists() {
        eprintln!("AgentScribe not initialized. Run 'agentscribe config init' to set up.");
        std::process::exit(1);
    }

    // Determine max age: CLI arg > config > error
    let max_age = if let Some(ref duration_str) = older_than {
        crate::gc::parse_duration(duration_str)?
    } else if config.scrape.max_session_age_days > 0 {
        chrono::Duration::days(config.scrape.max_session_age_days as i64)
    } else {
        eprintln!("No max session age configured. Set scrape.max_session_age_days in config.toml or use --older-than.");
        std::process::exit(1);
    };

    let result = crate::gc::run_gc(&data_dir, max_age, dry_run)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
    } else {
        print!("{}", crate::gc::format_human(&result));
    }

    Ok(())
}

/// Run blame command: show sessions associated with a file/line via git blame.
fn run_blame(spec: String, max_results: usize, json: bool) -> Result<()> {
    let config = load_config()?;
    let data_dir = config.data_dir()?;

    if !data_dir.exists() {
        eprintln!("AgentScribe not initialized. Run 'agentscribe config init' to set up.");
        std::process::exit(1);
    }

    let (file, line) = parse_blame_spec(&spec);

    // If a line number was given, try to resolve the commit hash via git blame.
    let commit_hash = if let Some(ln) = line {
        git_blame_commit(&file, ln)
    } else {
        None
    };

    let opts = if let Some(hash) = commit_hash {
        search::SearchOptions {
            git_commit: Some(hash),
            max_results,
            sort: search::SortOrder::Newest,
            ..Default::default()
        }
    } else {
        search::SearchOptions {
            file_path: Some(file.clone()),
            max_results,
            sort: search::SortOrder::Newest,
            ..Default::default()
        }
    };

    let output = match search::execute_search(&data_dir, &opts) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("Search error: {}", e);
            std::process::exit(1);
        }
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        if let Some(ln) = line {
            println!("Sessions related to {}:{}", file, ln);
        } else {
            println!("Sessions that touched: {}", file);
        }
        println!("{}", search::format_human(&output, 200));
    }

    Ok(())
}

/// Run file command: show all sessions that touched a file, chronologically.
fn run_file(path: String, max_results: usize, json: bool) -> Result<()> {
    let config = load_config()?;
    let data_dir = config.data_dir()?;

    if !data_dir.exists() {
        eprintln!("AgentScribe not initialized. Run 'agentscribe config init' to set up.");
        std::process::exit(1);
    }

    let opts = search::SearchOptions {
        file_path: Some(path.clone()),
        max_results,
        sort: search::SortOrder::Oldest,
        ..Default::default()
    };

    let output = match search::execute_search(&data_dir, &opts) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("Search error: {}", e);
            std::process::exit(1);
        }
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        println!("Sessions that touched: {}", path);
        println!("{}", search::format_human(&output, 200));
    }

    Ok(())
}

/// A summary entry for a single session (used in run_summarize).
#[derive(serde::Serialize)]
struct SummaryEntry {
    session_id: String,
    summary: String,
    project: Option<String>,
    started: chrono::DateTime<chrono::Utc>,
    outcome: Option<String>,
    files_touched: Vec<String>,
}

/// Run summarize command: generate Markdown summaries for sessions.
fn run_summarize(
    session: Option<String>,
    since: Option<String>,
    project: Option<String>,
    max_results: usize,
    write: bool,
    json: bool,
) -> Result<()> {
    let config = load_config()?;
    let data_dir = config.data_dir()?;

    if !data_dir.exists() {
        eprintln!("AgentScribe not initialized. Run 'agentscribe config init' to set up.");
        std::process::exit(1);
    }

    let sessions_dir = data_dir.join("sessions");
    if !sessions_dir.exists() {
        println!("No sessions found. Run 'agentscribe scrape' first.");
        return Ok(());
    }

    let since_dt = since.as_deref().map(search::parse_datetime).transpose()?;

    let mut entries: Vec<SummaryEntry> = Vec::new();

    if let Some(ref sid) = session {
        // Specific session
        let events = read_session_events(&sessions_dir, sid)?;
        if events.is_empty() {
            eprintln!("Session '{}' not found or has no events", sid);
            std::process::exit(1);
        }
        let agent = sid.split('/').next().unwrap_or("unknown");
        let manifest = crate::index::build_manifest_from_events(
            &events,
            sid,
            agent,
            events.first().and_then(|e| e.project.as_deref()),
            events.first().and_then(|e| e.model.as_deref()),
        );
        let summary = crate::enrichment::generate_summary(&events, &manifest);

        if write {
            write_summary_file(&sessions_dir, sid, &summary, &manifest)?;
        }

        entries.push(SummaryEntry {
            session_id: sid.clone(),
            summary,
            project: manifest.project,
            started: manifest.started,
            outcome: manifest.outcome,
            files_touched: manifest.files_touched,
        });
    } else {
        // Walk sessions directory
        let session_ids = collect_all_session_ids(&sessions_dir);
        let mut count = 0;

        for sid in session_ids {
            if count >= max_results {
                break;
            }

            let events = match read_session_events(&sessions_dir, &sid) {
                Ok(e) if !e.is_empty() => e,
                _ => continue,
            };

            let agent = sid.split('/').next().unwrap_or("unknown");
            let manifest = crate::index::build_manifest_from_events(
                &events,
                &sid,
                agent,
                events.first().and_then(|e| e.project.as_deref()),
                events.first().and_then(|e| e.model.as_deref()),
            );

            // Apply filters
            if let Some(ref proj) = project {
                if manifest.project.as_deref() != Some(proj.as_str()) {
                    continue;
                }
            }
            if let Some(since) = since_dt {
                if manifest.started < since {
                    continue;
                }
            }

            let summary = crate::enrichment::generate_summary(&events, &manifest);

            if write {
                write_summary_file(&sessions_dir, &sid, &summary, &manifest)?;
            }

            entries.push(SummaryEntry {
                session_id: sid.clone(),
                summary,
                project: manifest.project,
                started: manifest.started,
                outcome: manifest.outcome,
                files_touched: manifest.files_touched,
            });

            count += 1;
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&entries).unwrap());
    } else if write {
        println!(
            "Wrote {} summary file(s) to {}",
            entries.len(),
            sessions_dir.display()
        );
    } else {
        for entry in &entries {
            println!("# Session: {}", entry.session_id);
            println!();
            println!("**Summary:** {}", entry.summary);
            println!();
            if let Some(ref outcome) = entry.outcome {
                println!("- **Outcome:** {}", outcome);
            }
            println!("- **Started:** {}", entry.started.to_rfc3339());
            if let Some(ref proj) = entry.project {
                println!("- **Project:** {}", proj);
            }
            if !entry.files_touched.is_empty() {
                let files: Vec<&str> = entry
                    .files_touched
                    .iter()
                    .take(5)
                    .map(|s| s.as_str())
                    .collect();
                println!("- **Files touched:** {}", files.join(", "));
            }
            println!();
            println!("---");
            println!();
        }
        if entries.is_empty() {
            println!("No sessions found.");
        }
    }

    Ok(())
}

/// Parse a blame spec: "file.rs" or "file.rs:42"
fn parse_blame_spec(spec: &str) -> (String, Option<u32>) {
    if let Some(colon_pos) = spec.rfind(':') {
        let file = spec[..colon_pos].to_string();
        let line_str = &spec[colon_pos + 1..];
        if let Ok(line) = line_str.parse::<u32>() {
            return (file, Some(line));
        }
    }
    (spec.to_string(), None)
}

/// Run git blame to find the commit hash for a specific file/line.
/// Returns None if git is unavailable or the line is uncommitted.
fn git_blame_commit(file: &str, line: u32) -> Option<String> {
    let output = std::process::Command::new("git")
        .args([
            "blame",
            "-L",
            &format!("{},{}", line, line),
            "--porcelain",
            file,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let hash = stdout.lines().next()?.split_whitespace().next()?;

    // All-zeros means the line is not yet committed
    if hash.chars().all(|c| c == '0') {
        return None;
    }

    Some(hash.to_string())
}

/// Read session events from the normalized sessions directory.
fn read_session_events(
    sessions_dir: &std::path::Path,
    session_id: &str,
) -> Result<Vec<crate::event::Event>> {
    let parts: Vec<&str> = session_id.splitn(2, '/').collect();
    if parts.len() != 2 {
        return Ok(Vec::new());
    }
    let path = sessions_dir
        .join(parts[0])
        .join(format!("{}.jsonl", parts[1]));
    if !path.exists() {
        return Ok(Vec::new());
    }

    use std::io::BufRead;
    let file = std::fs::File::open(&path)?;
    let reader = std::io::BufReader::new(file);
    let mut events = Vec::new();

    for line in reader.lines() {
        let l = line?;
        if l.trim().is_empty() {
            continue;
        }
        if let Ok(event) = crate::event::Event::from_jsonl(&l) {
            events.push(event);
        }
    }

    Ok(events)
}

/// Collect all session IDs by walking the sessions directory.
fn collect_all_session_ids(sessions_dir: &std::path::Path) -> Vec<String> {
    let mut ids = Vec::new();

    let Ok(agents) = std::fs::read_dir(sessions_dir) else {
        return ids;
    };

    for agent_entry in agents.filter_map(|e| e.ok()) {
        let agent_path = agent_entry.path();
        if !agent_path.is_dir() {
            continue;
        }
        let Some(agent_name) = agent_path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        let Ok(sessions) = std::fs::read_dir(&agent_path) else {
            continue;
        };

        for session_entry in sessions.filter_map(|e| e.ok()) {
            let session_path = session_entry.path();
            if session_path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Some(stem) = session_path.file_stem().and_then(|s| s.to_str()) {
                ids.push(format!("{}/{}", agent_name, stem));
            }
        }
    }

    ids
}

/// Write a Markdown summary file alongside the session JSONL.
fn write_summary_file(
    sessions_dir: &std::path::Path,
    session_id: &str,
    summary: &str,
    manifest: &crate::event::SessionManifest,
) -> Result<()> {
    let parts: Vec<&str> = session_id.splitn(2, '/').collect();
    if parts.len() != 2 {
        return Ok(());
    }
    let md_path = sessions_dir.join(parts[0]).join(format!("{}.md", parts[1]));

    let mut content = format!("# Session: {}\n\n", session_id);
    content.push_str(&format!("**Summary:** {}\n\n", summary));

    if let Some(ref outcome) = manifest.outcome {
        content.push_str(&format!("- **Outcome:** {}\n", outcome));
    }
    content.push_str(&format!(
        "- **Started:** {}\n",
        manifest.started.to_rfc3339()
    ));
    if let Some(ref proj) = manifest.project {
        content.push_str(&format!("- **Project:** {}\n", proj));
    }
    if !manifest.files_touched.is_empty() {
        let files: Vec<&str> = manifest
            .files_touched
            .iter()
            .take(10)
            .map(|s| s.as_str())
            .collect();
        content.push_str(&format!("- **Files touched:** {}\n", files.join(", ")));
    }
    content.push('\n');

    std::fs::write(&md_path, content)?;
    Ok(())
}

/// Run file knowledge command
fn run_file_knowledge(file_path: &str, json: bool) -> Result<()> {
    let config = load_config()?;
    let data_dir = config.data_dir()?;

    if !data_dir.exists() {
        eprintln!("AgentScribe not initialized. Run 'agentscribe config init' to set up.");
        std::process::exit(1);
    }

    let knowledge = crate::file_knowledge::build_file_knowledge(&data_dir, file_path, &config)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&knowledge).unwrap());
    } else {
        print!("{}", crate::file_knowledge::format_human(&knowledge));
    }

    Ok(())
}

/// Run pulse-report command
fn run_pulse_report(quarter: String, output: Option<String>, format: String) -> Result<()> {
    let config = load_config()?;
    let data_dir = config.data_dir()?;

    if !data_dir.exists() {
        eprintln!("AgentScribe not initialized. Run 'agentscribe config init' to set up.");
        std::process::exit(1);
    }

    let fmt = match crate::pulse_report::ReportFormat::parse(&format) {
        Some(f) => f,
        None => {
            eprintln!("Unknown format '{}'. Use markdown, html, or json.", format);
            std::process::exit(1);
        }
    };

    let opts = crate::pulse_report::PulseReportOptions {
        quarter: quarter.clone(),
        output: output.clone(),
        format: fmt,
    };

    let report = crate::pulse_report::generate_pulse_report(&data_dir, &opts, &config)?;

    let content = match fmt {
        crate::pulse_report::ReportFormat::Markdown => {
            crate::pulse_report::format_markdown(&report)
        }
        crate::pulse_report::ReportFormat::Html => crate::pulse_report::format_html(&report),
        crate::pulse_report::ReportFormat::Json => crate::pulse_report::format_json(&report),
    };

    if let Some(ref path) = output {
        std::fs::write(path, &content)?;
        eprintln!("Pulse Report ({}) written to {}", report.quarter, path);
    } else {
        print!("{}", content);
    }

    Ok(())
}

/// Run capacity command
fn run_capacity(account_dirs: Vec<PathBuf>, cache_max_age: u64, json: bool) -> Result<()> {
    use crate::capacity::{CapacityMeter, CapacityMeterConfig};

    let config = if account_dirs.is_empty() {
        CapacityMeterConfig::default()
    } else {
        CapacityMeterConfig {
            account_dirs,
            cache_max_age_secs: cache_max_age,
            cache_base_dir: None,
        }
    };

    let meter = CapacityMeter::new(config);
    let accounts = meter.compute();

    if json {
        println!("{}", serde_json::to_string_pretty(&accounts).unwrap());
        return Ok(());
    }

    if accounts.is_empty() {
        println!("No Claude accounts found.");
        println!("Expected: ~/.claude/.credentials.json (or ~/.claude-*/.credentials.json)");
        return Ok(());
    }

    println!("Claude Code Capacity\n");

    for acct in &accounts {
        println!(
            "Account: {} ({} / {})",
            acct.account_id, acct.plan_type, acct.rate_limit_tier
        );
        println!("  Source: {}", acct.source);

        let bar_5h = util_bar(acct.utilization_5h);
        let reset_5h = acct
            .resets_at_5h
            .map(|dt| format!("  resets {}", format_resets_at(dt)))
            .unwrap_or_default();
        println!(
            "  5h window:  {:5.1}%  {}{}",
            acct.utilization_5h, bar_5h, reset_5h
        );

        let bar_7d = util_bar(acct.utilization_7d);
        let reset_7d = acct
            .resets_at_7d
            .map(|dt| format!("  resets {}", format_resets_at(dt)))
            .unwrap_or_default();
        println!(
            "  7d window:  {:5.1}%  {}{}",
            acct.utilization_7d, bar_7d, reset_7d
        );

        if !acct.model_windows_7d.is_empty() {
            for mw in &acct.model_windows_7d {
                let bar = util_bar(mw.utilization);
                println!("    {:10} {:5.1}%  {}", mw.model, mw.utilization, bar);
            }
        }

        if acct.burn_rate_per_min > 0.0 {
            println!("  Burn rate:  {:.0} tokens/min", acct.burn_rate_per_min);
            if let Some(mins) = acct.forecast_full_5h_min {
                if mins > 0.0 {
                    println!("  Forecast:   5h full in {}", format_minutes(mins));
                } else {
                    println!("  Forecast:   5h window full");
                }
            }
        }

        println!(
            "  Turns:      {} (5h)  {} (7d)",
            acct.turns_5h, acct.turns_7d
        );
        println!();
    }

    Ok(())
}

fn util_bar(util: f64) -> String {
    let filled = ((util / 100.0) * 20.0).round() as usize;
    let filled = filled.min(20);
    let empty = 20 - filled;
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

fn format_resets_at(dt: chrono::DateTime<chrono::Utc>) -> String {
    use chrono::Utc;
    let now = Utc::now();
    let delta = dt - now;
    let total_secs = delta.num_seconds();
    if total_secs <= 0 {
        return "now".to_string();
    }
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    if hours > 0 {
        format!("in {}h {}m", hours, mins)
    } else {
        format!("in {}m", mins)
    }
}

fn format_minutes(mins: f64) -> String {
    if mins < 60.0 {
        format!("{:.0}m", mins)
    } else {
        format!("{:.1}h", mins / 60.0)
    }
}

/// Run transcribe command
fn run_transcribe(input: PathBuf, wait: bool, timeout: u64, json: bool) -> Result<()> {
    let config = load_config()?;

    if !config.whisper.enabled {
        eprintln!(
            "Whisper transcription is disabled.\n\
             Enable it in config.toml:\n\n\
             [whisper]\n\
             enabled = true\n\
             model_path = \"~/.agentscribe/models/ggml-base.bin\"\n\
             backend = \"whisper_cpp\""
        );
        std::process::exit(1);
    }

    if !input.exists() {
        eprintln!("File not found: {}", input.display());
        std::process::exit(1);
    }

    let rt = tokio::runtime::Runtime::new().map_err(|e| {
        crate::error::AgentScribeError::Transcription(format!(
            "failed to create async runtime: {}",
            e
        ))
    })?;

    // For fire-and-forget (--no-wait), submit to the queue and print job ID.
    if !wait {
        let job_id = rt.block_on(async {
            let queue = crate::transcription::TranscriptionQueue::new(
                config.whisper.clone(),
                config.redaction.clone(),
            );
            queue.submit(input, config.whisper.max_retries).await
        })?;
        println!("Transcription job submitted: {}", job_id);
        return Ok(());
    }

    // Synchronous path: run transcription and wait for the result.
    let job = rt.block_on(async {
        let queue = crate::transcription::TranscriptionQueue::new(
            config.whisper.clone(),
            config.redaction.clone(),
        );
        let job_id = queue.submit(input, config.whisper.max_retries).await?;
        queue
            .wait_for_job(&job_id, std::time::Duration::from_secs(timeout))
            .await
    })?;

    let result = match job.result {
        Some(r) => r,
        None => {
            eprintln!(
                "Transcription failed: {}",
                job.error.as_deref().unwrap_or("unknown error")
            );
            std::process::exit(1);
        }
    };

    // ── Output ───────────────────────────────────────────────────────────────
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&result).map_err(|e| {
                crate::error::AgentScribeError::Transcription(format!(
                    "JSON serialization failed: {}",
                    e
                ))
            })?
        );
    } else {
        // Warning card for degraded results.
        if result.has_warnings {
            eprintln!("┌─ TRANSCRIPTION WARNINGS ─────────────────────────────────────┐");
            for w in &result.warnings {
                eprintln!("│  {}", w);
            }
            eprintln!("└──────────────────────────────────────────────────────────────┘");
        }

        println!("{}", result.full_text);

        // Summary line with timestamp granularity info.
        let level = match result.timestamp_level {
            crate::transcription::TimestampLevel::Word => "word-level",
            crate::transcription::TimestampLevel::Utterance => "utterance-level (fallback)",
        };
        eprintln!(
            "[{} segments, {}, transcribed at {}]",
            result.segment_count(),
            level,
            result.transcribed_at.to_rfc3339(),
        );
    }

    Ok(())
}

/// Run digest command
fn run_digest(since: String, output: Option<String>, json: bool) -> Result<()> {
    let config = load_config()?;
    let data_dir = config.data_dir()?;

    if !data_dir.exists() {
        eprintln!("AgentScribe not initialized. Run 'agentscribe config init' to set up.");
        std::process::exit(1);
    }

    let since_dt = search::parse_datetime(&since)?;

    let opts = crate::digest::DigestOptions {
        since: since_dt,
        output: output.clone(),
        json,
    };

    let digest_output = crate::digest::generate_digest(&data_dir, &opts, &config)?;

    let content = if json {
        crate::digest::format_json(&digest_output)
    } else {
        crate::digest::format_markdown(&digest_output)
    };

    if let Some(path) = output {
        std::fs::write(&path, &content)?;
        eprintln!("Digest written to {}", path);
    } else {
        print!("{}", content);
    }

    Ok(())
}

/// Run context command: pre-task priming query for agent workers
fn run_context(
    query: String,
    token_budget: usize,
    project: Option<PathBuf>,
    json: bool,
) -> Result<()> {
    let config = load_config()?;
    let data_dir = config.data_dir()?;

    if !data_dir.exists() {
        eprintln!("AgentScribe not initialized. Run 'agentscribe config init' to set up.");
        std::process::exit(1);
    }

    // Store the current_dir in a variable to avoid lifetime issues
    let current_dir = std::env::current_dir().ok();
    let project_path = if let Some(ref p) = project {
        Some(p.as_path())
    } else {
        current_dir.as_deref()
    };

    let pack = crate::search::context_pack(&data_dir, &query, token_budget, project_path)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&pack).unwrap());
    } else {
        print!("{}", pack.format_text());
    }

    Ok(())
}

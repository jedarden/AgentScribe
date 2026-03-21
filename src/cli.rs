//! CLI commands for AgentScribe

use crate::config::{self, Config};
use crate::error::Result;
use crate::plugin::validate_plugin_file;
use crate::scraper::Scraper;
use crate::search::{self, SearchOptions, SortOrder};
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
        Commands::Status { json, plugin } => run_status(json, plugin),
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
            max_results,
            snippet_length,
            token_budget,
            offset,
            sort,
            json,
        } => run_search(
            query, error, code, lang, solution_only, like, session,
            agent, project, since, before, tag, outcome, r#type, model,
            fuzzy, max_results, snippet_length, token_budget, offset, sort, json,
        ),
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

/// Run search command
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
    max_results: usize,
    snippet_length: usize,
    token_budget: Option<usize>,
    offset: usize,
    sort: String,
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

    let since_dt = since
        .as_deref()
        .map(search::parse_datetime)
        .transpose()?;

    let before_dt = before
        .as_deref()
        .map(search::parse_datetime)
        .transpose()?;

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
        fuzzy,
        max_results,
        snippet_length,
        token_budget,
        offset,
        sort: sort_order,
    };

    let output = search::execute_search(&data_dir, &opts)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        println!("{}", search::format_human(&output, snippet_length));
    }

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
        scraper.plugin_manager().names()
            .into_iter()
            .map(String::from)
            .collect()
    };

    // Gather scrape state
    let scrape_state = scraper.state_manager().get_all();

    // Collect per-plugin status
    let mut plugin_statuses = Vec::new();
    let mut total_events: u64 = 0;
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
        total_events += plugin_events;

        // Get source paths from plugin config
        let source_paths = scraper.plugin_manager()
            .get(plugin_name)
            .map(|p| p.source.paths.clone())
            .unwrap_or_default();

        // Find last scraped time and byte totals from scrape state for this plugin
        let plugin_files: Vec<_> = scrape_state.sources.iter()
            .filter(|(_, s)| s.plugin == *plugin_name)
            .collect();

        let last_scraped = plugin_files.iter()
            .filter_map(|(_, s)| Some(s.last_scraped))
            .max();

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

        let mut status = json!({
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
                ps.name, ps.sessions, ps.events, last, ps.source_files, format_bytes(ps.bytes)
            );
        }

        // Index
        if index_stats.exists {
            println!(
                "\nIndex: {} documents, {} on disk",
                index_stats.documents,
                format_bytes(index_stats.size_bytes)
            );
        } else {
            println!("\nIndex: not built (run 'agentscribe index build')");
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
    if !index_dir.exists() {
        return IndexStats {
            exists: false,
            documents: 0,
            size_bytes: 0,
        };
    }

    let size_bytes = dir_size(index_dir);

    // Count JSONL session files as document proxy (index not yet built in Phase 1)
    let documents = count_files_recursive(index_dir, "jsonl");

    IndexStats {
        exists: !documents == 0 || size_bytes > 0,
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

# AgentScribe Test File Import Analysis

Generated: 2026-08-12
Total test files analyzed: 65

## /home/coding/AgentScribe/src/analytics.rs

### Use Statements (19)
```rust
use crate::config::Config;
use crate::error::{AgentScribeError, Result};
use crate::index::build_schema;
use crate::scraper::load_session_content;
use crate::search::open_index;
use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;
use tantivy::collector::TopDocs;
use tantivy::query::AllQuery;
use tantivy::schema::Value;
use tantivy::{DocAddress, Searcher, TantivyDocument};
use super::*;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use tempfile::TempDir;
```

## /home/coding/AgentScribe/src/annotations.rs

### Use Statements (8)
```rust
use crate::error::{AgentScribeError, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use super::*;
use tempfile::TempDir;
```

## /home/coding/AgentScribe/src/capacity.rs

### Use Statements (11)
```rust
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use tracing::debug;
use super::*;
use std::io::Write;
use tempfile::TempDir;
```

## /home/coding/AgentScribe/src/config.rs

### Use Statements (8)
```rust
use crate::enrichment::outcome::OutcomeConfig;
use crate::error::{AgentScribeError, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use super::*;
```

## /home/coding/AgentScribe/src/daemon.rs

### Use Statements (24)
```rust
use crate::config::Config as AppConfig;
use crate::error::{AgentScribeError, Result};
use crate::plugin::Plugin;
use crate::scraper::{git_auto_commit as scraper_git_commit, Scraper};
use chrono::{DateTime, Utc};
use glob::Pattern as GlobPattern;
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use notify::EventKind;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::EnvFilter;
use std::io::{BufRead, Seek, SeekFrom};
use crate::config::Config;
use super::*;
use crate::plugin::{LogFormat, Parser, Plugin, PluginMeta, SessionDetection, Source};
use std::thread;
```

## /home/coding/AgentScribe/src/digest.rs

### Use Statements (14)
```rust
use crate::analytics::{self, AnalyticsOptions, AnalyticsOutput};
use crate::config::Config;
use crate::error::Result;
use crate::recurring::{self, RecurringOptions};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use crate::analytics::{estimate_cost, estimate_tokens};
use crate::index::build_schema;
use crate::search::open_index;
use tantivy::collector::TopDocs;
use tantivy::query::AllQuery;
use super::*;
```

## /home/coding/AgentScribe/src/embedding.rs

### Use Statements (5)
```rust
use crate::config::VectorConfig;
use crate::error::{AgentScribeError, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use super::*;
```

## /home/coding/AgentScribe/src/enrichment/antipatterns.rs

### Use Statements (9)
```rust
use std::path::Path;
use serde::{Deserialize, Serialize};
use crate::enrichment::solution;
use crate::event::{Event, Role, SessionManifest};
use crate::scraper::Scraper;
use std::io::Write;
use super::*;
use crate::event::Event;
use chrono::Utc;
```

## /home/coding/AgentScribe/src/enrichment/behavioral_signals.rs

### Use Statements (7)
```rust
use crate::event::{Event, Role};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use super::*;
use crate::event::Event;
use chrono::Utc;
use serde_json::json;
```

## /home/coding/AgentScribe/src/enrichment/code_artifacts.rs

### Use Statements (7)
```rust
use std::collections::HashMap;
use std::sync::LazyLock;
use serde::{Deserialize, Serialize};
use crate::event::{Event, Role};
use super::*;
use crate::event::Event;
use chrono::Utc;
```

## /home/coding/AgentScribe/src/enrichment/config_change_tracker.rs

### Use Statements (11)
```rust
use crate::error::{AgentScribeError, Result};
use crate::event::Event;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use glob::Pattern;
use super::*;
use std::io::Write;
use tempfile::TempDir;
```

## /home/coding/AgentScribe/src/enrichment/errors.rs

### Use Statements (6)
```rust
use std::sync::LazyLock;
use regex::Regex;
use crate::event::Event;
use super::*;
use crate::event::Role;
use chrono::Utc;
```

## /home/coding/AgentScribe/src/enrichment/git.rs

### Use Statements (6)
```rust
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use super::*;
```

## /home/coding/AgentScribe/src/enrichment/outcome.rs

### Use Statements (6)
```rust
use crate::event::{Event, Role, SessionManifest};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use super::*;
use chrono::Utc;
```

## /home/coding/AgentScribe/src/enrichment/solution.rs

### Use Statements (4)
```rust
use crate::event::{Event, Role};
use super::*;
use crate::event::Event;
use chrono::Utc;
```

## /home/coding/AgentScribe/src/enrichment/summary.rs

### Use Statements (3)
```rust
use crate::event::{Event, Role, SessionManifest};
use super::*;
use chrono::Utc;
```

## /home/coding/AgentScribe/src/event.rs

### Use Statements (4)
```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use super::*;
```

## /home/coding/AgentScribe/src/file_knowledge.rs

### Use Statements (16)
```rust
use crate::analytics;
use crate::config::Config;
use crate::error::Result;
use crate::index::build_schema;
use crate::search::open_index;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use tantivy::collector::TopDocs;
use tantivy::query::BooleanQuery;
use tantivy::query::TermQuery;
use tantivy::schema::IndexRecordOption;
use super::*;
use crate::event::{Event, Role, SessionManifest};
use crate::index::IndexManager;
```

## /home/coding/AgentScribe/src/gc.rs

### Use Statements (8)
```rust
use crate::error::{AgentScribeError, Result};
use crate::index::IndexManager;
use crate::scraper::Scraper;
use chrono::{Duration, Utc};
use serde::Serialize;
use std::fs;
use std::path::Path;
use super::*;
```

## /home/coding/AgentScribe/src/index.rs

### Use Statements (16)
```rust
use crate::error::{AgentScribeError, Result};
use crate::event::{Event, Role, SessionManifest};
use crate::tags;
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::path::Path;
use tracing::{debug, info};
use tantivy::schema::*;
use tantivy::TantivyDocument;
use super::*;
use chrono::Duration;
use tempfile::TempDir;
use tantivy::collector::Count;
use tantivy::query::TermQuery;
use tantivy::schema::IndexRecordOption;
use tantivy::collector::TopDocs;
```

## /home/coding/AgentScribe/src/mcp.rs

### Use Statements (12)
```rust
use crate::search::{execute_search, parse_datetime, SearchOptions, SortOrder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::task;
use crate::scraper::Scraper;
use super::*;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
```

## /home/coding/AgentScribe/src/parser/aider_input.rs

### Use Statements (10)
```rust
use crate::error::{AgentScribeError, Result};
use chrono::{DateTime, Utc};
use regex::Regex;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use super::*;
use std::io::Write;
use tempfile::NamedTempFile;
```

## /home/coding/AgentScribe/src/parser/json_array.rs

### Use Statements (12)
```rust
use crate::error::{AgentScribeError, Result};
use crate::event::{Event, Role, TokenCounts};
use crate::parser::extract_field;
use crate::parser::{extract_string, parse_timestamp, ParseContext, SessionInfo};
use crate::plugin::{Plugin, ProjectDetection, SessionDetection, SessionIdSource};
use chrono::Utc;
use serde_json::Value;
use std::fs;
use std::path::Path;
use super::*;
use crate::parser::FormatParser;
use crate::plugin::{;
```

## /home/coding/AgentScribe/src/parser/json_tree.rs

### Use Statements (10)
```rust
use crate::error::{AgentScribeError, Result};
use crate::event::{Event, Role};
use crate::parser::{extract_string, SessionInfo};
use crate::plugin::{Plugin, TreeConfig};
use chrono::{DateTime, Utc};
use glob::Pattern;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use super::*;
```

## /home/coding/AgentScribe/src/parser/jsonl.rs

### Use Statements (13)
```rust
use crate::error::{AgentScribeError, Result};
use crate::event::{Event, Role, TokenCounts};
use crate::parser::{;
use crate::plugin::{Plugin, SessionDetection, SessionIdSource};
use chrono::Utc;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::Path;
use tracing::warn;
use super::*;
use crate::parser::FormatParser;
use crate::plugin::{;
use std::path::PathBuf;
```

## /home/coding/AgentScribe/src/parser/jsonl/jsonl_subagent_test.rs

### Use Statements (5)
```rust
use crate::parser::FormatParser;
use crate::plugin::{;
use super::*;
use crate::parser::jsonl::JsonlParser;
use crate::scraper::Scraper;
```

## /home/coding/AgentScribe/src/parser/markdown.rs

### Use Statements (18)
```rust
use super::aider_input::AiderInputHistory;
use crate::error::{AgentScribeError, Result};
use crate::event::{Event, Role};
use crate::parser::{ParseContext, SessionInfo};
use crate::plugin::{Plugin, SessionDetection};
use chrono::Utc;
use regex::Regex;
use std::path::Path;
use tracing::debug;
use super::*;
use crate::parser::FormatParser;
use crate::plugin::{LogFormat, Parser, Plugin, PluginMeta, SessionDetection, Source};
use std::fs::File;
use std::io::Write;
use tempfile::NamedTempFile;
use std::fs;
use tempfile::TempDir;
use std::path::PathBuf;
```

## /home/coding/AgentScribe/src/parser/mod.rs

### Use Statements (8)
```rust
use crate::error::{AgentScribeError, Result};
use crate::event::Event;
use crate::plugin::Plugin;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::path::Path;
use super::*;
use serde_json::json;
```

## /home/coding/AgentScribe/src/parser/sqlite.rs

### Use Statements (15)
```rust
use crate::error::{AgentScribeError, Result};
use crate::event::{Event, Role};
use crate::parser::{extract_field, extract_string, parse_timestamp, ParseContext, SessionInfo};
use crate::plugin::{Plugin, SessionDetection, SessionIdSource};
use chrono::Utc;
use regex::Regex;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::Path;
use super::*;
use crate::parser::FormatParser;
use crate::plugin::{;
use rusqlite::Connection;
use std::collections::HashMap;
use tempfile::NamedTempFile;
```

## /home/coding/AgentScribe/src/plugin.rs

### Use Statements (6)
```rust
use crate::error::{AgentScribeError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::warn;
use super::*;
```

## /home/coding/AgentScribe/src/projects.rs

### Use Statements (8)
```rust
use std::collections::HashSet;
use std::path::Path;
use std::{fmt, fs};
use serde::{Deserialize, Serialize};
use crate::error::{AgentScribeError, Result};
use super::*;
use std::fs;
use tempfile::tempdir;
```

## /home/coding/AgentScribe/src/pulse_report.rs

### Use Statements (15)
```rust
use crate::analytics::{self, AgentMetrics, AnalyticsOptions, AnalyticsOutput};
use crate::config::Config;
use crate::error::{AgentScribeError, Result};
use crate::recurring::{self, RecurringOptions};
use chrono::{DateTime, Datelike, TimeZone, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use crate::analytics::{estimate_cost, estimate_tokens, extract_session_data};
use crate::index::build_schema;
use crate::search::open_index;
use tantivy::collector::TopDocs;
use tantivy::query::AllQuery;
use super::*;
use crate::analytics::AgentMetrics;
```

## /home/coding/AgentScribe/src/recurring.rs

### Use Statements (12)
```rust
use crate::enrichment::outcome::{detect_outcome, OutcomeConfig};
use crate::error::Result;
use crate::event::{Event, SessionManifest};
use crate::scraper::Scraper;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use super::*;
use crate::event::Role;
use chrono::Duration;
use std::path::PathBuf;
```

## /home/coding/AgentScribe/src/redaction.rs

### Use Statements (4)
```rust
use crate::config::RedactionConfig;
use regex::Regex;
use std::sync::LazyLock;
use super::*;
```

## /home/coding/AgentScribe/src/reflect.rs

### Use Statements (9)
```rust
use crate::enrichment::behavioral_signals::{load_behavioral_signals, BehavioralSignals};
use crate::event::{Event, SessionManifest};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use crate::enrichment::behavioral_signals::compute_behavioral_signals;
use super::*;
use crate::event::Role;
```

## /home/coding/AgentScribe/src/render.rs

### Use Statements (4)
```rust
use crate::error::Result;
use crate::event::{Event, Role, SessionManifest};
use chrono::{DateTime, Utc};
use super::*;
```

## /home/coding/AgentScribe/src/rules.rs

### Use Statements (10)
```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use regex::Regex;
use crate::error::Result;
use crate::event::Role;
use crate::scraper::Scraper;
use std::sync::LazyLock;
use super::*;
use crate::event::Event;
use chrono::Utc;
```

## /home/coding/AgentScribe/src/scraper/companion.rs

### Use Statements (10)
```rust
use crate::error::{AgentScribeError, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::{Arc, RwLock};
use super::*;
use std::io::Write;
use tempfile::NamedTempFile;
```

## /home/coding/AgentScribe/src/scraper/file_path_extractor.rs

### Use Statements (10)
```rust
use crate::event::Event;
use crate::plugin::Plugin;
use std::sync::LazyLock;
use regex::Regex;
use crate::parser::extract_field;
use std::path::{Path, PathBuf};
use super::*;
use crate::event::{Event, Role};
use crate::plugin::{;
use chrono::Utc;
```

## /home/coding/AgentScribe/src/scraper/mod.rs

### Use Statements (17)
```rust
use crate::error::{AgentScribeError, Result};
use crate::event::Event;
use crate::index::{build_content, build_manifest_from_events, IndexManager};
use crate::parser::{;
use crate::plugin::{LogFormat, ModelDetection, Plugin, PluginManager, ProjectDetection};
use chrono::Utc;
use glob::glob;
use serde_json::Value;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tracing::{debug, info, warn};
use super::*;
use crate::plugin::{;
use crate::plugin::{Parser, PluginMeta, SessionDetection, SessionIdSource, Source};
```

## /home/coding/AgentScribe/src/scraper/state.rs

### Use Statements (12)
```rust
use crate::error::Result;
use crate::event::{ScrapeState, SourceFileState};
use chrono::Utc;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use super::*;
use std::sync::Arc;
use tempfile::NamedTempFile;
```

## /home/coding/AgentScribe/src/search.rs

### Use Statements (21)
```rust
use crate::error::{AgentScribeError, Result};
use crate::index::{build_schema, IndexFields};
use crate::scraper::load_session_content;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::ops::Bound;
use std::path::Path;
use tantivy::collector::TopDocs;
use tantivy::query::{;
use tantivy::schema::{Field, Value};
use tantivy::{DateTime as TantivyDateTime, DocAddress, Searcher, TantivyDocument};
use crate::config::Config;
use crate::vector::VectorIndex;
use crate::embedding::create_client;
use tantivy::query::AllQuery;
use super::*;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use tempfile::TempDir;
use crate::index::{build_code_artifact_document, build_session_document};
use crate::index::build_code_artifact_document;
```

## /home/coding/AgentScribe/src/shell_hook.rs

### Use Statements (3)
```rust
use crate::config::ShellHookConfig;
use crate::error::{AgentScribeError, Result};
use super::*;
```

## /home/coding/AgentScribe/src/tags.rs

### Use Statements (7)
```rust
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use regex::Regex;
use crate::event::{Event, Role};
use super::*;
use crate::event::Event;
use chrono::Utc;
```

## /home/coding/AgentScribe/src/transcription.rs

### Use Statements (15)
```rust
use crate::config::{RedactionConfig, WhisperConfig};
use crate::error::{AgentScribeError, Result};
use crate::redaction::RedactionScanner;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};
use serde::Deserialize;
use super::*;
```

## /home/coding/AgentScribe/src/vector.rs

### Use Statements (8)
```rust
use crate::config::VectorConfig;
use crate::error::{AgentScribeError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use super::*;
use tempfile::TempDir;
```

## /home/coding/AgentScribe/src/write_guard.rs

### Use Statements (3)
```rust
use std::process::Output;
use anyhow::Result;
use super::*;
```

## /home/coding/AgentScribe/test_timestamps.rs

### Use Statements (2)
```rust
use chrono::{DateTime, Utc};
use chrono::NaiveDateTime;
```

## /home/coding/AgentScribe/tests/aider_glob_discovery_test.rs

### Use Statements (2)
```rust
use std::fs;
use tempfile::TempDir;
```

## /home/coding/AgentScribe/tests/aider_input_scrape_test.rs

### Use Statements (4)
```rust
use agentscribe::event::Role;
use agentscribe::parser::{FormatParser, MarkdownParser};
use agentscribe::plugin::{LogFormat, Parser, Plugin, PluginMeta, SessionDetection, Source};
use std::path::PathBuf;
```

## /home/coding/AgentScribe/tests/aider_toml_glob_validation_test.rs

### Use Statements (2)
```rust
use std::path::PathBuf;
use tempfile::TempDir;
```

## /home/coding/AgentScribe/tests/context_tests.rs

### Use Statements (6)
```rust
use std::fs;
use std::path::PathBuf;
use agentscribe::event::{Event, Role, SessionManifest};
use agentscribe::index::IndexManager;
use agentscribe::search::{context_pack, extract_file_paths, ContextPack};
use chrono::Utc;
```

## /home/coding/AgentScribe/tests/daemon_mcp.rs

### Use Statements (10)
```rust
use agentscribe::daemon;
use agentscribe::mcp;
use agentscribe::scraper::Scraper;
use agentscribe::search::{execute_search, SearchOptions, SortOrder};
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;
```

## /home/coding/AgentScribe/tests/integration_tests.rs

### Use Statements (13)
```rust
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;
use agentscribe::enrichment::outcome::OutcomeConfig;
use agentscribe::enrichment::{detect_outcome, enrich_events, extract_solution, generate_summary};
use agentscribe::event::{Event, Role, SessionManifest};
use agentscribe::plugin::{;
use agentscribe::scraper::Scraper;
use agentscribe::search::{execute_search, SearchOptions, SortOrder};
use chrono::Utc;
use agentscribe::parser::{FormatParser, JsonTreeParser};
use std::collections::HashMap;
```

## /home/coding/AgentScribe/tests/main_session_parent_tests.rs

### Use Statements (3)
```rust
use agentscribe::event::{Event, Role};
use agentscribe::index::build_manifest_from_events;
use chrono::Utc;
```

## /home/coding/AgentScribe/tests/parent_session_tests.rs

### Use Statements (4)
```rust
use std::fs;
use agentscribe::index::build_manifest_from_events;
use agentscribe::plugin::{;
use agentscribe::scraper::Scraper;
```

## /home/coding/AgentScribe/tests/phase6_tests.rs

### Use Statements (15)
```rust
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use agentscribe::analytics::{self, AgentMetrics, AnalyticsOptions};
use agentscribe::config::Config;
use agentscribe::digest::{self, DigestOptions};
use agentscribe::enrichment::antipatterns;
use agentscribe::event::{Event, Role, SessionManifest};
use agentscribe::gc;
use agentscribe::index::IndexManager;
use agentscribe::recurring::{self, RecurringOptions};
use agentscribe::rules::{self, OutputFormat, Rule};
use agentscribe::search::{execute_search, SearchOptions};
use agentscribe::shell_hook;
use chrono::{Duration, Utc};
```

## /home/coding/AgentScribe/tests/pulse_report_tests.rs

### Use Statements (7)
```rust
use std::fs;
use agentscribe::config::Config;
use agentscribe::pulse_report::{;
use agentscribe::scraper::Scraper;
use chrono::{Datelike, Timelike, Utc};
use agentscribe::index::build_schema;
use tantivy::Index;
```

## /home/coding/AgentScribe/tests/render_tests.rs

### Use Statements (5)
```rust
use std::fs;
use tempfile::TempDir;
use agentscribe::event::{Event, Role, SessionManifest};
use agentscribe::render;
use chrono::Utc;
```

## /home/coding/AgentScribe/tests/subagent_integration_test.rs

### Use Statements (1)
```rust
use agentscribe::scraper::Scraper;
```

## /home/coding/AgentScribe/tests/subagent_parent_session_unit_tests.rs

### Use Statements (3)
```rust
use agentscribe::event::{Event, Role};
use agentscribe::index::build_manifest_from_events;
use chrono::Utc;
```

## /home/coding/AgentScribe/tests/subagent_spawning_integration_tests.rs

### Use Statements (10)
```rust
use std::fs;
use agentscribe::index::IndexManager;
use agentscribe::plugin::{;
use agentscribe::scraper::Scraper;
use agentscribe::search;
use agentscribe::search::SearchOptions;
use tantivy::{Searcher, TantivyDocument};
use tantivy::schema::Value;
use tantivy::query::TermQuery;
use tantivy::schema::Term;
```

## /home/coding/AgentScribe/tests/test_helpers.rs

### Use Statements (6)
```rust
use agentscribe::plugin::{;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use agentscribe::parser::{JsonlParser, ParseContext};
use super::*;
```

## /home/coding/AgentScribe/tests/transcription_tests.rs

### Use Statements (4)
```rust
use std::path::PathBuf;
use agentscribe::config::{RedactionConfig, WhisperConfig};
use agentscribe::redaction::RedactionScanner;
use agentscribe::transcription::{;
```

## /home/coding/AgentScribe/tests/zero_write_tests.rs

### Use Statements (4)
```rust
use std::fs;
use std::path::PathBuf;
use agentscribe::write_guard;
use walkdir::WalkDir;
```

## Summary Statistics
- Total files with imports: 65
- Total use statements: 570
- Total extern crate statements: 0

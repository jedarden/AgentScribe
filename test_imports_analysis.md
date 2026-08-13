# Test Imports Analysis - AgentScribe
Generated: 2026-08-13 04:45:54 UTC

**Total imports found:** 739

**Files analyzed:** 66

## Summary by Framework

| Framework | Import Count |
|-----------|-------------|
| other | 613 |
| resource-testing | 64 |
| internal | 57 |
| async-runtime | 5 |

## Imports by Framework

### other (613 imports)

**Unique imports:** 143

```rust
use super::*; // used in 49 tests
use crate::event::{Event, Role, SessionManifest}; // used in 44 tests
use crate::index::build_session_document; // used in 34 tests
use std::collections::HashMap; // used in 25 tests
use std::fs; // used in 24 tests
use std::path::Path; // used in 24 tests
use chrono::Utc; // used in 24 tests
use crate::error::{AgentScribeError, Result}; // used in 24 tests
use serde::{Deserialize, Serialize}; // used in 17 tests
use crate::parser::jsonl::JsonlParser; // used in 16 tests
use chrono::{DateTime, Utc}; // used in 14 tests
use std::path::{Path, PathBuf}; // used in 13 tests
use crate::event::Event; // used in 11 tests
use std::path::PathBuf; // used in 10 tests
use regex::Regex; // used in 10 tests
use crate::config::Config; // used in 9 tests
use std::io::Write; // used in 8 tests
use std::sync::LazyLock; // used in 8 tests
use crate::event::{Event, Role}; // used in 8 tests
use crate::plugin::{; // used in 7 tests
```

### resource-testing (64 imports)

**Unique imports:** 3

```rust
use tempfile::TempDir; // used in 58 tests
use tempfile::NamedTempFile; // used in 5 tests
use tempfile::tempdir; // used in 1 test
```

### internal (57 imports)

**Unique imports:** 35

```rust
use agentscribe::scraper::Scraper; // used in 6 tests
use agentscribe::event::{Event, Role, SessionManifest}; // used in 6 tests
use agentscribe::plugin::{; // used in 4 tests
use agentscribe::index::build_manifest_from_events; // used in 3 tests
use agentscribe::index::IndexManager; // used in 3 tests
use agentscribe::render; // used in 3 tests
use agentscribe::search::{execute_search, SearchOptions, SortOrder}; // used in 2 tests
use agentscribe::event::{Event, Role}; // used in 2 tests
use agentscribe::config::Config; // used in 2 tests
use agentscribe::enrichment::outcome::OutcomeConfig; // used in 1 test
use agentscribe::enrichment::{detect_outcome, enrich_events, extract_solution, generate_summary}; // used in 1 test
use agentscribe::parser::{FormatParser, JsonTreeParser}; // used in 1 test
use agentscribe::config::{RedactionConfig, WhisperConfig}; // used in 1 test
use agentscribe::redaction::RedactionScanner; // used in 1 test
use agentscribe::transcription::{; // used in 1 test
use agentscribe::search; // used in 1 test
use agentscribe::search::SearchOptions; // used in 1 test
use agentscribe::parser::{JsonlParser, ParseContext}; // used in 1 test
use agentscribe::pulse_report::{; // used in 1 test
use agentscribe::index::build_schema; // used in 1 test
```

### async-runtime (5 imports)

**Unique imports:** 5

```rust
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader}; // used in 1 test
use tokio::net::UnixListener; // used in 1 test
use tokio::task; // used in 1 test
use tokio::sync::{mpsc, Mutex}; // used in 1 test
use tokio::time::sleep; // used in 1 test
```

## Imports by File

### extract_test_imports.rs

**Total imports:** 3

#### other (3 imports)

```rust
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
```

### src/analytics.rs

**Total imports:** 19

#### other (18 imports)

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
```

#### resource-testing (1 imports)

```rust
use tempfile::TempDir;
```

### src/annotations.rs

**Total imports:** 8

#### other (7 imports)

```rust
use crate::error::{AgentScribeError, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use super::*;
```

#### resource-testing (1 imports)

```rust
use tempfile::TempDir;
```

### src/capacity.rs

**Total imports:** 11

#### other (10 imports)

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
```

#### resource-testing (1 imports)

```rust
use tempfile::TempDir;
```

### src/config.rs

**Total imports:** 8

#### other (8 imports)

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

### src/daemon.rs

**Total imports:** 24

#### other (24 imports)

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

### src/digest.rs

**Total imports:** 14

#### other (14 imports)

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

### src/embedding.rs

**Total imports:** 5

#### other (5 imports)

```rust
use crate::config::VectorConfig;
use crate::error::{AgentScribeError, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use super::*;
```

### src/enrichment/antipatterns.rs

**Total imports:** 9

#### other (9 imports)

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

### src/enrichment/behavioral_signals.rs

**Total imports:** 7

#### other (7 imports)

```rust
use crate::event::{Event, Role};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use super::*;
use crate::event::Event;
use chrono::Utc;
use serde_json::json;
```

### src/enrichment/code_artifacts.rs

**Total imports:** 7

#### other (7 imports)

```rust
use std::collections::HashMap;
use std::sync::LazyLock;
use serde::{Deserialize, Serialize};
use crate::event::{Event, Role};
use super::*;
use crate::event::Event;
use chrono::Utc;
```

### src/enrichment/config_change_tracker.rs

**Total imports:** 12

#### other (11 imports)

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
use std::fs;
use std::io::Write;
```

#### resource-testing (1 imports)

```rust
use tempfile::TempDir;
```

### src/enrichment/errors.rs

**Total imports:** 6

#### other (6 imports)

```rust
use std::sync::LazyLock;
use regex::Regex;
use crate::event::Event;
use super::*;
use crate::event::Role;
use chrono::Utc;
```

### src/enrichment/git.rs

**Total imports:** 6

#### other (6 imports)

```rust
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use super::*;
```

### src/enrichment/outcome.rs

**Total imports:** 6

#### other (6 imports)

```rust
use crate::event::{Event, Role, SessionManifest};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use super::*;
use chrono::Utc;
```

### src/enrichment/solution.rs

**Total imports:** 4

#### other (4 imports)

```rust
use crate::event::{Event, Role};
use super::*;
use crate::event::Event;
use chrono::Utc;
```

### src/enrichment/summary.rs

**Total imports:** 3

#### other (3 imports)

```rust
use crate::event::{Event, Role, SessionManifest};
use super::*;
use chrono::Utc;
```

### src/event.rs

**Total imports:** 4

#### other (4 imports)

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use super::*;
```

### src/file_knowledge.rs

**Total imports:** 20

#### other (20 imports)

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
use crate::event::{Event, Role, SessionManifest};
use crate::index::IndexManager;
use crate::event::{Event, Role, SessionManifest};
use crate::index::IndexManager;
```

### src/gc.rs

**Total imports:** 9

#### other (9 imports)

```rust
use crate::error::{AgentScribeError, Result};
use crate::index::IndexManager;
use crate::scraper::Scraper;
use chrono::{Duration, Utc};
use serde::Serialize;
use std::fs;
use std::path::Path;
use super::*;
use std::fs;
```

### src/index.rs

**Total imports:** 31

#### other (22 imports)

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
use chrono::Duration;
use tantivy::collector::Count;
use tantivy::query::TermQuery;
use tantivy::schema::IndexRecordOption;
use tantivy::collector::TopDocs;
use tantivy::query::TermQuery;
use tantivy::schema::IndexRecordOption;
use chrono::Duration;
use tantivy::collector::Count;
use tantivy::query::TermQuery;
use tantivy::schema::IndexRecordOption;
```

#### resource-testing (9 imports)

```rust
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
```

### src/mcp.rs

**Total imports:** 12

#### async-runtime (3 imports)

```rust
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::task;
```

#### other (9 imports)

```rust
use crate::search::{execute_search, parse_datetime, SearchOptions, SortOrder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use crate::scraper::Scraper;
use super::*;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
```

### src/parser/aider_input.rs

**Total imports:** 10

#### other (9 imports)

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
```

#### resource-testing (1 imports)

```rust
use tempfile::NamedTempFile;
```

### src/parser/json_array.rs

**Total imports:** 12

#### other (12 imports)

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

### src/parser/json_tree.rs

**Total imports:** 10

#### other (10 imports)

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

### src/parser/jsonl/jsonl_subagent_test.rs

**Total imports:** 24

#### other (24 imports)

```rust
use crate::parser::FormatParser;
use crate::plugin::{;
use super::*;
use crate::parser::jsonl::JsonlParser;
use crate::parser::jsonl::JsonlParser;
use crate::parser::jsonl::JsonlParser;
use crate::parser::jsonl::JsonlParser;
use crate::scraper::Scraper;
use crate::parser::jsonl::JsonlParser;
use crate::parser::jsonl::JsonlParser;
use crate::parser::jsonl::JsonlParser;
use crate::parser::jsonl::JsonlParser;
use crate::parser::FormatParser;
use crate::plugin::{;
use super::*;
use crate::parser::jsonl::JsonlParser;
use crate::parser::jsonl::JsonlParser;
use crate::parser::jsonl::JsonlParser;
use crate::parser::jsonl::JsonlParser;
use crate::scraper::Scraper;
use crate::parser::jsonl::JsonlParser;
use crate::parser::jsonl::JsonlParser;
use crate::parser::jsonl::JsonlParser;
use crate::parser::jsonl::JsonlParser;
```

### src/parser/jsonl.rs

**Total imports:** 13

#### other (13 imports)

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

### src/parser/markdown.rs

**Total imports:** 18

#### other (16 imports)

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
use std::fs;
use std::path::PathBuf;
```

#### resource-testing (2 imports)

```rust
use tempfile::NamedTempFile;
use tempfile::TempDir;
```

### src/parser/mod.rs

**Total imports:** 8

#### other (8 imports)

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

### src/parser/sqlite.rs

**Total imports:** 15

#### other (14 imports)

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
```

#### resource-testing (1 imports)

```rust
use tempfile::NamedTempFile;
```

### src/plugin.rs

**Total imports:** 6

#### other (6 imports)

```rust
use crate::error::{AgentScribeError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::warn;
use super::*;
```

### src/projects.rs

**Total imports:** 8

#### other (7 imports)

```rust
use std::collections::HashSet;
use std::path::Path;
use std::{fmt, fs};
use serde::{Deserialize, Serialize};
use crate::error::{AgentScribeError, Result};
use super::*;
use std::fs;
```

#### resource-testing (1 imports)

```rust
use tempfile::tempdir;
```

### src/pulse_report.rs

**Total imports:** 17

#### other (17 imports)

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
use std::collections::HashMap;
use crate::analytics::AgentMetrics;
use std::collections::HashMap;
```

### src/recurring.rs

**Total imports:** 12

#### other (12 imports)

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

### src/redaction.rs

**Total imports:** 4

#### other (4 imports)

```rust
use crate::config::RedactionConfig;
use regex::Regex;
use std::sync::LazyLock;
use super::*;
```

### src/reflect.rs

**Total imports:** 9

#### other (9 imports)

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

### src/render.rs

**Total imports:** 4

#### other (4 imports)

```rust
use crate::error::Result;
use crate::event::{Event, Role, SessionManifest};
use chrono::{DateTime, Utc};
use super::*;
```

### src/rules.rs

**Total imports:** 10

#### other (10 imports)

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

### src/scraper/companion.rs

**Total imports:** 10

#### other (9 imports)

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
```

#### resource-testing (1 imports)

```rust
use tempfile::NamedTempFile;
```

### src/scraper/file_path_extractor.rs

**Total imports:** 10

#### other (10 imports)

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

### src/scraper/mod.rs

**Total imports:** 18

#### other (18 imports)

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
use crate::plugin::{Parser, PluginMeta, SessionDetection, SessionIdSource, Source};
```

### src/scraper/state.rs

**Total imports:** 12

#### other (11 imports)

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
```

#### resource-testing (1 imports)

```rust
use tempfile::NamedTempFile;
```

### src/search.rs

**Total imports:** 135

#### other (95 imports)

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
use crate::config::Config;
use crate::embedding::create_client;
use crate::vector::VectorIndex;
use crate::config::Config;
use crate::embedding::create_client;
use crate::vector::VectorIndex;
use tantivy::collector::TopDocs;
use tantivy::query::AllQuery;
use crate::config::Config;
use crate::vector::VectorIndex;
use super::*;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::{build_code_artifact_document, build_session_document};
use crate::index::build_code_artifact_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::{build_code_artifact_document, build_session_document};
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
use crate::event::{Event, Role, SessionManifest};
use crate::index::build_session_document;
```

#### resource-testing (40 imports)

```rust
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
use tempfile::TempDir;
```

### src/shell_hook.rs

**Total imports:** 4

#### other (4 imports)

```rust
use crate::config::ShellHookConfig;
use crate::error::{AgentScribeError, Result};
use super::*;
use crate::config::ShellHookConfig;
```

### src/tags.rs

**Total imports:** 7

#### other (7 imports)

```rust
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use regex::Regex;
use crate::event::{Event, Role};
use super::*;
use crate::event::Event;
use chrono::Utc;
```

### src/transcription.rs

**Total imports:** 16

#### async-runtime (2 imports)

```rust
use tokio::sync::{mpsc, Mutex};
use tokio::time::sleep;
```

#### other (14 imports)

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
use tracing::{debug, error, info, warn};
use serde::Deserialize;
use serde::Deserialize;
use super::*;
```

### src/vector.rs

**Total imports:** 8

#### other (7 imports)

```rust
use crate::config::VectorConfig;
use crate::error::{AgentScribeError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use super::*;
```

#### resource-testing (1 imports)

```rust
use tempfile::TempDir;
```

### src/write_guard.rs

**Total imports:** 3

#### other (3 imports)

```rust
use std::process::Output;
use anyhow::Result;
use super::*;
```

### test_timestamps.rs

**Total imports:** 2

#### other (2 imports)

```rust
use chrono::{DateTime, Utc};
use chrono::NaiveDateTime;
```

### tests/aider_glob_discovery_test.rs

**Total imports:** 2

#### other (1 imports)

```rust
use std::fs;
```

#### resource-testing (1 imports)

```rust
use tempfile::TempDir;
```

### tests/aider_input_scrape_test.rs

**Total imports:** 4

#### internal (3 imports)

```rust
use agentscribe::event::Role;
use agentscribe::parser::{FormatParser, MarkdownParser};
use agentscribe::plugin::{LogFormat, Parser, Plugin, PluginMeta, SessionDetection, Source};
```

#### other (1 imports)

```rust
use std::path::PathBuf;
```

### tests/aider_toml_glob_validation_test.rs

**Total imports:** 2

#### other (1 imports)

```rust
use std::path::PathBuf;
```

#### resource-testing (1 imports)

```rust
use tempfile::TempDir;
```

### tests/context_tests.rs

**Total imports:** 6

#### internal (3 imports)

```rust
use agentscribe::event::{Event, Role, SessionManifest};
use agentscribe::index::IndexManager;
use agentscribe::search::{context_pack, extract_file_paths, ContextPack};
```

#### other (3 imports)

```rust
use std::fs;
use std::path::PathBuf;
use chrono::Utc;
```

### tests/daemon_mcp.rs

**Total imports:** 10

#### internal (4 imports)

```rust
use agentscribe::daemon;
use agentscribe::mcp;
use agentscribe::scraper::Scraper;
use agentscribe::search::{execute_search, SearchOptions, SortOrder};
```

#### other (6 imports)

```rust
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;
```

### tests/integration_tests.rs

**Total imports:** 13

#### internal (7 imports)

```rust
use agentscribe::enrichment::outcome::OutcomeConfig;
use agentscribe::enrichment::{detect_outcome, enrich_events, extract_solution, generate_summary};
use agentscribe::event::{Event, Role, SessionManifest};
use agentscribe::plugin::{;
use agentscribe::scraper::Scraper;
use agentscribe::search::{execute_search, SearchOptions, SortOrder};
use agentscribe::parser::{FormatParser, JsonTreeParser};
```

#### other (6 imports)

```rust
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;
use chrono::Utc;
use std::collections::HashMap;
```

### tests/main_session_parent_tests.rs

**Total imports:** 3

#### internal (2 imports)

```rust
use agentscribe::event::{Event, Role};
use agentscribe::index::build_manifest_from_events;
```

#### other (1 imports)

```rust
use chrono::Utc;
```

### tests/parent_session_tests.rs

**Total imports:** 4

#### internal (3 imports)

```rust
use agentscribe::index::build_manifest_from_events;
use agentscribe::plugin::{;
use agentscribe::scraper::Scraper;
```

#### other (1 imports)

```rust
use std::fs;
```

### tests/phase6_tests.rs

**Total imports:** 15

#### internal (11 imports)

```rust
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
```

#### other (4 imports)

```rust
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use chrono::{Duration, Utc};
```

### tests/pulse_report_tests.rs

**Total imports:** 7

#### internal (4 imports)

```rust
use agentscribe::config::Config;
use agentscribe::pulse_report::{;
use agentscribe::scraper::Scraper;
use agentscribe::index::build_schema;
```

#### other (3 imports)

```rust
use std::fs;
use chrono::{Datelike, Timelike, Utc};
use tantivy::Index;
```

### tests/render_tests.rs

**Total imports:** 11

#### internal (6 imports)

```rust
use agentscribe::event::{Event, Role, SessionManifest};
use agentscribe::render;
use agentscribe::event::{Event, Role, SessionManifest};
use agentscribe::render;
use agentscribe::event::{Event, Role, SessionManifest};
use agentscribe::render;
```

#### other (4 imports)

```rust
use std::fs;
use chrono::Utc;
use chrono::Utc;
use chrono::Utc;
```

#### resource-testing (1 imports)

```rust
use tempfile::TempDir;
```

### tests/subagent_integration_test.rs

**Total imports:** 1

#### internal (1 imports)

```rust
use agentscribe::scraper::Scraper;
```

### tests/subagent_parent_session_unit_tests.rs

**Total imports:** 3

#### internal (2 imports)

```rust
use agentscribe::event::{Event, Role};
use agentscribe::index::build_manifest_from_events;
```

#### other (1 imports)

```rust
use chrono::Utc;
```

### tests/subagent_spawning_integration_tests.rs

**Total imports:** 11

#### internal (5 imports)

```rust
use agentscribe::index::IndexManager;
use agentscribe::plugin::{;
use agentscribe::scraper::Scraper;
use agentscribe::search;
use agentscribe::search::SearchOptions;
```

#### other (6 imports)

```rust
use std::fs;
use tantivy::{Searcher, TantivyDocument};
use tantivy::schema::Value;
use tantivy::query::TermQuery;
use tantivy::schema::Term;
use tantivy::schema::Value;
```

### tests/test_helpers.rs

**Total imports:** 6

#### internal (2 imports)

```rust
use agentscribe::plugin::{;
use agentscribe::parser::{JsonlParser, ParseContext};
```

#### other (4 imports)

```rust
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use super::*;
```

### tests/transcription_tests.rs

**Total imports:** 4

#### internal (3 imports)

```rust
use agentscribe::config::{RedactionConfig, WhisperConfig};
use agentscribe::redaction::RedactionScanner;
use agentscribe::transcription::{;
```

#### other (1 imports)

```rust
use std::path::PathBuf;
```

### tests/zero_write_tests.rs

**Total imports:** 4

#### internal (1 imports)

```rust
use agentscribe::write_guard;
```

#### other (3 imports)

```rust
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;
```


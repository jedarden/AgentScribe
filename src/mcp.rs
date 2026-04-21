//! Optional MCP server over Unix domain socket.
//!
//! When `[daemon] mcp_enabled = true` in config.toml, the daemon binds a Unix
//! socket at `mcp_socket_path` (default: `~/.agentscribe/mcp.sock`) and speaks
//! the Model Context Protocol (JSON-RPC 2.0, newline-delimited) over it.
//!
//! Four tools are exposed:
//!   - `agentscribe_search`  — full-text / faceted search (mirrors the CLI)
//!   - `agentscribe_status`  — plugin list, session counts, daemon state, index stats
//!   - `agentscribe_blame`   — file path (+ optional line) → sessions that touched it
//!   - `agentscribe_file`    — chronological session list for a file path

use crate::search::{execute_search, parse_datetime, SearchOptions, SortOrder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::task;

// ── JSON-RPC helpers ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Request {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

fn ok_response(id: Value, result: Value) -> String {
    format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        })
    )
}

fn err_response(id: Value, code: i32, message: &str) -> String {
    format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message
            }
        })
    )
}

// ── Tool definitions ────────────────────────────────────────────────────

fn tool_definitions() -> Value {
    json!({
        "tools": [
            {
                "name": "agentscribe_search",
                "description": "Search AgentScribe session index with full-text and faceted filters. Returns matching sessions with snippets.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Full-text search query (Tantivy syntax)"
                        },
                        "error": {
                            "type": "string",
                            "description": "Error fingerprint pattern to search"
                        },
                        "code": {
                            "type": "string",
                            "description": "Code content query"
                        },
                        "lang": {
                            "type": "string",
                            "description": "Language filter for code search"
                        },
                        "solution_only": {
                            "type": "boolean",
                            "description": "Return only extracted solutions",
                            "default": false
                        },
                        "like": {
                            "type": "string",
                            "description": "Find sessions similar to this session ID"
                        },
                        "session": {
                            "type": "string",
                            "description": "Retrieve a specific session by ID"
                        },
                        "agent": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Filter by source agent type (OR logic)"
                        },
                        "project": {
                            "type": "string",
                            "description": "Filter by project path"
                        },
                        "since": {
                            "type": "string",
                            "description": "Only sessions after this time (ISO 8601 or relative, e.g. 30d)"
                        },
                        "before": {
                            "type": "string",
                            "description": "Only sessions before this time (ISO 8601 or relative)"
                        },
                        "tag": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Filter by tag (AND logic)"
                        },
                        "outcome": {
                            "type": "string",
                            "enum": ["success", "failure", "abandoned", "unknown"],
                            "description": "Filter by session outcome"
                        },
                        "model": {
                            "type": "string",
                            "description": "Filter by model name"
                        },
                        "fuzzy": {
                            "type": "boolean",
                            "description": "Enable fuzzy matching on all query terms",
                            "default": false
                        },
                        "max_results": {
                            "type": "integer",
                            "description": "Maximum number of results",
                            "default": 10
                        },
                        "snippet_length": {
                            "type": "integer",
                            "description": "Snippet length per result in characters",
                            "default": 200
                        },
                        "token_budget": {
                            "type": "integer",
                            "description": "Token budget for greedy knapsack context packing"
                        },
                        "offset": {
                            "type": "integer",
                            "description": "Skip first N results for pagination",
                            "default": 0
                        },
                        "sort": {
                            "type": "string",
                            "enum": ["relevance", "newest", "oldest", "turns"],
                            "description": "Sort order for results",
                            "default": "relevance"
                        },
                        "file_path": {
                            "type": "string",
                            "description": "Filter to sessions that touched this file path"
                        }
                    }
                }
            },
            {
                "name": "agentscribe_status",
                "description": "Show AgentScribe status: plugin list with session counts, daemon state, and index stats.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "plugin": {
                            "type": "string",
                            "description": "Show status for a specific plugin only (optional)"
                        }
                    }
                }
            },
            {
                "name": "agentscribe_blame",
                "description": "Find sessions that touched a specific file path, sorted by newest first. Optionally provide a line number as context.",
                "inputSchema": {
                    "type": "object",
                    "required": ["file"],
                    "properties": {
                        "file": {
                            "type": "string",
                            "description": "Absolute or relative file path to look up"
                        },
                        "line": {
                            "type": "integer",
                            "description": "Optional line number (informational; returned in response for context)"
                        }
                    }
                }
            },
            {
                "name": "agentscribe_file",
                "description": "List all sessions that touched a file in chronological order (oldest first).",
                "inputSchema": {
                    "type": "object",
                    "required": ["file"],
                    "properties": {
                        "file": {
                            "type": "string",
                            "description": "File path"
                        }
                    }
                }
            }
        ]
    })
}

// ── Tool handlers ───────────────────────────────────────────────────────

async fn handle_search(data_dir: Arc<PathBuf>, args: Value) -> Value {
    let result = task::spawn_blocking(move || {
        let query = args["query"].as_str().map(String::from);
        let error_pattern = args["error"].as_str().map(String::from);
        let code_query = args["code"].as_str().map(String::from);
        let code_lang = args["lang"].as_str().map(String::from);
        let solution_only = args["solution_only"].as_bool().unwrap_or(false);
        let like_session = args["like"].as_str().map(String::from);
        let session_id = args["session"].as_str().map(String::from);
        let project = args["project"].as_str().map(String::from);
        let outcome = args["outcome"].as_str().map(String::from);
        let model = args["model"].as_str().map(String::from);
        let file_path = args["file_path"].as_str().map(String::from);
        let fuzzy = args["fuzzy"].as_bool().unwrap_or(false);
        let max_results = args["max_results"].as_u64().unwrap_or(10) as usize;
        let snippet_length = args["snippet_length"].as_u64().unwrap_or(200) as usize;
        let token_budget = args["token_budget"].as_u64().map(|v| v as usize);
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;

        let sort = match args["sort"].as_str().unwrap_or("relevance") {
            "newest" => SortOrder::Newest,
            "oldest" => SortOrder::Oldest,
            "turns" => SortOrder::Turns,
            _ => SortOrder::Relevance,
        };

        let agent: Vec<String> = args["agent"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();

        let tag: Vec<String> = args["tag"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();

        let since = args["since"].as_str().and_then(|s| parse_datetime(s).ok());
        let before = args["before"].as_str().and_then(|s| parse_datetime(s).ok());

        let opts = SearchOptions {
            query,
            error_pattern,
            code_query,
            code_lang,
            solution_only,
            like_session,
            session_id,
            agent,
            project,
            since,
            before,
            tag,
            outcome,
            doc_type_filter: None,
            model,
            fuzzy,
            fuzzy_distance: 1,
            max_results,
            snippet_length,
            token_budget,
            offset,
            sort,
            file_path,
        };

        execute_search(&data_dir, &opts)
    })
    .await;

    match result {
        Ok(Ok(output)) => {
            serde_json::to_value(&output).unwrap_or_else(|e| json!({"error": e.to_string()}))
        }
        Ok(Err(e)) => json!({"error": e.to_string()}),
        Err(e) => json!({"error": format!("task panicked: {}", e)}),
    }
}

async fn handle_status(data_dir: Arc<PathBuf>, args: Value) -> Value {
    let plugin_filter = args["plugin"].as_str().map(String::from);

    let result = task::spawn_blocking(move || {
        use crate::scraper::Scraper;

        let daemon_info = crate::daemon::status(&data_dir).ok();

        let mut scraper = match Scraper::new(data_dir.to_path_buf()) {
            Ok(s) => s,
            Err(e) => return json!({"error": format!("scraper init failed: {}", e)}),
        };
        if let Err(e) = scraper.load_plugins() {
            tracing::warn!("MCP status: failed to load plugins: {}", e);
        }

        let plugin_names: Vec<String> = if let Some(ref name) = plugin_filter {
            vec![name.clone()]
        } else {
            scraper
                .plugin_manager()
                .names()
                .into_iter()
                .map(String::from)
                .collect()
        };

        let scrape_state = scraper.state_manager().get_all();

        let plugins: Vec<Value> = plugin_names
            .iter()
            .map(|plugin_name| {
                let session_count = scraper.list_sessions(plugin_name).unwrap_or_default().len();
                let source_paths = scraper
                    .plugin_manager()
                    .get(plugin_name)
                    .map(|p| p.source.paths.clone())
                    .unwrap_or_default();
                let plugin_files: Vec<_> = scrape_state
                    .sources
                    .iter()
                    .filter(|(_, s)| s.plugin == *plugin_name)
                    .collect();
                let last_scraped = plugin_files.iter().map(|(_, s)| s.last_scraped).max();
                json!({
                    "name":         plugin_name,
                    "sessions":     session_count,
                    "source_files": plugin_files.len(),
                    "source_paths": source_paths,
                    "last_scraped": last_scraped.map(|t| t.to_rfc3339()),
                })
            })
            .collect();

        let index_dir = data_dir.join("index");
        let index_size = if index_dir.exists() {
            dir_size_sync(&index_dir)
        } else {
            0
        };

        let mut out = json!({
            "version":  env!("CARGO_PKG_VERSION"),
            "data_dir": data_dir.display().to_string(),
            "plugins":  plugins,
            "index": {
                "exists":     index_dir.exists(),
                "size_bytes": index_size,
            },
        });

        if let Some(info) = daemon_info {
            out["daemon"] = json!({
                "running":          info.running,
                "pid":              info.pid,
                "sessions_indexed": info.sessions_indexed,
                "last_scrape":      info.last_scrape.map(|t| t.to_rfc3339()),
                "started_at":       info.started_at.map(|t| t.to_rfc3339()),
                "uptime_secs":      info.uptime_secs,
            });
        }

        out
    })
    .await;

    match result {
        Ok(v) => v,
        Err(e) => json!({"error": format!("task panicked: {}", e)}),
    }
}

async fn handle_blame(data_dir: Arc<PathBuf>, args: Value) -> Value {
    let file = match args["file"].as_str() {
        Some(f) => f.to_string(),
        None => return json!({"error": "missing required parameter: file"}),
    };
    let line = args["line"].as_u64();
    let file_clone = file.clone();

    let result = task::spawn_blocking(move || {
        let opts = file_search_opts(file_clone, SortOrder::Newest, 20, 200);
        execute_search(&data_dir, &opts)
    })
    .await;

    match result {
        Ok(Ok(output)) => json!({
            "file":          file,
            "line":          line,
            "sessions":      output.results,
            "total_matches": output.total_matches,
        }),
        Ok(Err(e)) => json!({"error": e.to_string()}),
        Err(e) => json!({"error": format!("task panicked: {}", e)}),
    }
}

async fn handle_file(data_dir: Arc<PathBuf>, args: Value) -> Value {
    let file = match args["file"].as_str() {
        Some(f) => f.to_string(),
        None => return json!({"error": "missing required parameter: file"}),
    };
    let file_clone = file.clone();

    let result = task::spawn_blocking(move || {
        let opts = file_search_opts(file_clone, SortOrder::Oldest, 50, 100);
        execute_search(&data_dir, &opts)
    })
    .await;

    match result {
        Ok(Ok(output)) => json!({
            "file":          file,
            "sessions":      output.results,
            "total_matches": output.total_matches,
        }),
        Ok(Err(e)) => json!({"error": e.to_string()}),
        Err(e) => json!({"error": format!("task panicked: {}", e)}),
    }
}

/// Build a SearchOptions that filters by file path only (used by blame and file tools).
fn file_search_opts(
    file_path: String,
    sort: SortOrder,
    max_results: usize,
    snippet_length: usize,
) -> SearchOptions {
    SearchOptions {
        query: None,
        error_pattern: None,
        code_query: None,
        code_lang: None,
        solution_only: false,
        like_session: None,
        session_id: None,
        agent: vec![],
        project: None,
        since: None,
        before: None,
        tag: vec![],
        outcome: None,
        doc_type_filter: None,
        model: None,
        fuzzy: false,
        fuzzy_distance: 1,
        max_results,
        snippet_length,
        token_budget: None,
        offset: 0,
        sort,
        file_path: Some(file_path),
    }
}

/// Recursively compute directory size in bytes (synchronous helper).
fn dir_size_sync(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    if path.is_file() {
        return path.metadata().map(|m| m.len()).unwrap_or(0);
    }
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.is_dir() {
                total += dir_size_sync(&p);
            } else if let Ok(m) = p.metadata() {
                total += m.len();
            }
        }
    }
    total
}

// ── Per-connection handler ──────────────────────────────────────────────

async fn handle_connection(stream: tokio::net::UnixStream, data_dir: Arc<PathBuf>) {
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let _ = write_half
                    .write_all(
                        err_response(Value::Null, -32700, &format!("parse error: {}", e))
                            .as_bytes(),
                    )
                    .await;
                continue;
            }
        };

        // Notifications have no `id` — no response required.
        let id = match req.id {
            Some(ref v) => v.clone(),
            None => continue,
        };

        let empty_obj = Value::Object(Default::default());
        let params = req.params.unwrap_or(empty_obj);
        let data_dir = Arc::clone(&data_dir);

        let response = match req.method.as_str() {
            "initialize" => ok_response(
                id,
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": {
                        "name":    "agentscribe",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            ),

            "tools/list" => ok_response(id, tool_definitions()),

            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("").to_string();
                let args = {
                    let raw = &params["arguments"];
                    if raw.is_null() {
                        json!({})
                    } else {
                        raw.clone()
                    }
                };

                let (result_val, is_error) = match name.as_str() {
                    "agentscribe_search" => {
                        let v = handle_search(data_dir, args).await;
                        let err = v.get("error").is_some();
                        (v, err)
                    }
                    "agentscribe_status" => {
                        let v = handle_status(data_dir, args).await;
                        let err = v.get("error").is_some();
                        (v, err)
                    }
                    "agentscribe_blame" => {
                        let v = handle_blame(data_dir, args).await;
                        let err = v.get("error").is_some();
                        (v, err)
                    }
                    "agentscribe_file" => {
                        let v = handle_file(data_dir, args).await;
                        let err = v.get("error").is_some();
                        (v, err)
                    }
                    _ => {
                        let msg = format!("unknown tool: {}", name);
                        (json!({ "error": msg }), true)
                    }
                };

                let text = if is_error {
                    result_val["error"]
                        .as_str()
                        .unwrap_or("unknown error")
                        .to_string()
                } else {
                    serde_json::to_string_pretty(&result_val).unwrap_or_default()
                };

                ok_response(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": text }],
                        "isError": is_error
                    }),
                )
            }

            _ => err_response(id, -32601, &format!("method not found: {}", req.method)),
        };

        if let Err(e) = write_half.write_all(response.as_bytes()).await {
            tracing::warn!("MCP: write failed: {}", e);
            break;
        }
    }
}

// ── Public API ──────────────────────────────────────────────────────────

/// Run the MCP Unix socket server until `shutdown_rx` fires or is dropped.
///
/// Removes any stale socket file before binding.  On exit, removes the socket
/// file so clients get a clean `ENOENT` instead of a dangling path.
///
/// Each accepted client connection is handled in its own Tokio task, so
/// concurrent requests are handled independently.
pub async fn run_mcp_server(
    data_dir: PathBuf,
    socket_path: PathBuf,
    mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) {
    // Remove any leftover socket from a previous run.
    let _ = std::fs::remove_file(&socket_path);

    let listener = match UnixListener::bind(&socket_path) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(
                path = %socket_path.display(),
                error = %e,
                "MCP: failed to bind Unix socket"
            );
            return;
        }
    };

    tracing::info!(path = %socket_path.display(), "MCP server listening");

    let data_dir = Arc::new(data_dir);

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _)) => {
                        let data_dir = Arc::clone(&data_dir);
                        tokio::spawn(handle_connection(stream, data_dir));
                    }
                    Err(e) => {
                        tracing::warn!("MCP: accept error: {}", e);
                        break;
                    }
                }
            }
            _ = &mut shutdown_rx => {
                tracing::info!("MCP: shutdown signal received");
                break;
            }
        }
    }

    drop(listener);
    let _ = std::fs::remove_file(&socket_path);
    tracing::info!("MCP server stopped, socket removed");
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    /// Send a JSON-RPC request and read the response line.
    fn rpc_call(stream: &mut UnixStream, method: &str, params: Value) -> Value {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });
        let mut line = serde_json::to_string(&request).unwrap();
        line.push('\n');
        stream.write_all(line.as_bytes()).unwrap();

        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut response_line = String::new();
        reader.read_line(&mut response_line).unwrap();
        serde_json::from_str(response_line.trim()).unwrap()
    }

    /// Spin up the MCP server on a temp socket, run a test, then shut down.
    fn with_mcp_server<F>(test_fn: F)
    where
        F: FnOnce(&UnixStream),
    {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_path_buf();
        let socket_path = dir.path().join("mcp.sock");

        // Create minimal data directory structure
        std::fs::create_dir_all(data_dir.join("plugins")).unwrap();
        std::fs::create_dir_all(data_dir.join("sessions")).unwrap();
        std::fs::create_dir_all(data_dir.join("index")).unwrap();
        std::fs::create_dir_all(data_dir.join("state")).unwrap();

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let socket_path_clone = socket_path.clone();
        let data_dir_clone = data_dir.clone();

        rt.spawn(async move {
            run_mcp_server(data_dir_clone, socket_path_clone, shutdown_rx).await;
        });

        // Give the server a moment to bind
        std::thread::sleep(std::time::Duration::from_millis(100));

        let stream = UnixStream::connect(&socket_path).unwrap();
        test_fn(&stream);
        drop(stream);

        // Shutdown the server
        let _ = shutdown_tx.send(());
        rt.shutdown_background();
    }

    #[test]
    fn test_mcp_initialize() {
        with_mcp_server(|stream| {
            let resp = rpc_call(&mut stream.try_clone().unwrap(), "initialize", json!({}));
            assert_eq!(resp["jsonrpc"], "2.0");
            assert_eq!(resp["result"]["serverInfo"]["name"], "agentscribe");
            assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
        });
    }

    #[test]
    fn test_mcp_tools_list() {
        with_mcp_server(|stream| {
            let resp = rpc_call(&mut stream.try_clone().unwrap(), "tools/list", json!({}));
            let tools = resp["result"]["tools"].as_array().unwrap();
            let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
            assert!(names.contains(&"agentscribe_search"));
            assert!(names.contains(&"agentscribe_status"));
            assert!(names.contains(&"agentscribe_blame"));
            assert!(names.contains(&"agentscribe_file"));
        });
    }

    #[test]
    fn test_mcp_status_tool() {
        with_mcp_server(|stream| {
            let resp = rpc_call(
                &mut stream.try_clone().unwrap(),
                "tools/call",
                json!({
                    "name": "agentscribe_status",
                    "arguments": {}
                }),
            );
            assert_eq!(resp["jsonrpc"], "2.0");
            let content = resp["result"]["content"].as_array().unwrap();
            assert!(!content.is_empty());
            assert_eq!(content[0]["type"], "text");
            // Should be valid JSON containing version
            let text = content[0]["text"].as_str().unwrap();
            let parsed: Value = serde_json::from_str(text).unwrap();
            assert!(parsed["version"].is_string());
        });
    }

    #[test]
    fn test_mcp_search_tool_no_index() {
        with_mcp_server(|stream| {
            let resp = rpc_call(
                &mut stream.try_clone().unwrap(),
                "tools/call",
                json!({
                    "name": "agentscribe_search",
                    "arguments": {
                        "query": "test"
                    }
                }),
            );
            // Search on empty index should return error (no index) but not crash
            let content = resp["result"]["content"].as_array().unwrap();
            assert!(!content.is_empty());
        });
    }

    #[test]
    fn test_mcp_blame_tool_missing_param() {
        with_mcp_server(|stream| {
            let resp = rpc_call(
                &mut stream.try_clone().unwrap(),
                "tools/call",
                json!({
                    "name": "agentscribe_blame",
                    "arguments": {}
                }),
            );
            let content = resp["result"]["content"].as_array().unwrap();
            assert_eq!(content[0]["type"], "text");
            // Should report missing required parameter
            let text = content[0]["text"].as_str().unwrap();
            assert!(text.contains("missing"));
        });
    }

    #[test]
    fn test_mcp_unknown_method() {
        with_mcp_server(|stream| {
            let resp = rpc_call(
                &mut stream.try_clone().unwrap(),
                "nonexistent/method",
                json!({}),
            );
            assert!(resp["error"].is_object());
            assert_eq!(resp["error"]["code"], -32601);
        });
    }

    #[test]
    fn test_mcp_unknown_tool() {
        with_mcp_server(|stream| {
            let resp = rpc_call(
                &mut stream.try_clone().unwrap(),
                "tools/call",
                json!({
                    "name": "agentscribe_nonexistent",
                    "arguments": {}
                }),
            );
            let content = resp["result"]["content"].as_array().unwrap();
            assert_eq!(content[0]["type"], "text");
            assert!(resp["result"]["isError"].as_bool().unwrap());
        });
    }

    #[test]
    fn test_mcp_socket_cleanup_on_shutdown() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("mcp.sock");
        let data_dir = dir.path().to_path_buf();
        std::fs::create_dir_all(data_dir.join("plugins")).unwrap();

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        rt.block_on(async {
            let sp = socket_path.clone();
            tokio::spawn(run_mcp_server(data_dir, sp, shutdown_rx));
            // Wait for server to start
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            assert!(
                socket_path.exists(),
                "socket should exist while server is running"
            );
            // Trigger shutdown
            shutdown_tx.send(()).unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            assert!(
                !socket_path.exists(),
                "socket should be removed after shutdown"
            );
        });
    }

    #[test]
    fn test_file_search_opts_builder() {
        let opts = file_search_opts("/src/main.rs".to_string(), SortOrder::Newest, 20, 200);
        assert_eq!(opts.file_path, Some("/src/main.rs".to_string()));
        assert_eq!(opts.max_results, 20);
        assert_eq!(opts.snippet_length, 200);
        assert!(opts.query.is_none());
    }

    #[test]
    fn test_jsonrpc_response_format() {
        let resp = ok_response(json!(1), json!({"status": "ok"}));
        let parsed: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["id"], 1);
        assert_eq!(parsed["result"]["status"], "ok");
        assert!(resp.ends_with('\n'));
    }

    #[test]
    fn test_jsonrpc_error_format() {
        let resp = err_response(json!(2), -32600, "invalid request");
        let parsed: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(parsed["error"]["code"], -32600);
        assert_eq!(parsed["error"]["message"], "invalid request");
    }

    #[test]
    fn test_dir_size_sync() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("b.txt"), "world").unwrap();
        let size = dir_size_sync(dir.path());
        assert_eq!(size, 5 + 5); // "hello" + "world"
    }
}

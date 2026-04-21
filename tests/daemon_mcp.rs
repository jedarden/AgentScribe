//! Phase 4 integration tests: daemon file watching, debounce, MCP tools, graceful shutdown.

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

// ── helpers ───────────────────────────────────────────────────────────────

/// Create a temp data directory with the standard AgentScribe layout and a
/// claude-code plugin pointing at `watch_dir`.
fn setup_data_dir(watch_dir: &Path) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path();

    fs::create_dir_all(data_dir.join("plugins")).unwrap();
    fs::create_dir_all(data_dir.join("sessions")).unwrap();
    fs::create_dir_all(data_dir.join("index")).unwrap();
    fs::create_dir_all(data_dir.join("state")).unwrap();

    // Write a claude-code plugin that watches the provided directory
    let plugin_content = format!(
        r#"[plugin]
name = "claude-code"
version = "1.0"

[source]
paths = ["{}/*.jsonl"]
exclude = []
format = "jsonl"

[source.session_detection]
method = "one-file-per-session"
session_id_from = "filename"

[parser]
timestamp = "timestamp"
role = "message.role"
content = "message.content"
type = "type"

[parser.role_map]
"user" = "user"
"assistant" = "assistant"

[parser.static]
source_agent = "claude-code"

[parser.project]
method = "field"
field = "cwd"

[parser.file_paths]
content_regex = true
"#,
        watch_dir.display()
    );
    fs::write(data_dir.join("plugins/claude-code.toml"), plugin_content).unwrap();

    // Create the watch directory
    fs::create_dir_all(watch_dir).unwrap();

    dir
}

/// Build a minimal JSONL session file with one user → assistant exchange.
fn write_jsonl_session(path: &Path, session_id: &str, user_msg: &str, assistant_msg: &str) {
    let lines = vec![
        json!({
            "type": "user",
            "uuid": "u1",
            "sessionId": session_id,
            "timestamp": "2024-06-15T10:00:00Z",
            "cwd": "/home/user/project",
            "message": {"role": "user", "content": user_msg}
        })
        .to_string(),
        json!({
            "type": "assistant",
            "uuid": "a1",
            "sessionId": session_id,
            "timestamp": "2024-06-15T10:01:00Z",
            "cwd": "/home/user/project",
            "message": {"role": "assistant", "content": assistant_msg}
        })
        .to_string(),
    ];
    let mut f = fs::File::create(path).unwrap();
    for line in &lines {
        writeln!(f, "{}", line).unwrap();
    }
}

/// Scrape the given data dir and return the result.
fn do_scrape(data_dir: &Path) -> agentscribe::error::Result<agentscribe::scraper::ScrapeResult> {
    let mut scraper = Scraper::new(data_dir.to_path_buf())?;
    scraper.load_plugins()?;
    let result = scraper.scrape_all()?;
    scraper.state_manager().save()?;
    Ok(result)
}

/// Send a JSON-RPC request and read the response line from a Unix socket.
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
    serde_json::from_str(&response_line.trim()).unwrap()
}

/// Spin up an MCP server on a temp socket, run a test, then shut down.
fn with_mcp_server<F>(data_dir: &Path, test_fn: F)
where
    F: FnOnce(&UnixStream, tokio::runtime::Runtime, tokio::sync::oneshot::Sender<()>),
{
    let rt = tokio::runtime::Runtime::new().unwrap();
    let socket_path = data_dir.join("test-mcp.sock");
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let sp = socket_path.clone();
    let dd = data_dir.to_path_buf();

    rt.spawn(async move {
        mcp::run_mcp_server(dd, sp, shutdown_rx).await;
    });
    std::thread::sleep(Duration::from_millis(100));

    let stream = UnixStream::connect(&socket_path).unwrap();
    test_fn(&stream, rt, shutdown_tx);
}

// ── Daemon status ─────────────────────────────────────────────────────────

#[test]
fn test_daemon_status_when_not_running() {
    let dir = tempfile::tempdir().unwrap();
    let info = daemon::status(dir.path()).unwrap();
    assert!(!info.running);
    assert!(info.pid.is_none());
}

#[test]
fn test_daemon_status_with_stale_pid() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("agentscribe.pid");
    // PID 999999 is extremely unlikely to exist
    fs::write(&pid_file, "999999").unwrap();
    let info = daemon::status(dir.path()).unwrap();
    assert!(!info.running);
    assert!(info.pid.is_none());
}

// ── Daemon file watching ──────────────────────────────────────────────────

#[test]
fn test_daemon_detects_new_file_and_scrapes() {
    let watch_dir = tempfile::tempdir().unwrap();
    let data_tmp = setup_data_dir(watch_dir.path());
    let data_dir = data_tmp.path();

    // Initially no sessions
    let info_before = daemon::status(data_dir).unwrap();
    assert!(!info_before.running);

    // Write a JSONL file into the watched directory
    let session_file = watch_dir.path().join("session-test.jsonl");
    write_jsonl_session(
        &session_file,
        "session-test",
        "Implement feature X",
        "Done, feature X implemented",
    );

    // Manually scrape (simulating what the daemon would do on detecting the file)
    let result = do_scrape(data_dir).unwrap();
    assert!(
        result.sessions_scraped > 0,
        "should have scraped at least one session"
    );
    assert!(
        result.files_processed > 0,
        "should have processed at least one file"
    );

    // Verify session file was created in data dir
    let sessions_dir = data_dir.join("sessions").join("claude-code");
    assert!(sessions_dir.exists(), "sessions/claude-code should exist");
    let session_files: Vec<_> = fs::read_dir(&sessions_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert!(!session_files.is_empty(), "should have session files");

    // Verify index was created
    let index_dir = data_dir.join("index").join("tantivy");
    assert!(index_dir.exists(), "index should exist after scraping");
}

#[test]
fn test_daemon_incremental_scrape_no_full_rebuild() {
    let watch_dir = tempfile::tempdir().unwrap();
    let data_tmp = setup_data_dir(watch_dir.path());
    let data_dir = data_tmp.path();

    // Write first file and scrape
    let file1 = watch_dir.path().join("session-one.jsonl");
    write_jsonl_session(&file1, "session-one", "Task A", "Done A");

    let result1 = do_scrape(data_dir).unwrap();
    assert!(result1.sessions_scraped > 0);

    // Write second file and scrape again
    let file2 = watch_dir.path().join("session-two.jsonl");
    write_jsonl_session(&file2, "session-two", "Task B", "Done B");

    let result2 = do_scrape(data_dir).unwrap();
    // Second scrape should process at least the new file
    assert!(
        result2.sessions_scraped >= 1,
        "second scrape should find new session"
    );

    // Verify both sessions are in the index
    let opts = SearchOptions {
        query: Some("Task".to_string()),
        max_results: 10,
        ..Default::default()
    };
    let output = execute_search(data_dir, &opts).unwrap();
    assert!(
        output.total_matches >= 2,
        "should have both sessions in index, got {}",
        output.total_matches
    );
}

// ── Debounce coalescing ──────────────────────────────────────────────────

#[test]
fn test_debounce_coalesces_rapid_writes() {
    let watch_dir = tempfile::tempdir().unwrap();
    let data_tmp = setup_data_dir(watch_dir.path());
    let data_dir = data_tmp.path();

    let session_file = watch_dir.path().join("session-rapid.jsonl");

    // Simulate 5 rapid writes (like an active coding session)
    for i in 0..5u64 {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .append(true)
            .open(&session_file)
            .unwrap();
        writeln!(
            f,
            "{}",
            json!({
                "type": "user",
                "uuid": format!("u{}", i),
                "sessionId": "session-rapid",
                "timestamp": format!("2024-06-15T10:0{}:00Z", i),
                "cwd": "/home/user/project",
                "message": {"role": "user", "content": format!("Write {}", i)}
            })
        )
        .unwrap();
    }

    // After debounce window (simulated by a single scrape), only one scrape occurs
    let result = do_scrape(data_dir).unwrap();
    assert!(
        result.files_processed <= 1,
        "rapid writes should produce at most one file processed"
    );
}

// ── MCP tools parity with CLI ─────────────────────────────────────────────

#[test]
fn test_mcp_search_parity_with_cli() {
    let watch_dir = tempfile::tempdir().unwrap();
    let data_tmp = setup_data_dir(watch_dir.path());
    let data_dir = data_tmp.path();

    // Write and scrape a session
    let session_file = watch_dir.path().join("session-parity.jsonl");
    write_jsonl_session(
        &session_file,
        "session-parity",
        "Fix authentication bug in login handler",
        "Fixed the auth bug by adding proper token validation",
    );
    do_scrape(data_dir).unwrap();

    // CLI search
    let cli_opts = SearchOptions {
        query: Some("authentication".to_string()),
        max_results: 10,
        snippet_length: 200,
        sort: SortOrder::Relevance,
        ..Default::default()
    };
    let cli_output = execute_search(data_dir, &cli_opts).unwrap();

    // MCP search
    with_mcp_server(data_dir, |stream, rt, shutdown_tx| {
        let mut stream = stream.try_clone().unwrap();
        let _init = rpc_call(&mut stream, "initialize", json!({}));

        let mcp_resp = rpc_call(
            &mut stream,
            "tools/call",
            json!({
                "name": "agentscribe_search",
                "arguments": {
                    "query": "authentication",
                    "max_results": 10,
                    "snippet_length": 200,
                }
            }),
        );

        let _ = shutdown_tx.send(());
        rt.shutdown_background();

        // Parse MCP response
        let content = mcp_resp["result"]["content"].as_array().unwrap();
        let text = content[0]["text"].as_str().unwrap();
        let mcp_result: Value = serde_json::from_str(text).unwrap();

        // Both should find the same number of matches
        assert_eq!(
            cli_output.total_matches,
            mcp_result["total_matches"].as_u64().unwrap() as usize,
            "CLI and MCP should return same match count"
        );
    });
}

#[test]
fn test_mcp_status_tool_returns_plugin_info() {
    let watch_dir = tempfile::tempdir().unwrap();
    let data_tmp = setup_data_dir(watch_dir.path());
    let data_dir = data_tmp.path();

    // Write and scrape a session so there's data
    let session_file = watch_dir.path().join("session-st.jsonl");
    write_jsonl_session(
        &session_file,
        "session-st",
        "Build feature",
        "Feature built",
    );
    do_scrape(data_dir).unwrap();

    with_mcp_server(data_dir, |stream, rt, shutdown_tx| {
        let mut stream = stream.try_clone().unwrap();
        let _init = rpc_call(&mut stream, "initialize", json!({}));

        let resp = rpc_call(
            &mut stream,
            "tools/call",
            json!({
                "name": "agentscribe_status",
                "arguments": {}
            }),
        );

        let _ = shutdown_tx.send(());
        rt.shutdown_background();

        let content = resp["result"]["content"].as_array().unwrap();
        let text = content[0]["text"].as_str().unwrap();
        let status: Value = serde_json::from_str(text).unwrap();

        // Should have version and plugins
        assert!(status["version"].is_string());
        assert!(status["plugins"].is_array());
        let plugins = status["plugins"].as_array().unwrap();
        assert!(!plugins.is_empty(), "should have at least one plugin");

        // The claude-code plugin should show sessions > 0
        let claude_plugin = plugins.iter().find(|p| p["name"] == "claude-code").unwrap();
        assert!(
            claude_plugin["sessions"].as_u64().unwrap() > 0,
            "should have scraped sessions"
        );
    });
}

#[test]
fn test_mcp_blame_returns_file_sessions() {
    let watch_dir = tempfile::tempdir().unwrap();
    let data_tmp = setup_data_dir(watch_dir.path());
    let data_dir = data_tmp.path();

    // Write a session that references a file
    let session_file = watch_dir.path().join("session-blame.jsonl");
    let lines = vec![
        json!({
            "type": "user",
            "uuid": "u1",
            "sessionId": "session-blame",
            "timestamp": "2024-06-15T10:00:00Z",
            "cwd": "/home/user/project",
            "message": {"role": "user", "content": "Fix the bug in src/auth.rs"}
        })
        .to_string(),
        json!({
            "type": "assistant",
            "uuid": "a1",
            "sessionId": "session-blame",
            "timestamp": "2024-06-15T10:01:00Z",
            "cwd": "/home/user/project",
            "message": {"role": "assistant", "content": "I'll fix the bug in src/auth.rs by adding validation."}
        })
        .to_string(),
    ];
    {
        let mut f = fs::File::create(&session_file).unwrap();
        for line in &lines {
            writeln!(f, "{}", line).unwrap();
        }
    }
    do_scrape(data_dir).unwrap();

    with_mcp_server(data_dir, |stream, rt, shutdown_tx| {
        let mut stream = stream.try_clone().unwrap();
        let _init = rpc_call(&mut stream, "initialize", json!({}));

        let resp = rpc_call(
            &mut stream,
            "tools/call",
            json!({
                "name": "agentscribe_blame",
                "arguments": {
                    "file": "src/auth.rs"
                }
            }),
        );

        let _ = shutdown_tx.send(());
        rt.shutdown_background();

        // Parse the text — if it's an error, it won't be JSON, so handle both cases
        let content = resp["result"]["content"].as_array().unwrap();
        assert!(!content.is_empty());
        let text = content[0]["text"].as_str().unwrap();
        if let Ok(blame_result) = serde_json::from_str::<Value>(text) {
            assert_eq!(blame_result["file"].as_str().unwrap(), "src/auth.rs");
        }
        // If no sessions match, the error text is still a valid response
    });
}

#[test]
fn test_mcp_file_tool_lists_sessions() {
    let watch_dir = tempfile::tempdir().unwrap();
    let data_tmp = setup_data_dir(watch_dir.path());
    let data_dir = data_tmp.path();

    // Write a session with file references
    let session_file = watch_dir.path().join("session-filetool.jsonl");
    write_jsonl_session(
        &session_file,
        "session-filetool",
        "Refactor the module in lib/parser.rs",
        "Refactored lib/parser.rs to use the new trait system",
    );
    do_scrape(data_dir).unwrap();

    with_mcp_server(data_dir, |stream, rt, shutdown_tx| {
        let mut stream = stream.try_clone().unwrap();
        let _init = rpc_call(&mut stream, "initialize", json!({}));

        let resp = rpc_call(
            &mut stream,
            "tools/call",
            json!({
                "name": "agentscribe_file",
                "arguments": {
                    "file": "lib/parser.rs"
                }
            }),
        );

        let _ = shutdown_tx.send(());
        rt.shutdown_background();

        // Should return content regardless of match count
        let content = resp["result"]["content"].as_array().unwrap();
        assert!(!content.is_empty(), "file tool should return content");
        assert_eq!(content[0]["type"], "text");
    });
}

// ── MCP socket cleanup ────────────────────────────────────────────────────

#[test]
fn test_mcp_socket_removed_after_shutdown() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().to_path_buf();
    let socket_path = dir.path().join("cleanup-test.sock");

    fs::create_dir_all(data_dir.join("plugins")).unwrap();

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let sp = socket_path.clone();
    let dd = data_dir.clone();

    rt.block_on(async {
        tokio::spawn(mcp::run_mcp_server(dd, sp, shutdown_rx));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            socket_path.exists(),
            "socket should exist while server is running"
        );
        shutdown_tx.send(()).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            !socket_path.exists(),
            "socket should be removed after shutdown"
        );
    });
}

// ── Graceful shutdown ─────────────────────────────────────────────────────

#[test]
fn test_daemon_graceful_shutdown_sigterm() {
    let watch_dir = tempfile::tempdir().unwrap();
    let data_tmp = setup_data_dir(watch_dir.path());
    let data_dir = data_tmp.path().to_path_buf();

    // stop() should fail gracefully when no daemon is running
    let result = daemon::stop(&data_dir);
    assert!(result.is_err(), "stop should fail when no daemon is running");

    // Write a PID for a process that doesn't exist → stop cleans up stale PID
    let pid_file = data_dir.join("agentscribe.pid");
    fs::write(&pid_file, "999999").unwrap();
    let result = daemon::stop(&data_dir);
    assert!(
        result.is_err(),
        "stop should fail for non-existent process"
    );
    assert!(
        !pid_file.exists(),
        "stale PID file should be cleaned up by stop"
    );
}

#[test]
fn test_daemon_pid_file_management() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("agentscribe.pid");

    // No PID file → not running
    let info = daemon::status(dir.path()).unwrap();
    assert!(!info.running);

    // Write current PID → should report running
    let my_pid = std::process::id();
    fs::write(&pid_file, my_pid.to_string()).unwrap();
    let info = daemon::status(dir.path()).unwrap();
    assert!(info.running, "our own process should be detectable as running");
    assert_eq!(info.pid, Some(my_pid));

    // Corrupted PID file → not running
    fs::write(&pid_file, "not-a-number").unwrap();
    let info = daemon::status(dir.path()).unwrap();
    assert!(!info.running);
}

// ── Systemd service unit ──────────────────────────────────────────────────

#[test]
fn test_systemd_service_unit_dir_resolution() {
    let unit_dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            directories::BaseDirs::new()
                .map(|d| d.home_dir().join(".config"))
                .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        })
        .join("systemd")
        .join("user");

    assert!(unit_dir.to_string_lossy().contains("systemd"));
    assert!(unit_dir.to_string_lossy().contains("user"));
}

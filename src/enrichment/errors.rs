//! Error fingerprinting.
//!
//! Extracts and normalizes error patterns from event content.
//! Strips variable parts (paths, hosts, ports, PIDs, timestamps, UUIDs)
//! to create stable fingerprints that can be matched across sessions.

use std::sync::LazyLock;

use regex::Regex;

use crate::event::Event;

/// Regex patterns that extract error lines from content.
static ERROR_EXTRACT_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // Rust compiler errors
        r"error\[?[E\d]{4,5}\]?:.*",
        // Rust panics
        r"thread '.*' panicked at.*",
        // Python tracebacks (full line)
        r"(?:Traceback \(most recent call last\):.*?(?=\n\n|\Z))",
        // Python exceptions
        r"\w+Error: .+",
        // JavaScript/TypeScript errors
        r"(?:TypeError|ReferenceError|SyntaxError|RangeError): .+",
        // Go panics
        r"panic: .+",
        // Go fatal errors
        r"fatal error: .+",
        // HTTP status errors
        r"HTTP/[12]\.[01] [45]\d\d .+",
        // SQL errors
        r"(?:SQLSTATE\[|ORA-\d{5}:|ERROR: \d+ ).+",
        // Compilation errors
        r"(?:cannot find module|undefined symbol|undefined reference).+",
        // Network errors
        r"(?:ECONNREFUSED|ECONNRESET|ETIMEDOUT|ENOTFOUND|Connection refused|Connection timed out).+",
        // General error patterns
        r"(?:error|Error|ERROR): .+",
        // Permission errors
        r"Permission denied.*",
        // File not found
        r"(?:No such file|file not found|File not found).+",
    ]
    .into_iter()
    .map(|p| Regex::new(p).unwrap())
    .collect()
});

/// Normalization regexes: strip variable parts to create stable fingerprints.
static NORMALIZATION_RULES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        // File paths: /home/user/project/src/file.rs -> <PATH>
        (Regex::new(r"(?:/[\w.-]+){2,}").unwrap(), "<PATH>"),
        // Windows paths: C:\Users\... -> <PATH>
        (Regex::new(r"[A-Z]:\\[\w\\.-]+").unwrap(), "<PATH>"),
        // Host:port patterns: 192.168.1.1:8080 -> <HOST>:<PORT>
        (Regex::new(r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}").unwrap(), "<HOST>"),
        // Port numbers after known protocols
        (Regex::new(r":\d{2,5}(?=[/\s)]|$)").unwrap(), ":<PORT>"),
        // PIDs: pid 12345 -> pid <PID>
        (Regex::new(r"\bpid\s+\d+\b").unwrap(), "pid <PID>"),
        // Timestamps: 2026-03-20T10:30:00.123Z or similar ISO 8601
        (Regex::new(r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}").unwrap(), "<TIMESTAMP>"),
        // Simple timestamps: HH:MM:SS
        (Regex::new(r"\b\d{2}:\d{2}:\d{2}\b").unwrap(), "<TIME>"),
        // UUIDs
        (Regex::new(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}").unwrap(), "<UUID>"),
        // Memory addresses: 0x7f8b4c2d3a10
        (Regex::new(r"0x[0-9a-fA-F]{8,16}").unwrap(), "<ADDR>"),
        // Line numbers: :42 -> :<LINE>
        (Regex::new(r":(\d{2,5})(?=[\s,)\]:]|$)").unwrap(), ":<LINE>"),
        // Hex numbers (other than addresses): 0x1234
        (Regex::new(r"0x[0-9a-fA-F]{1,6}(?![0-9a-fA-F])").unwrap(), "<HEX>"),
        // Large numbers that might be sizes/offsets (6+ digits)
        (Regex::new(r"\b\d{6,}\b").unwrap(), "<NUM>"),
    ]
});

/// Extract error fingerprints from a piece of content.
///
/// Returns normalized error patterns found in the content.
pub fn extract_error_fingerprints(content: &str) -> Vec<String> {
    let mut fingerprints = Vec::new();

    for pattern in &*ERROR_EXTRACT_PATTERNS {
        for mat in pattern.find_iter(content) {
            let normalized = normalize_error(mat.as_str());
            if !normalized.is_empty() {
                fingerprints.push(normalized);
            }
        }
    }

    // Deduplicate while preserving order
    let mut seen = std::collections::HashSet::new();
    fingerprints.retain(|fp| seen.insert(fp.clone()));

    fingerprints
}

/// Normalize an error string by stripping variable parts.
pub fn normalize_error(error: &str) -> String {
    let mut normalized = error.to_string();

    for (pattern, replacement) in &*NORMALIZATION_RULES {
        normalized = pattern.replace_all(&normalized, *replacement).to_string();
    }

    // Collapse whitespace
    normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");

    // Truncate very long fingerprints
    if normalized.len() > 200 {
        if let Some(idx) = normalized.rfind(' ') {
            if idx > 150 {
                normalized.truncate(idx);
            }
        }
    }

    normalized
}

/// Enrich events with error fingerprints.
///
/// Returns new events with error_fingerprints populated.
pub fn enrich_events(events: &[Event]) -> Vec<Event> {
    events
        .iter()
        .map(|event| {
            let fps = extract_error_fingerprints(&event.content);
            if fps.is_empty() {
                event.clone()
            } else {
                let mut enriched = event.clone();
                enriched.error_fingerprints = fps;
                enriched
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_rust_error() {
        let content = "error[E0425]: cannot find value `x` in this scope\n  --> src/main.rs:42:5";
        let fps = extract_error_fingerprints(content);
        assert!(!fps.is_empty());
        // Should contain normalized error
        assert!(fps.iter().any(|f| f.contains("E0425")));
    }

    #[test]
    fn test_extract_python_traceback() {
        let content = "Traceback (most recent call last):\n  File \"app.py\", line 42, in <module>\nTypeError: 'NoneType' object is not callable";
        let fps = extract_error_fingerprints(content);
        assert!(!fps.is_empty());
        assert!(fps.iter().any(|f| f.contains("TypeError")));
    }

    #[test]
    fn test_extract_network_error() {
        let content = "Error: dial tcp 192.168.1.100:5432: connect: connection refused";
        let fps = extract_error_fingerprints(content);
        assert!(!fps.is_empty());
    }

    #[test]
    fn test_no_errors_in_clean_content() {
        let content = "The code looks good and all tests pass.";
        let fps = extract_error_fingerprints(content);
        assert!(fps.is_empty());
    }

    #[test]
    fn test_normalize_strips_paths() {
        let error = "error: cannot open file /home/user/project/src/main.rs";
        let normalized = normalize_error(error);
        assert!(!normalized.contains("/home/user"));
        assert!(normalized.contains("<PATH>"));
    }

    #[test]
    fn test_normalize_strips_hosts() {
        let error = "Error: Connection refused to 192.168.1.100:8080";
        let normalized = normalize_error(error);
        assert!(!normalized.contains("192.168.1.100"));
        assert!(normalized.contains("<HOST>"));
    }

    #[test]
    fn test_normalize_strips_uuids() {
        let error = "Error: Record 550e8400-e29b-41d4-a716-446655440000 not found";
        let normalized = normalize_error(error);
        assert!(!normalized.contains("550e8400"));
        assert!(normalized.contains("<UUID>"));
    }

    #[test]
    fn test_normalize_strips_timestamps() {
        let error = "Error at 2026-03-20T10:30:00.123Z: connection timeout";
        let normalized = normalize_error(error);
        assert!(!normalized.contains("2026-03-20"));
        assert!(normalized.contains("<TIMESTAMP>"));
    }

    #[test]
    fn test_normalize_strips_pids() {
        let error = "Process pid 12345 crashed with segfault";
        let normalized = normalize_error(error);
        assert!(!normalized.contains("12345"));
        assert!(normalized.contains("pid <PID>"));
    }

    #[test]
    fn test_normalize_strips_line_numbers() {
        let error = "error at src/main.rs:42:5";
        let normalized = normalize_error(error);
        assert!(normalized.contains(":<LINE>"));
    }

    #[test]
    fn test_fingerprints_match_across_sessions() {
        // Same error type but different variable parts
        let error1 = "error[E0425]: cannot find value `x` in src/main.rs:42";
        let error2 = "error[E0425]: cannot find value `y` in src/lib.rs:15";

        let fps1 = extract_error_fingerprints(error1);
        let fps2 = extract_error_fingerprints(error2);

        // Both should have E0425 fingerprints
        assert!(fps1.iter().any(|f| f.contains("E0425")));
        assert!(fps2.iter().any(|f| f.contains("E0425")));

        // After normalization, paths should be stripped
        let norm1 = normalize_error(error1);
        let norm2 = normalize_error(error2);
        assert!(!norm1.contains("src/main.rs"));
        assert!(!norm2.contains("src/lib.rs"));
    }

    #[test]
    fn test_enrich_events_adds_fingerprints() {
        use crate::event::Role;
        use chrono::Utc;

        let events = vec![
            Event::new(
                Utc::now(),
                "test/1".into(),
                "test".into(),
                Role::ToolResult,
                "error[E0425]: cannot find value".into(),
            ),
            Event::new(
                Utc::now(),
                "test/1".into(),
                "test".into(),
                Role::Assistant,
                "no errors here".into(),
            ),
        ];

        let enriched = enrich_events(&events);
        assert!(!enriched[0].error_fingerprints.is_empty());
        assert!(enriched[1].error_fingerprints.is_empty());
    }

    #[test]
    fn test_deduplication() {
        let content = "error: something failed\nerror: something failed";
        let fps = extract_error_fingerprints(content);
        assert_eq!(fps.len(), 1);
    }

    #[test]
    fn test_http_error() {
        let content = "HTTP/1.1 503 Service Unavailable";
        let fps = extract_error_fingerprints(content);
        assert!(!fps.is_empty());
        assert!(fps.iter().any(|f| f.contains("503")));
    }

    #[test]
    fn test_go_panic() {
        let content = "panic: runtime error: index out of range [0] with length 0";
        let fps = extract_error_fingerprints(content);
        assert!(!fps.is_empty());
        assert!(fps.iter().any(|f| f.contains("panic")));
    }
}

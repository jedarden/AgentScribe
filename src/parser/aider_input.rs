//! Aider input history parser
//!
//! Parses `.aider.input.history` files in prompt_toolkit format.
//! Format:
//!   # timestamp
//!   + user input text
//!
//! Each user input is prefixed with a timestamp line. This provides
//! per-input timestamps for finer session granularity.

use crate::error::{AgentScribeError, Result};
use chrono::{DateTime, Utc};
use regex::Regex;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Parsed entry from .aider.input.history
#[derive(Debug, Clone)]
pub struct AiderInputEntry {
    /// Timestamp when the input was submitted
    pub timestamp: DateTime<Utc>,
    /// The user input text
    pub input: String,
}

/// A parsed .aider.input.history file
///
/// Maps user input content (first N chars as key) to timestamp.
/// The key is truncated because the full input may not match exactly
/// due to whitespace differences or truncation in the chat history.
#[derive(Debug, Clone)]
pub struct AiderInputHistory {
    /// Map from input prefix -> timestamp
    /// Using prefix (first 100 chars) as key for fuzzy matching
    entries: HashMap<String, DateTime<Utc>>,
    /// Ordered list of timestamps for sequence matching
    timestamps: Vec<DateTime<Utc>>,
}

impl AiderInputHistory {
    /// Parse a .aider.input.history file
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let file = File::open(path)
            .map_err(|_| AgentScribeError::FileNotFound(path.to_path_buf()))?;
        let reader = BufReader::new(file);

        let mut entries = HashMap::new();
        let mut timestamps = Vec::new();
        let mut current_ts: Option<DateTime<Utc>> = None;
        let mut input_lines = Vec::new();

        // Pattern: # timestamp (ISO 8601 or similar)
        let ts_re = Regex::new(r#"^#\s*(.+)$"#)
            .map_err(|e| AgentScribeError::Parse {
                file: path.display().to_string(),
                line: None,
                message: format!("Invalid timestamp regex: {}", e),
            })?;

        for (line_num, line_result) in reader.lines().enumerate() {
            let line = line_result.map_err(|e| AgentScribeError::Parse {
                file: path.display().to_string(),
                line: Some(line_num + 1),
                message: format!("Read error: {}", e),
            })?;

            // Check for timestamp line
            if let Some(caps) = ts_re.captures(&line) {
                // Flush previous input if any
                if let Some(ts) = current_ts {
                    if !input_lines.is_empty() {
                        let input = input_lines.join("\n");
                        // Use first 100 chars as key for fuzzy matching
                        let key = Self::make_key(&input);
                        entries.insert(key, ts);
                        timestamps.push(ts);
                    }
                }

                // Parse new timestamp
                let ts_str = caps.get(1).unwrap().as_str().trim();
                current_ts = Some(Self::parse_timestamp(ts_str)?);
                input_lines.clear();
            } else if line.starts_with('+') {
                // Content line (strip the '+' prefix)
                let content = line[1..].trim().to_string();
                if !content.is_empty() {
                    input_lines.push(content);
                }
            } else if !line.is_empty() && current_ts.is_some() {
                // Continuation of current input (no prefix)
                input_lines.push(line);
            }
        }

        // Flush final entry
        if let Some(ts) = current_ts {
            if !input_lines.is_empty() {
                let input = input_lines.join("\n");
                let key = Self::make_key(&input);
                entries.insert(key, ts);
                timestamps.push(ts);
            }
        }

        Ok(AiderInputHistory {
            entries,
            timestamps,
        })
    }

    /// Create a key for the input map
    fn make_key(input: &str) -> String {
        // Normalize: trim, collapse whitespace, truncate to 100 chars
        let normalized = input
            .trim()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if normalized.len() > 100 {
            normalized[..100].to_string()
        } else {
            normalized
        }
    }

    /// Parse a timestamp from the input history
    ///
    /// Accepts multiple formats:
    /// - ISO 8601: 2026-03-16T12:00:00Z
    /// - With microseconds: 2026-03-16 12:00:00.123456
    /// - Simple: 2026-03-16 12:00:00
    fn parse_timestamp(s: &str) -> Result<DateTime<Utc>> {
        // Try ISO 8601 first
        if let Ok(dt) = s.parse::<DateTime<Utc>>() {
            return Ok(dt);
        }

        // Try common formats
        let formats = [
            "%Y-%m-%d %H:%M:%S%.f",     // 2026-03-16 12:00:00.123456
            "%Y-%m-%d %H:%M:%S",        // 2026-03-16 12:00:00
            "%Y-%m-%dT%H:%M:%S%.f",     // 2026-03-16T12:00:00.123456
            "%Y-%m-%dT%H:%M:%S",        // 2026-03-16T12:00:00
            "%Y-%m-%d %H:%M:%S%.f %Z",  // with timezone
        ];

        for fmt in &formats {
            if let Ok(dt) = DateTime::parse_from_str(s, fmt) {
                return Ok(dt.with_timezone(&Utc));
            }
        }

        // Assume local time if no timezone specified
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f") {
            return Ok(DateTime::from_naive_utc_and_offset(dt, Utc));
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            return Ok(DateTime::from_naive_utc_and_offset(dt, Utc));
        }

        Err(AgentScribeError::Parse {
            file: ".aider.input.history".to_string(),
            line: None,
            message: format!("Unable to parse timestamp: {}", s),
        })
    }

    /// Find the best matching timestamp for a user input
    ///
    /// This matches user events from the chat history to timestamps
    /// from the input history. Returns None if no match is found.
    pub fn find_timestamp_for_input(&self, input: &str) -> Option<DateTime<Utc>> {
        let key = Self::make_key(input);
        self.entries.get(&key).copied()
    }

    /// Get the Nth timestamp by sequence (0-indexed)
    ///
    /// Useful for matching user events in order when exact
    /// content matching isn't available.
    pub fn get_timestamp_by_sequence(&self, index: usize) -> Option<DateTime<Utc>> {
        self.timestamps.get(index).copied()
    }

    /// Get the number of input entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_aider_input_history() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write test data
        let mut file = File::create(path).unwrap();
        writeln!(file, "# 2026-03-16 12:00:00").unwrap();
        writeln!(file, "+ Fix the login bug").unwrap();
        writeln!(file, "# 2026-03-16 12:01:30").unwrap();
        writeln!(file, "+ Update the auth middleware").unwrap();
        writeln!(file, "+ Add JWT validation").unwrap();

        // Parse
        let history = AiderInputHistory::load_from_file(path).unwrap();

        assert_eq!(history.len(), 2);

        // Check first entry
        let ts1 = history.find_timestamp_for_input("Fix the login bug");
        assert!(ts1.is_some());

        // Check second entry (multiline)
        let ts2 = history.find_timestamp_for_input("Update the auth middleware\nAdd JWT validation");
        assert!(ts2.is_some());

        // Timestamps should be in order
        let seq1 = history.get_timestamp_by_sequence(0);
        let seq2 = history.get_timestamp_by_sequence(1);
        assert!(seq1.is_some());
        assert!(seq2.is_some());
        assert!(seq1 < seq2);
    }

    #[test]
    fn test_timestamp_parsing() {
        // ISO 8601
        let ts = AiderInputHistory::parse_timestamp("2026-03-16T12:00:00Z").unwrap();
        assert_eq!(ts.timestamp(), 1773662400);

        // Space format
        let ts = AiderInputHistory::parse_timestamp("2026-03-16 12:00:00").unwrap();
        assert_eq!(ts.timestamp(), 1773662400);

        // With microseconds
        let ts = AiderInputHistory::parse_timestamp("2026-03-16 12:00:00.123456").unwrap();
        assert_eq!(ts.timestamp(), 1773662400);
    }

    #[test]
    fn test_key_normalization() {
        let key1 = AiderInputHistory::make_key("  Fix   the   login  bug  ");
        let key2 = AiderInputHistory::make_key("Fix the login bug");
        assert_eq!(key1, key2);
        assert_eq!(key1, "Fix the login bug");
    }

    #[test]
    fn test_empty_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        File::create(path).unwrap();

        let history = AiderInputHistory::load_from_file(path).unwrap();
        assert!(history.is_empty());
    }

    #[test]
    fn test_missing_file() {
        let history = AiderInputHistory::load_from_file(Path::new("/nonexistent/.aider.input.history"));
        assert!(history.is_err());
    }
}

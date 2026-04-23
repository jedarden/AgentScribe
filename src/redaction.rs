//! Privacy redaction scanner.
//!
//! Scans transcription text for personally identifiable information (PII)
//! and replaces matches with labelled placeholders before storage or indexing.
//! Applied to all Whisper transcripts as required by the privacy policy (§18).

use crate::config::RedactionConfig;
use regex::Regex;
use std::sync::LazyLock;

// ─── Precompiled PII patterns ─────────────────────────────────────────────────

static EMAIL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}\b").unwrap());

static PHONE_RE: LazyLock<Regex> = LazyLock::new(|| {
    // US / international: +1 (555) 555-5555 · 555-555-5555 · (555) 555 5555
    // \b at start prevents matching mid-number (e.g. inside a credit card).
    Regex::new(r"\b(?:\+\d{1,3}[\s\-]?)?(?:\(?\d{3}\)?[\s\-\.])?\d{3}[\s\-\.]\d{4}\b").unwrap()
});

static CREDIT_CARD_RE: LazyLock<Regex> = LazyLock::new(|| {
    // 13-19 digit card numbers, optionally grouped by spaces or dashes
    Regex::new(r"\b(?:\d{4}[\s\-]){3}\d{1,4}\b|\b\d{16,19}\b").unwrap()
});

static SSN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{3}[\s\-]\d{2}[\s\-]\d{4}\b").unwrap());

// ─── RedactionScanner ─────────────────────────────────────────────────────────

/// Scans text for PII and replaces matches with labelled tokens.
#[derive(Clone)]
pub struct RedactionScanner {
    config: RedactionConfig,
    custom_patterns: Vec<Regex>,
}

impl RedactionScanner {
    /// Build a scanner from the given config.
    ///
    /// Invalid custom patterns are silently skipped with a warning logged.
    pub fn new(config: RedactionConfig) -> Self {
        let custom_patterns = config
            .custom_patterns
            .iter()
            .filter_map(|p| match Regex::new(p) {
                Ok(re) => Some(re),
                Err(e) => {
                    tracing::warn!(pattern = %p, error = %e, "skipping invalid redaction pattern");
                    None
                }
            })
            .collect();

        RedactionScanner {
            config,
            custom_patterns,
        }
    }

    /// Return `text` with all enabled PII categories replaced by placeholders.
    pub fn redact(&self, text: &str) -> String {
        if !self.config.enabled || text.is_empty() {
            return text.to_string();
        }

        let mut result = text.to_string();

        if self.config.redact_emails {
            result = EMAIL_RE.replace_all(&result, "[EMAIL]").into_owned();
        }
        if self.config.redact_phones {
            result = PHONE_RE.replace_all(&result, "[PHONE]").into_owned();
        }
        if self.config.redact_credit_cards {
            result = CREDIT_CARD_RE.replace_all(&result, "[CARD]").into_owned();
        }
        if self.config.redact_ssn {
            result = SSN_RE.replace_all(&result, "[SSN]").into_owned();
        }
        for re in &self.custom_patterns {
            result = re.replace_all(&result, "[REDACTED]").into_owned();
        }

        result
    }

    /// Return `true` if any enabled PII pattern matches `text`.
    pub fn has_pii(&self, text: &str) -> bool {
        if !self.config.enabled {
            return false;
        }
        (self.config.redact_emails && EMAIL_RE.is_match(text))
            || (self.config.redact_phones && PHONE_RE.is_match(text))
            || (self.config.redact_credit_cards && CREDIT_CARD_RE.is_match(text))
            || (self.config.redact_ssn && SSN_RE.is_match(text))
            || self.custom_patterns.iter().any(|re| re.is_match(text))
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn default_scanner() -> RedactionScanner {
        RedactionScanner::new(RedactionConfig::default())
    }

    #[test]
    fn test_email_redaction() {
        let s = default_scanner();
        let result = s.redact("Contact me at alice@example.com for details.");
        assert!(!result.contains("alice@example.com"));
        assert!(result.contains("[EMAIL]"));
    }

    #[test]
    fn test_ssn_redaction() {
        let s = default_scanner();
        let result = s.redact("SSN: 123-45-6789");
        assert!(!result.contains("123-45-6789"));
        assert!(result.contains("[SSN]"));
    }

    #[test]
    fn test_credit_card_redaction() {
        let s = default_scanner();
        let result = s.redact("Card: 4111 1111 1111 1111");
        assert!(!result.contains("4111 1111 1111 1111"));
        assert!(result.contains("[CARD]"));
    }

    #[test]
    fn test_no_redaction_when_disabled() {
        let config = RedactionConfig {
            enabled: false,
            ..Default::default()
        };
        let s = RedactionScanner::new(config);
        let text = "alice@example.com";
        assert_eq!(s.redact(text), text);
    }

    #[test]
    fn test_has_pii_detection() {
        let s = default_scanner();
        assert!(s.has_pii("reach me at bob@corp.io"));
        assert!(!s.has_pii("no sensitive data here"));
    }

    #[test]
    fn test_custom_pattern() {
        let config = RedactionConfig {
            custom_patterns: vec![r"ACCT-\d+".to_string()],
            ..Default::default()
        };
        let s = RedactionScanner::new(config);
        let result = s.redact("Account ACCT-99887 is overdue.");
        assert!(!result.contains("ACCT-99887"));
        assert!(result.contains("[REDACTED]"));
    }

    #[test]
    fn test_invalid_custom_pattern_ignored() {
        let config = RedactionConfig {
            custom_patterns: vec!["[invalid(".to_string()],
            ..Default::default()
        };
        // Should not panic — invalid patterns are silently skipped.
        let s = RedactionScanner::new(config);
        assert_eq!(s.redact("hello"), "hello");
    }
}

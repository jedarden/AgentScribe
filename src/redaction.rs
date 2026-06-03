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
    fn test_phone_redaction() {
        let s = default_scanner();
        // Test various phone formats
        let test_cases = vec![
            ("Call me at 555-123-4567", "[PHONE]"),
            ("+1 (555) 555-5555", "[PHONE]"),
            ("(555) 555 5555", "[PHONE]"),
            ("555.555.5555", "[PHONE]"),
            ("Phone: 1-800-555-0199", "[PHONE]"),
        ];
        for (input, expected) in test_cases {
            let result = s.redact(input);
            assert!(result.contains(expected), "Failed for: {}", input);
            assert!(
                !result.contains("555"),
                "Phone number not redacted in: {}",
                input
            );
        }
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

    // ─── Regression test for PHONE_RE \b edge case (f0c6efa) ───────────

    // Regression test for bug fixed in f0c6efa: PHONE_RE was missing leading \b
    // and would match inside credit card numbers, breaking credit-card redaction.
    #[test]
    fn test_phone_re_does_not_match_inside_credit_card() {
        // Create scanner that only redacts phones (not cards)
        let config = RedactionConfig {
            enabled: true,
            redact_emails: false,
            redact_phones: true,
            redact_credit_cards: false,
            redact_ssn: false,
            custom_patterns: vec![],
        };
        let s = RedactionScanner::new(config);

        // Credit card number "4111 1111 1111 1111" contains "111-1111" which
        // looks like a phone number. PHONE_RE should NOT match it due to \b.
        let result = s.redact("Card: 4111 1111 1111 1111");
        assert_eq!(
            result, "Card: 4111 1111 1111 1111",
            "PHONE_RE should not match inside credit card digits"
        );

        // Another example: "4242 4242 4242 4242" could have "424-2424" pattern
        let result = s.redact("Card: 4242 4242 4242 4242");
        assert_eq!(
            result, "Card: 4242 4242 4242 4242",
            "PHONE_RE should not match 424-2424 pattern inside card"
        );
    }

    // ─── Credit card negative cases ─────────────────────────────────────

    #[test]
    fn test_credit_card_does_not_match_short_numbers() {
        let s = default_scanner();

        // 12-digit number (too short for credit card)
        let result = s.redact("ID: 1234 5678 9012");
        assert!(result.contains("1234 5678 9012"));
        assert!(!result.contains("[CARD]"));

        // 15-digit without proper grouping (below 16 threshold for plain numbers)
        let result = s.redact("Number: 123456789012345");
        assert!(result.contains("123456789012345"));
        assert!(!result.contains("[CARD]"));
    }

    #[test]
    fn test_credit_card_does_not_match_mid_sequence() {
        let s = default_scanner();

        // 20-digit number (too long for credit card)
        let result = s.redact("Long: 12345678901234567890");
        assert!(result.contains("12345678901234567890"));
        assert!(!result.contains("[CARD]"));

        // Partial grouping at end - note: the grouped pattern matches 3 groups of 4 digits
        // followed by 1-4 more digits, so "1234 5678 9012 34" does match (14 digits total)
        let result = s.redact("Ends with 1234 5678 9012 34");
        assert!(!result.contains("1234 5678 9012 34"));
        assert!(result.contains("[CARD]"));

        // But "1234 5678 9012" (only 3 groups, no trailing digits) should not match
        let result = s.redact("Just 1234 5678 9012 here");
        assert!(result.contains("1234 5678 9012"));
        assert!(!result.contains("[CARD]"));
    }

    #[test]
    fn test_credit_card_variations() {
        let s = default_scanner();

        // 15-digit card (below 16-19 threshold for plain numbers)
        let result = s.redact("Card: 378282246310005");
        assert!(result.contains("378282246310005"));
        assert!(!result.contains("[CARD]"));

        // 16-digit with dashes
        let result = s.redact("Card: 4111-1111-1111-1111");
        assert!(!result.contains("4111-1111-1111-1111"));
        assert!(result.contains("[CARD]"));

        // 19-digit card
        let result = s.redact("Card: 1234567890123456789");
        assert!(!result.contains("1234567890123456789"));
        assert!(result.contains("[CARD]"));

        // 13-digit grouped (Amex-style: 4-6-3 grouping not supported, but 4-4-4-1 is)
        let result = s.redact("Card: 3782 822463 10005");
        assert!(result.contains("3782 822463 10005"));
        assert!(!result.contains("[CARD]"));

        // 16-digit plain number
        let result = s.redact("Card: 4111111111111111");
        assert!(!result.contains("4111111111111111"));
        assert!(result.contains("[CARD]"));
    }

    // ─── SSN negative cases ─────────────────────────────────────────────

    #[test]
    fn test_ssn_does_not_match_invalid_formats() {
        let s = default_scanner();

        // SSN with spaces between groups (valid - space is in [\s\-] character class)
        let result = s.redact("SSN: 123 45 6789");
        assert!(!result.contains("123 45 6789"));
        assert!(result.contains("[SSN]"));

        // SSN without separators (should not match)
        let result = s.redact("ID: 123456789");
        assert!(result.contains("123456789"));
        assert!(!result.contains("[SSN]"));

        // SSN with wrong grouping (2-3-4 instead of 3-2-4)
        // Note: this will be redacted by phone pattern matching "45-6789",
        // so we use a scanner with only SSN enabled to test SSN behavior.
        let config = RedactionConfig {
            enabled: true,
            redact_emails: false,
            redact_phones: false,
            redact_credit_cards: false,
            redact_ssn: true,
            custom_patterns: vec![],
        };
        let s = RedactionScanner::new(config);
        let result = s.redact("ID: 12-345-6789");
        assert!(result.contains("12-345-6789"));
        assert!(!result.contains("[SSN]"));

        // SSN with 4-2-4 grouping (wrong)
        let result = s.redact("ID: 1234-56-7890");
        assert!(result.contains("1234-56-7890"));
        assert!(!result.contains("[SSN]"));
    }

    // ─── Email negative cases ───────────────────────────────────────────

    #[test]
    fn test_email_does_not_match_invalid_formats() {
        let s = default_scanner();

        // Missing TLD
        let result = s.redact("Contact at user@localhost");
        assert!(result.contains("user@localhost"));
        assert!(!result.contains("[EMAIL]"));

        // Missing @ symbol
        let result = s.redact("Contact at user.example.com");
        assert!(result.contains("user.example.com"));
        assert!(!result.contains("[EMAIL]"));

        // Invalid TLD (single char)
        let result = s.redact("Contact at user@example.c");
        assert!(result.contains("user@example.c"));
        assert!(!result.contains("[EMAIL]"));
    }

    // ─── Phone negative cases ───────────────────────────────────────────

    #[test]
    fn test_phone_does_not_match_short_numbers() {
        let s = default_scanner();

        // Too short (less than 7 digits after area code)
        let result = s.redact("Extension: x1234");
        assert!(result.contains("x1234"));
        assert!(!result.contains("[PHONE]"));

        // 6-digit number (too short)
        let result = s.redact("Code: 123-456");
        assert!(result.contains("123-456"));
        assert!(!result.contains("[PHONE]"));
    }

    // ─── Multiple PII in one text ───────────────────────────────────────

    #[test]
    fn test_multiple_pii_types_in_one_text() {
        let s = default_scanner();
        let input = "Contact alice@example.com or call 555-123-4567. Card: 4111 1111 1111 1111. SSN: 123-45-6789.";
        let result = s.redact(input);

        assert!(!result.contains("alice@example.com"));
        assert!(!result.contains("555-123-4567"));
        assert!(!result.contains("4111 1111 1111 1111"));
        assert!(!result.contains("123-45-6789"));

        assert!(result.contains("[EMAIL]"));
        assert!(result.contains("[PHONE]"));
        assert!(result.contains("[CARD]"));
        assert!(result.contains("[SSN]"));
    }

    // ─── Selective redaction ────────────────────────────────────────────

    #[test]
    fn test_selective_redaction_phones_only() {
        let config = RedactionConfig {
            enabled: true,
            redact_emails: false,
            redact_phones: true,
            redact_credit_cards: false,
            redact_ssn: false,
            custom_patterns: vec![],
        };
        let s = RedactionScanner::new(config);
        let result = s.redact("alice@example.com or 555-123-4567");

        assert!(result.contains("alice@example.com"));
        assert!(!result.contains("555-123-4567"));
        assert!(result.contains("[PHONE]"));
        assert!(!result.contains("[EMAIL]"));
    }

    #[test]
    fn test_selective_redaction_emails_only() {
        let config = RedactionConfig {
            enabled: true,
            redact_emails: true,
            redact_phones: false,
            redact_credit_cards: false,
            redact_ssn: false,
            custom_patterns: vec![],
        };
        let s = RedactionScanner::new(config);
        let result = s.redact("alice@example.com or 555-123-4567");

        assert!(!result.contains("alice@example.com"));
        assert!(result.contains("555-123-4567"));
        assert!(!result.contains("[PHONE]"));
        assert!(result.contains("[EMAIL]"));
    }

    // ─── Additional edge cases ─────────────────────────────────────────────

    #[test]
    fn test_empty_text() {
        let s = default_scanner();
        assert_eq!(s.redact(""), "");
        assert!(!s.has_pii(""));
    }

    #[test]
    fn test_text_with_no_pii() {
        let s = default_scanner();
        let text = "Hello world, this is just plain text with no sensitive data.";
        assert_eq!(s.redact(text), text);
        assert!(!s.has_pii(text));
    }

    #[test]
    fn test_multiple_emails_in_one_text() {
        let s = default_scanner();
        let result = s.redact("Contact alice@example.com or bob@corp.io or carol@domain.org");
        assert!(!result.contains("alice@example.com"));
        assert!(!result.contains("bob@corp.io"));
        assert!(!result.contains("carol@domain.org"));
        assert_eq!(result.matches("[EMAIL]").count(), 3);
    }

    #[test]
    fn test_unusual_but_valid_email_formats() {
        let s = default_scanner();
        let test_cases = vec![
            "user+tag@example.com",
            "user.name@example.co.uk",
            "user_name@sub.domain.example.com",
            "12345@example.com",
            "user+test+filter@example-domain.io",
        ];
        for email in test_cases {
            let result = s.redact(email);
            assert!(!result.contains(email), "Email not redacted: {}", email);
            assert!(
                result.contains("[EMAIL]"),
                "No [EMAIL] placeholder for: {}",
                email
            );
        }
    }

    #[test]
    fn test_international_phone_formats() {
        let s = default_scanner();
        // PHONE_RE supports US/NANP-style international numbers (with +1 country code)
        // and standard 7-10 digit formats with various separators
        let test_cases = vec![
            ("Call +1-555-123-4567", "[PHONE]"),
            ("Phone: +1 (555) 555-5555", "[PHONE]"),
            ("Mobile: +1 555.555.5555", "[PHONE]"),
            // Also test various separators without country code
            ("Tel: 555.123.4567", "[PHONE]"),
            ("Direct: (555) 555-5555", "[PHONE]"),
        ];
        for (input, expected) in test_cases {
            let result = s.redact(input);
            assert!(result.contains(expected), "Failed for: {}", input);
        }
    }

    #[test]
    fn test_has_pii_with_selective_redaction() {
        // Scanner that only redacts emails
        let config = RedactionConfig {
            enabled: true,
            redact_emails: true,
            redact_phones: false,
            redact_credit_cards: false,
            redact_ssn: false,
            custom_patterns: vec![],
        };
        let s = RedactionScanner::new(config);

        // Should detect PII when email is present
        assert!(s.has_pii("Contact alice@example.com"));

        // Should NOT detect PII when only phone is present (phones not enabled)
        assert!(!s.has_pii("Call 555-123-4567"));

        // Should NOT detect PII when only credit card is present
        assert!(!s.has_pii("Card: 4111 1111 1111 1111"));
    }

    #[test]
    fn test_custom_pattern_with_special_chars() {
        let config = RedactionConfig {
            custom_patterns: vec![r"KEY\[[A-Z0-9]+\]".to_string()],
            ..Default::default()
        };
        let s = RedactionScanner::new(config);
        let result = s.redact("Use KEY[ABCD] to access.");
        assert!(!result.contains("KEY[ABCD]"));
        assert!(result.contains("[REDACTED]"));
    }

    #[test]
    fn test_multiple_custom_patterns() {
        let config = RedactionConfig {
            custom_patterns: vec![
                r"ACCT-\d+".to_string(),
                r"ID:\d{5}".to_string(),
                r"SECRET\[[^\]]+\]".to_string(),
            ],
            ..Default::default()
        };
        let s = RedactionScanner::new(config);
        let result = s.redact("Account ACCT-99887, ID:12345, and SECRET[passw0rd]");
        assert!(!result.contains("ACCT-99887"));
        assert!(!result.contains("ID:12345"));
        assert!(!result.contains("SECRET[passw0rd]"));
        assert_eq!(result.matches("[REDACTED]").count(), 3);
    }

    // ─── Email positive match tests ─────────────────────────────────────────

    /// Positive match test: Standard email format user@domain.com
    #[test]
    fn test_email_positive_match_standard_format() {
        let s = default_scanner();
        let input = "Contact test@example.com for details";
        let result = s.redact(input);

        assert!(result.contains("[EMAIL]"), "Expected [EMAIL] placeholder");
        assert!(
            !result.contains("test@example.com"),
            "Email should be redacted"
        );
        assert!(!result.contains("@"), "No @ symbol should remain");
    }

    /// Positive match test: Email with dots in local part
    #[test]
    fn test_email_positive_match_dotted_local_part() {
        let s = default_scanner();
        let input = "Email user.name@domain.co.uk for support";
        let result = s.redact(input);

        assert!(result.contains("[EMAIL]"), "Expected [EMAIL] placeholder");
        assert!(
            !result.contains("user.name@domain.co.uk"),
            "Email should be redacted"
        );
        assert!(!result.contains("@"), "No @ symbol should remain");
    }

    /// Positive match test: Email with plus addressing
    #[test]
    fn test_email_positive_match_plus_addressing() {
        let s = default_scanner();
        let input = "Send to user+tag@example.org";
        let result = s.redact(input);

        assert!(result.contains("[EMAIL]"), "Expected [EMAIL] placeholder");
        assert!(
            !result.contains("user+tag@example.org"),
            "Email should be redacted"
        );
        assert!(!result.contains("@"), "No @ symbol should remain");
    }

    /// Positive match test: Email with numbers in local part
    #[test]
    fn test_email_positive_match_numeric_local_part() {
        let s = default_scanner();
        let input = "Contact 123user@example.com or user456@test.io";
        let result = s.redact(input);

        assert!(result.contains("[EMAIL]"), "Expected [EMAIL] placeholder");
        assert!(
            !result.contains("123user@example.com"),
            "First email should be redacted"
        );
        assert!(
            !result.contains("user456@test.io"),
            "Second email should be redacted"
        );
        assert!(!result.contains("@"), "No @ symbol should remain");
    }

    /// Positive match test: Email with hyphens in domain
    #[test]
    fn test_email_positive_match_hyphenated_domain() {
        let s = default_scanner();
        let input = "Email admin@my-domain.com for access";
        let result = s.redact(input);

        assert!(result.contains("[EMAIL]"), "Expected [EMAIL] placeholder");
        assert!(
            !result.contains("admin@my-domain.com"),
            "Email should be redacted"
        );
        assert!(!result.contains("@"), "No @ symbol should remain");
    }

    /// Positive match test: Email with multi-part TLD
    #[test]
    fn test_email_positive_match_multi_part_tld() {
        let s = default_scanner();
        let test_cases = vec![
            "user@example.co.uk",
            "admin@domain.gov.au",
            "support@service.ac.nz",
        ];

        for email in test_cases {
            let result = s.redact(email);
            assert!(result.contains("[EMAIL]"), "Expected [EMAIL] for {}", email);
            assert!(
                !result.contains(email),
                "Email {} should be redacted",
                email
            );
            assert!(
                !result.contains("@"),
                "No @ symbol should remain for {}",
                email
            );
        }
    }

    /// Positive match test: Email with underscores in local part
    #[test]
    fn test_email_positive_match_underscore_local_part() {
        let s = default_scanner();
        let input = "Contact user_name@example.com";
        let result = s.redact(input);

        assert!(result.contains("[EMAIL]"), "Expected [EMAIL] placeholder");
        assert!(
            !result.contains("user_name@example.com"),
            "Email should be redacted"
        );
        assert!(!result.contains("@"), "No @ symbol should remain");
    }

    /// Positive match test: Multiple emails in single text
    #[test]
    fn test_email_positive_match_multiple_emails() {
        let s = default_scanner();
        let input = "Contact alice@example.com, bob@corp.co.uk, or carol@domain.org";
        let result = s.redact(input);

        assert_eq!(
            result.matches("[EMAIL]").count(),
            3,
            "Expected 3 [EMAIL] placeholders"
        );
        assert!(
            !result.contains("alice@example.com"),
            "First email should be redacted"
        );
        assert!(
            !result.contains("bob@corp.co.uk"),
            "Second email should be redacted"
        );
        assert!(
            !result.contains("carol@domain.org"),
            "Third email should be redacted"
        );
        assert!(!result.contains("@"), "No @ symbol should remain");
    }

    /// Positive match test: Email at start of text
    #[test]
    fn test_email_positive_match_at_start() {
        let s = default_scanner();
        let input = "test@example.com is my email";
        let result = s.redact(input);

        assert!(result.contains("[EMAIL]"), "Expected [EMAIL] placeholder");
        assert!(
            !result.contains("test@example.com"),
            "Email should be redacted"
        );
    }

    /// Positive match test: Email at end of text
    #[test]
    fn test_email_positive_match_at_end() {
        let s = default_scanner();
        let input = "My email is test@example.com";
        let result = s.redact(input);

        assert!(result.contains("[EMAIL]"), "Expected [EMAIL] placeholder");
        assert!(
            !result.contains("test@example.com"),
            "Email should be redacted"
        );
    }

    /// Positive match test: Email with subdomain in domain
    #[test]
    fn test_email_positive_match_subdomain() {
        let s = default_scanner();
        let input = "Email user@sub.domain.example.com";
        let result = s.redact(input);

        assert!(result.contains("[EMAIL]"), "Expected [EMAIL] placeholder");
        assert!(
            !result.contains("user@sub.domain.example.com"),
            "Email should be redacted"
        );
        assert!(!result.contains("@"), "No @ symbol should remain");
    }

    /// Positive match test: has_pii returns true for emails
    #[test]
    fn test_email_positive_match_has_pii() {
        let s = default_scanner();

        // All these should be detected as containing PII
        assert!(s.has_pii("test@example.com"));
        assert!(s.has_pii("user.name@domain.co.uk"));
        assert!(s.has_pii("user+tag@example.org"));
        assert!(s.has_pii("Contact me at admin@example.com for details"));

        // Text without email should not be detected
        assert!(!s.has_pii("No sensitive data here"));
        assert!(!s.has_pii("Just regular text without PII"));
    }
}

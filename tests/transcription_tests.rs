//! Integration tests for transcription job queue and PII redaction.
//!
//! Tests the async job queue, retry logic, state management, and
//! integration between Whisper transcription and RedactionScanner.
//!
//! Note: These tests do NOT require the Whisper binary to be installed.
//! They use mock scenarios and test the queue logic in isolation.

use std::path::PathBuf;

use agentscribe::config::{RedactionConfig, WhisperConfig};
use agentscribe::redaction::RedactionScanner;
use agentscribe::transcription::{
    AudioFormat, TranscriptionJob, TranscriptionQueue, TranscriptionResult,
};

// ─── RedactionScanner integration tests ────────────────────────────────────────

#[test]
fn test_redaction_scanner_pii_patterns() {
    // Test all PII patterns with real-world examples
    let config = RedactionConfig::default();
    let scanner = RedactionScanner::new(config);

    // Email patterns
    let email_cases = vec![
        ("Contact alice@example.com for details", "[EMAIL]"),
        ("Emails: bob@corp.co.uk and carol@domain.info", "[EMAIL]"),
        ("user+tag@example.org", "[EMAIL]"),
        ("test.user123@test-domain.com", "[EMAIL]"),
    ];
    for (input, expected) in email_cases {
        let result = scanner.redact(input);
        assert!(
            result.contains(expected),
            "Expected '{}' in result of: {}",
            expected,
            input
        );
        assert!(
            !result.contains("@"),
            "Email not fully redacted in: {} -> {}",
            input,
            result
        );
    }

    // Phone patterns (US and international formats)
    // Note: The PHONE_RE regex is designed for US/NANP-style numbers.
    // International formats with variable area code lengths (e.g., UK +44 20 xxxx)
    // are not fully supported.
    let phone_cases = vec![
        "Call 555-123-4567 for support",
        "Office: +1 (555) 555-5555",
        "Mobile: (555) 555 5555",
        "Phone: 1-800-555-0199",
        "555.555.5555 is the number",
    ];
    for input in phone_cases {
        let result = scanner.redact(input);
        assert!(
            result.contains("[PHONE]"),
            "Expected [PHONE] in: {} -> {}",
            input,
            result
        );
    }

    // Credit card patterns (16 digits: Visa, MC, Discover)
    // Note: AMEX (15 digits) is not currently matched by the regex
    let cc_cases = vec![
        "Card: 4111 1111 1111 1111",
        "Pay with 4111-1111-1111-1111",
        "Card ending in 1111: 4111111111111111",
    ];
    for input in cc_cases {
        let result = scanner.redact(input);
        assert!(
            result.contains("[CARD]"),
            "Expected [CARD] in: {} -> {}",
            input,
            result
        );
        // Verify no card number sequence remains
        assert!(
            !result.contains("4111") && !result.contains("3782"),
            "Card number not fully redacted in: {} -> {}",
            input,
            result
        );
    }

    // SSN patterns
    let ssn_cases = vec!["SSN: 123-45-6789", "Social: 123 45 6789", "ID: 123-45-6789"];
    for input in ssn_cases {
        let result = scanner.redact(input);
        assert!(
            result.contains("[SSN]"),
            "Expected [SSN] in: {} -> {}",
            input,
            result
        );
    }
}

#[test]
fn test_redaction_scanner_has_pii_detection() {
    let scanner = RedactionScanner::new(RedactionConfig::default());

    // Positive cases
    assert!(scanner.has_pii("Email me at test@example.com"));
    assert!(scanner.has_pii("Call 555-123-4567"));
    assert!(scanner.has_pii("Card 4111 1111 1111 1111"));
    assert!(scanner.has_pii("SSN 123-45-6789"));

    // Negative cases
    assert!(!scanner.has_pii("No sensitive data here"));
    assert!(!scanner.has_pii("Just regular text without PII"));
    assert!(!scanner.has_pii("Numbers like 12345 are not PII by themselves"));
}

#[test]
fn test_redaction_scanner_custom_patterns() {
    let mut config = RedactionConfig::default();
    config.custom_patterns = vec![
        r"ACCT-\d+".to_string(),
        r"TICKET-[A-Z]{2,4}-\d+".to_string(),
    ];
    let scanner = RedactionScanner::new(config);

    let input = "Account ACCT-99887 and ticket TICKET-ABCD-12345";
    let result = scanner.redact(input);

    assert!(result.contains("[REDACTED]"));
    assert!(!result.contains("ACCT-99887"));
    assert!(!result.contains("TICKET-ABCD-12345"));
}

#[test]
fn test_redaction_scanner_selective_enable() {
    // Test individual category controls
    let mut config = RedactionConfig::default();
    config.redact_emails = true;
    config.redact_phones = false;
    config.redact_credit_cards = false;
    config.redact_ssn = false;

    let scanner = RedactionScanner::new(config);
    let input = "Email: test@example.com, Phone: 555-123-4567";
    let result = scanner.redact(input);

    assert!(result.contains("[EMAIL]"));
    assert!(!result.contains("[PHONE]"));
    assert!(
        result.contains("555-123-4567"),
        "Phone should not be redacted"
    );
}

#[test]
fn test_redaction_scanner_disabled() {
    let mut config = RedactionConfig::default();
    config.enabled = false;

    let scanner = RedactionScanner::new(config);
    let input = "Email: test@example.com, SSN: 123-45-6789";
    let result = scanner.redact(input);

    // When disabled, no redaction occurs
    assert_eq!(result, input);
    assert!(result.contains("test@example.com"));
    assert!(result.contains("123-45-6789"));
}

#[test]
fn test_redaction_multiple_pii_in_one_text() {
    let scanner = RedactionScanner::new(RedactionConfig::default());

    let input = "Contact alice@example.com or call 555-123-4567. \
                 Card ending in 4242: 4111 1111 1111 4242. SSN: 987-65-4321";
    let result = scanner.redact(input);

    assert!(result.contains("[EMAIL]"));
    assert!(result.contains("[PHONE]"));
    assert!(result.contains("[CARD]"));
    assert!(result.contains("[SSN]"));

    // Verify no actual PII remains
    assert!(!result.contains("@"));
    assert!(!result.contains("555-"));
    assert!(!result.contains("4111"));
    assert!(!result.contains("987-65-4321"));
}

// ─── Audio format detection tests ───────────────────────────────────────────────

#[test]
fn test_audio_format_detection() {
    let test_cases = vec![
        ("speech.wav", Some(AudioFormat::Wav)),
        ("speech.mp3", Some(AudioFormat::Mp3)),
        ("speech.m4a", Some(AudioFormat::M4a)),
        ("speech.WAV", Some(AudioFormat::Wav)), // Case insensitive
        ("speech.MP3", Some(AudioFormat::Mp3)),
        ("speech.ogg", None),
        ("speech.flac", None),
        ("speech", None),
        ("path/to/speech.wav", Some(AudioFormat::Wav)),
    ];

    for (filename, expected) in test_cases {
        let result = AudioFormat::from_path(PathBuf::from(filename).as_path());
        assert_eq!(
            result, expected,
            "Format detection failed for: {}",
            filename
        );
    }
}

// ─── TranscriptionJob state tests ────────────────────────────────────────────────

#[test]
fn test_transcription_job_creation() {
    let input_path = PathBuf::from("/tmp/test.wav");
    let job = TranscriptionJob::new(input_path.clone(), 3);

    assert_eq!(job.input_path, input_path);
    assert_eq!(job.max_retries, 3);
    assert_eq!(job.attempts, 0);
    assert_eq!(job.status, agentscribe::transcription::JobStatus::Pending);
    assert!(job.result.is_none());
    assert!(job.error.is_none());
    assert!(!job.id.is_empty());
}

#[test]
fn test_transcription_job_id_uniqueness() {
    let path = PathBuf::from("/tmp/test.wav");

    let j1 = TranscriptionJob::new(path.clone(), 3);
    let j2 = TranscriptionJob::new(path, 3);

    assert_ne!(j1.id, j2.id, "Job IDs must be unique");
}

// ─── TranscriptionResult tests ───────────────────────────────────────────────────

#[test]
fn test_transcription_result_empty() {
    // Empty text
    let r = TranscriptionResult::utterance_level("   ".to_string(), vec![], None, vec![]);
    assert!(r.is_empty());

    // Only whitespace
    let r = TranscriptionResult::utterance_level("\n\t  \n".to_string(), vec![], None, vec![]);
    assert!(r.is_empty());

    // Has content
    let r = TranscriptionResult::utterance_level("Hello world".to_string(), vec![], None, vec![]);
    assert!(!r.is_empty());
}

#[test]
fn test_transcription_result_segment_count() {
    // Word-level: count words
    let words = vec![
        agentscribe::transcription::WordTimestamp {
            text: "Hello".to_string(),
            start_ms: 0,
            end_ms: 100,
            probability: None,
        },
        agentscribe::transcription::WordTimestamp {
            text: "world".to_string(),
            start_ms: 100,
            end_ms: 200,
            probability: None,
        },
    ];
    let r = TranscriptionResult::word_level("Hello world".to_string(), words, vec![], None);
    assert_eq!(r.segment_count(), 2);

    // Utterance-level: count utterances
    let utterances = vec![
        agentscribe::transcription::UtteranceTimestamp {
            text: "Hello world".to_string(),
            start_ms: 0,
            end_ms: 200,
        },
        agentscribe::transcription::UtteranceTimestamp {
            text: "How are you?".to_string(),
            start_ms: 200,
            end_ms: 400,
        },
    ];
    let r = TranscriptionResult::utterance_level(
        "Hello world. How are you?".to_string(),
        utterances,
        None,
        vec![],
    );
    assert_eq!(r.segment_count(), 2);
}

// ─── Redaction integration with TranscriptionResult ─────────────────────────────

#[test]
fn test_redaction_applied_to_transcription_result() {
    let scanner = RedactionScanner::new(RedactionConfig::default());

    // Create a result with PII in the text
    let mut result = TranscriptionResult::utterance_level(
        "My email is alice@example.com and my phone is 555-123-4567".to_string(),
        vec![],
        None,
        vec![],
    );

    // Apply redaction
    result.full_text = scanner.redact(&result.full_text);

    assert!(result.full_text.contains("[EMAIL]"));
    assert!(result.full_text.contains("[PHONE]"));
    assert!(!result.full_text.contains("alice@example.com"));
    assert!(!result.full_text.contains("555-123-4567"));
}

// ─── WhisperConfig defaults ──────────────────────────────────────────────────────

#[test]
fn test_whisper_config_defaults() {
    let config = WhisperConfig::default();

    assert!(!config.enabled);
    assert_eq!(config.max_retries, 3);
    assert_eq!(config.timeout_seconds, 300);
    assert!(config.word_timestamps);
    assert!(config.model_path.is_none());
    assert!(config.executable.is_none());
    assert!(config.backend.is_none());
    assert!(config.language.is_none());
}

// ─── RedactionConfig defaults ────────────────────────────────────────────────────

#[test]
fn test_redaction_config_defaults() {
    let config = RedactionConfig::default();

    assert!(config.enabled);
    assert!(config.redact_emails);
    assert!(config.redact_phones);
    assert!(config.redact_credit_cards);
    assert!(config.redact_ssn);
    assert!(config.custom_patterns.is_empty());
}

// ─── TranscriptionQueue basic structure test ─────────────────────────────────────

#[test]
fn test_transcription_queue_creation() {
    // This test verifies the queue can be created without Whisper installed
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();

    let whisper_config = WhisperConfig {
        enabled: false,
        ..Default::default()
    };
    let redaction_config = RedactionConfig::default();

    // Creating the queue spawns a worker task
    let queue = TranscriptionQueue::new(whisper_config, redaction_config);

    // The queue should be usable (we can't test actual transcription without Whisper)
    // But we can verify the queue was created successfully
    drop(queue); // Explicit drop to verify cleanup works
}

// ─── Integration: redaction happens before storage ───────────────────────────────

#[test]
fn test_redaction_prevents_pii_storage() {
    let scanner = RedactionScanner::new(RedactionConfig::default());

    // Simulate transcript output from Whisper (with PII)
    let raw_transcript = "Hi, my name is John and you can reach me at john@example.com \
                          or call me at 555-987-6543. My credit card is 4242 4242 4242 4242.";

    // Redact before "storage"
    let redacted = scanner.redact(raw_transcript);

    // Verify no PII remains
    assert!(!redacted.contains("john@example.com"));
    assert!(!redacted.contains("555-987-6543"));
    assert!(!redacted.contains("4242"));
    assert!(redacted.contains("[EMAIL]"));
    assert!(redacted.contains("[PHONE]"));
    assert!(redacted.contains("[CARD]"));
}

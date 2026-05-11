# Bead bf-4u2d — RedactionScanner PII Pattern Tests

## Summary

The task requested adding tests for `src/redaction.rs`. Upon investigation, all required
tests were already present — added in commit `089cea0 test(redaction): add comprehensive
PII pattern tests`.

## Tests verified (18 total, all passing)

| Test | Coverage |
|------|----------|
| `test_email_redaction` | Positive match: email addresses |
| `test_ssn_redaction` | Positive match: SSN `###-##-####` format |
| `test_phone_redaction` | Positive match: US/international phone formats |
| `test_credit_card_redaction` | Positive match: grouped 16-digit card |
| `test_no_redaction_when_disabled` | Disabled scanner passes through unchanged |
| `test_has_pii_detection` | `has_pii()` method |
| `test_custom_pattern` | Custom regex via `RedactionConfig` |
| `test_invalid_custom_pattern_ignored` | Invalid pattern is skipped, no panic |
| `test_phone_re_does_not_match_inside_credit_card` | PHONE_RE `\b` regression (f0c6efa) |
| `test_credit_card_does_not_match_short_numbers` | Negative: < 16 digits |
| `test_credit_card_does_not_match_mid_sequence` | Negative: > 19 digits |
| `test_credit_card_variations` | 16-digit dash-delimited, 19-digit plain |
| `test_ssn_does_not_match_invalid_formats` | Negative: wrong grouping |
| `test_email_does_not_match_invalid_formats` | Negative: missing TLD, missing `@` |
| `test_phone_does_not_match_short_numbers` | Negative: 6-digit codes |
| `test_multiple_pii_types_in_one_text` | All four types in one pass |
| `test_selective_redaction_phones_only` | Per-type enable/disable |
| `test_selective_redaction_emails_only` | Per-type enable/disable |

All 18 tests ran and passed: `cargo test --lib redaction`

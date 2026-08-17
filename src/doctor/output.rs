//! Structured output types for the doctor command
//!
//! This module provides the core data structures for representing doctor check results
//! and overall health status, with support for both human-readable (Display) and
//! machine-readable (Serialize/JSON) output formats.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Status of a doctor check or overall health
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    /// Check passed or system is healthy
    Pass,
    /// Check failed or system has issues
    Fail,
}

impl fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckStatus::Pass => write!(f, "PASS"),
            CheckStatus::Fail => write!(f, "FAIL"),
        }
    }
}

/// Result of a single doctor check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorCheckResult {
    /// Name of the check (e.g., "daemon_alive", "state_file_parses")
    pub name: String,
    /// Status of the check
    pub status: CheckStatus,
    /// Optional message with details about the check
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl DoctorCheckResult {
    /// Create a new doctor check result
    pub fn new(name: impl Into<String>, status: CheckStatus) -> Self {
        DoctorCheckResult {
            name: name.into(),
            status,
            message: None,
        }
    }

    /// Create a passing check result
    pub fn pass(name: impl Into<String>) -> Self {
        Self::new(name, CheckStatus::Pass)
    }

    /// Create a failing check result
    pub fn fail(name: impl Into<String>) -> Self {
        Self::new(name, CheckStatus::Fail)
    }

    /// Set the message for this check result
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

impl fmt::Display for DoctorCheckResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status_symbol = match self.status {
            CheckStatus::Pass => "✓",
            CheckStatus::Fail => "✗",
        };
        write!(f, "{} {}", status_symbol, self.name)?;
        if let Some(ref message) = self.message {
            write!(f, ": {}", message)?;
        }
        Ok(())
    }
}

/// Overall output from the doctor command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorOutput {
    /// Individual check results
    pub checks: Vec<DoctorCheckResult>,
    /// Overall health status across all checks
    pub overall_status: CheckStatus,
}

impl DoctorOutput {
    /// Create a new doctor output with no checks
    pub fn new() -> Self {
        DoctorOutput {
            checks: Vec::new(),
            overall_status: CheckStatus::Pass,
        }
    }

    /// Add a check result to the output
    pub fn add_check(&mut self, check: DoctorCheckResult) {
        // Update overall status if any check fails
        if check.status == CheckStatus::Fail && self.overall_status == CheckStatus::Pass {
            self.overall_status = CheckStatus::Fail;
        }
        self.checks.push(check);
    }

    /// Get the number of passing checks
    pub fn pass_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.status == CheckStatus::Pass)
            .count()
    }

    /// Get the number of failing checks
    pub fn fail_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.status == CheckStatus::Fail)
            .count()
    }

    /// Get the total number of checks
    pub fn total_count(&self) -> usize {
        self.checks.len()
    }
}

impl Default for DoctorOutput {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for DoctorOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status_symbol = match self.overall_status {
            CheckStatus::Pass => "✓",
            CheckStatus::Fail => "✗",
        };

        writeln!(f, "Doctor Diagnostic Results")?;
        writeln!(
            f,
            "Overall Status: {} {}",
            status_symbol, self.overall_status
        )?;
        writeln!(f)?;

        for check in &self.checks {
            writeln!(f, "  {}", check)?;
        }

        writeln!(f)?;
        writeln!(
            f,
            "Summary: {}/{} checks passed",
            self.pass_count(),
            self.total_count()
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_status_display() {
        assert_eq!(format!("{}", CheckStatus::Pass), "PASS");
        assert_eq!(format!("{}", CheckStatus::Fail), "FAIL");
    }

    #[test]
    fn test_doctor_check_result_new() {
        let check = DoctorCheckResult::new("test_check", CheckStatus::Pass);
        assert_eq!(check.name, "test_check");
        assert_eq!(check.status, CheckStatus::Pass);
        assert!(check.message.is_none());
    }

    #[test]
    fn test_doctor_check_result_pass() {
        let check = DoctorCheckResult::pass("test_check");
        assert_eq!(check.name, "test_check");
        assert_eq!(check.status, CheckStatus::Pass);
    }

    #[test]
    fn test_doctor_check_result_fail() {
        let check = DoctorCheckResult::fail("test_check");
        assert_eq!(check.name, "test_check");
        assert_eq!(check.status, CheckStatus::Fail);
    }

    #[test]
    fn test_doctor_check_result_with_message() {
        let check = DoctorCheckResult::pass("test_check").with_message("All good");
        assert_eq!(check.message, Some("All good".to_string()));
    }

    #[test]
    fn test_doctor_check_result_display() {
        let check = DoctorCheckResult::pass("test_check");
        assert_eq!(format!("{}", check), "✓ test_check");

        let check_with_msg = DoctorCheckResult::fail("test_check").with_message("Error found");
        assert_eq!(format!("{}", check_with_msg), "✗ test_check: Error found");
    }

    #[test]
    fn test_doctor_output_new() {
        let output = DoctorOutput::new();
        assert!(output.checks.is_empty());
        assert_eq!(output.overall_status, CheckStatus::Pass);
        assert_eq!(output.pass_count(), 0);
        assert_eq!(output.fail_count(), 0);
        assert_eq!(output.total_count(), 0);
    }

    #[test]
    fn test_doctor_output_add_check_pass() {
        let mut output = DoctorOutput::new();
        output.add_check(DoctorCheckResult::pass("check1"));
        assert_eq!(output.pass_count(), 1);
        assert_eq!(output.fail_count(), 0);
        assert_eq!(output.overall_status, CheckStatus::Pass);
    }

    #[test]
    fn test_doctor_output_add_check_fail() {
        let mut output = DoctorOutput::new();
        output.add_check(DoctorCheckResult::fail("check1"));
        assert_eq!(output.pass_count(), 0);
        assert_eq!(output.fail_count(), 1);
        assert_eq!(output.overall_status, CheckStatus::Fail);
    }

    #[test]
    fn test_doctor_output_multiple_checks() {
        let mut output = DoctorOutput::new();
        output.add_check(DoctorCheckResult::pass("check1"));
        output.add_check(DoctorCheckResult::pass("check2"));
        output.add_check(DoctorCheckResult::fail("check3"));
        assert_eq!(output.pass_count(), 2);
        assert_eq!(output.fail_count(), 1);
        assert_eq!(output.total_count(), 3);
        assert_eq!(output.overall_status, CheckStatus::Fail);
    }

    #[test]
    fn test_doctor_output_display() {
        let mut output = DoctorOutput::new();
        output.add_check(DoctorCheckResult::pass("check1"));
        output.add_check(DoctorCheckResult::fail("check2").with_message("Failed"));

        let display = format!("{}", output);
        assert!(display.contains("Doctor Diagnostic Results"));
        assert!(display.contains("Overall Status: ✗ FAIL"));
        assert!(display.contains("✓ check1"));
        assert!(display.contains("✗ check2: Failed"));
        assert!(display.contains("Summary: 1/2 checks passed"));
    }

    #[test]
    fn test_doctor_check_result_serialize() {
        let check = DoctorCheckResult::pass("test_check").with_message("All good");
        let json = serde_json::to_string(&check).unwrap();
        assert!(json.contains("\"name\":\"test_check\""));
        assert!(json.contains("\"status\":\"pass\""));
        assert!(json.contains("\"message\":\"All good\""));
    }

    #[test]
    fn test_doctor_check_result_serialize_no_message() {
        let check = DoctorCheckResult::fail("test_check");
        let json = serde_json::to_string(&check).unwrap();
        assert!(json.contains("\"name\":\"test_check\""));
        assert!(json.contains("\"status\":\"fail\""));
        // message should not be in output when None
        assert!(!json.contains("message"));
    }

    #[test]
    fn test_doctor_output_serialize() {
        let mut output = DoctorOutput::new();
        output.add_check(DoctorCheckResult::pass("check1"));
        output.add_check(DoctorCheckResult::fail("check2"));

        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("\"overall_status\":\"fail\""));
        assert!(json.contains("\"name\":\"check1\""));
        assert!(json.contains("\"name\":\"check2\""));
    }

    #[test]
    fn test_check_status_serialize() {
        assert_eq!(
            serde_json::to_string(&CheckStatus::Pass).unwrap(),
            "\"pass\""
        );
        assert_eq!(
            serde_json::to_string(&CheckStatus::Fail).unwrap(),
            "\"fail\""
        );
    }

    #[test]
    fn test_check_status_deserialize() {
        let pass: CheckStatus = serde_json::from_str("\"pass\"").unwrap();
        assert_eq!(pass, CheckStatus::Pass);

        let fail: CheckStatus = serde_json::from_str("\"fail\"").unwrap();
        assert_eq!(fail, CheckStatus::Fail);
    }

    #[test]
    fn test_doctor_output_default() {
        let output = DoctorOutput::default();
        assert!(output.checks.is_empty());
        assert_eq!(output.overall_status, CheckStatus::Pass);
    }
}

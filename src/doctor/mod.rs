//! Doctor command for self-diagnosis of AgentScribe failure modes
//!
//! This module provides health checking capabilities for the AgentScribe system,
//! including structured output types for check results and overall health status.

pub mod implementation;
pub mod output;

// Re-export the main types for convenience
pub use implementation::{run_doctor, CheckResult};
pub use output::{CheckStatus, DoctorCheckResult, DoctorOutput};

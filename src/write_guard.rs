//! Zero-write invariant enforcement for Phase 1.
//!
//! Phase 1 (scraper, parser) is strictly read-only. The beads CLI (`br`) write
//! verbs — `create`, `close`, `update`, `release` — must never be reachable from
//! phase-1 code paths.
//!
//! ## Enforcement layers
//!
//! 1. **Compile-time** (`zero-write-v01` feature): write-verb helper functions
//!    (`br_create`, `br_close`, `br_update`, `br_release`) are absent. Any call
//!    site fails with "cannot find function" — run
//!    `cargo check --features=zero-write-v01` in CI.
//! 2. **Runtime** (`assert_no_br_write`): panics immediately if a write verb is
//!    attempted — belt-and-suspenders in case the compile-time guard is bypassed.
//!
//! ## Correct usage
//!
//! All `br` interactions must go through [`br_exec`]. The write-verb helpers are
//! convenience wrappers that exist only outside `zero-write-v01`.

use std::process::Output;

use anyhow::Result;

/// All br subcommands that mutate bead state.
///
/// Kept as a `const` so the runtime guard and tests share a single source of truth.
pub const WRITE_VERBS: &[&str] = &["create", "close", "update", "release"];

/// Assert that `subcommand` is not a br write verb.
///
/// Panics immediately with a clear message if the Phase 1 zero-write invariant
/// would be violated. This is the runtime (belt-and-suspenders) guard; the
/// compile-time guard is the `zero-write-v01` feature flag.
///
/// # Panics
///
/// Panics if `subcommand` is one of `create`, `close`, `update`, or `release`.
pub fn assert_no_br_write(subcommand: &str) {
    if WRITE_VERBS.contains(&subcommand) {
        panic!(
            "Zero-write invariant violated: `br {}` is a write verb. \
             Phase 1 code paths (scraper, parser) must be strictly read-only. \
             Use `cargo check --features=zero-write-v01` to enforce this at compile time. \
             See §3.8 and §6 of the implementation plan.",
            subcommand
        );
    }
}

/// Execute a read-only `br` subcommand.
///
/// Applies the runtime zero-write assertion before spawning the process.
/// Under `zero-write-v01` this is the **only** callable path — write-verb
/// helpers are absent, so passing a write verb here will panic.
///
/// # Errors
///
/// Returns an error if the `br` process cannot be spawned.
pub fn br_exec(subcommand: &str, args: &[&str]) -> Result<Output> {
    assert_no_br_write(subcommand);
    Ok(std::process::Command::new("br")
        .arg(subcommand)
        .args(args)
        .output()?)
}

// ── Write-verb helpers ────────────────────────────────────────────────────────
//
// These functions exist ONLY when `zero-write-v01` is NOT active.
// Under the feature flag they are absent — any call site in phase-1 code
// will fail to compile with "cannot find function in module `write_guard`".
// This is the compile-time half of the zero-write invariant.

/// Create a new bead. **Not available under `zero-write-v01`.**
#[cfg(not(feature = "zero-write-v01"))]
pub fn br_create(args: &[&str]) -> Result<Output> {
    br_exec("create", args)
}

/// Close a bead. **Not available under `zero-write-v01`.**
#[cfg(not(feature = "zero-write-v01"))]
pub fn br_close(args: &[&str]) -> Result<Output> {
    br_exec("close", args)
}

/// Update a bead. **Not available under `zero-write-v01`.**
#[cfg(not(feature = "zero-write-v01"))]
pub fn br_update(args: &[&str]) -> Result<Output> {
    br_exec("update", args)
}

/// Release a bead. **Not available under `zero-write-v01`.**
#[cfg(not(feature = "zero-write-v01"))]
pub fn br_release(args: &[&str]) -> Result<Output> {
    br_exec("release", args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_verbs_list_is_exhaustive() {
        for verb in &["create", "close", "update", "release"] {
            assert!(
                WRITE_VERBS.contains(verb),
                "WRITE_VERBS is missing write verb: {verb}"
            );
        }
        assert_eq!(
            WRITE_VERBS.len(),
            4,
            "unexpected entry count in WRITE_VERBS"
        );
    }

    #[test]
    #[should_panic(expected = "Zero-write invariant violated")]
    fn runtime_guard_panics_on_create() {
        assert_no_br_write("create");
    }

    #[test]
    #[should_panic(expected = "Zero-write invariant violated")]
    fn runtime_guard_panics_on_close() {
        assert_no_br_write("close");
    }

    #[test]
    #[should_panic(expected = "Zero-write invariant violated")]
    fn runtime_guard_panics_on_update() {
        assert_no_br_write("update");
    }

    #[test]
    #[should_panic(expected = "Zero-write invariant violated")]
    fn runtime_guard_panics_on_release() {
        assert_no_br_write("release");
    }

    #[test]
    fn runtime_guard_allows_read_only_verbs() {
        // None of these should panic.
        for verb in &["list", "show", "search", "status", "sync", "doctor", "log"] {
            assert_no_br_write(verb);
        }
    }
}

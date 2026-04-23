//! CI tests for the Phase 1 zero-write invariant (§3.8, §6).
//!
//! These tests enforce that phase-1 code paths (scraper, parser) never invoke
//! the beads CLI (`br`) write verbs: `create`, `close`, `update`, `release`.
//!
//! ## Enforcement layers covered here
//!
//! - **Runtime guard**: verify `assert_no_br_write` panics for every write verb
//!   and is silent for read-only verbs.
//! - **Static analysis**: scan phase-1 source files for write-verb call patterns
//!   that could bypass the `write_guard` module (e.g. raw `Command::new("br")`
//!   paired with a write verb arg).
//!
//! The compile-time layer is verified separately by running
//! `cargo check --features=zero-write-v01` in CI.

use std::fs;
use std::path::PathBuf;

use agentscribe::write_guard;
use walkdir::WalkDir;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Collect all `.rs` files under `src/<rel_dir>` with their contents.
fn rs_files_in(rel_dir: &str) -> Vec<(PathBuf, String)> {
    let dir = workspace_root().join("src").join(rel_dir);
    let mut files = Vec::new();
    for entry in WalkDir::new(&dir).into_iter().flatten() {
        if entry.path().extension().map(|e| e == "rs").unwrap_or(false) {
            let content = fs::read_to_string(entry.path())
                .unwrap_or_else(|_| panic!("failed to read {:?}", entry.path()));
            files.push((entry.path().to_path_buf(), content));
        }
    }
    files
}

/// Return a description of the first write-verb call pattern found in `content`,
/// or `None` if the content is clean.
///
/// Detects two patterns:
/// 1. String literals: `"br create"`, `"br close"`, etc.
/// 2. Chained calls: `Command::new("br")` combined with `.arg("create")` etc.
fn find_write_verb_pattern(content: &str) -> Option<String> {
    // Pattern 1: string literal containing write verb
    for verb in write_guard::WRITE_VERBS {
        let literal = format!("\"br {verb}\"");
        if content.contains(&literal) {
            return Some(format!("string literal {literal:?}"));
        }
    }

    // Pattern 2: Command::new("br") and a write-verb .arg(...)
    if content.contains(r#"Command::new("br")"#) {
        for verb in write_guard::WRITE_VERBS {
            let arg_pat = format!(".arg(\"{verb}\")");
            if content.contains(&arg_pat) {
                return Some(format!(
                    r#"Command::new("br") combined with .arg("{verb}")"#
                ));
            }
        }
    }

    None
}

// ── Static analysis tests ─────────────────────────────────────────────────────

#[test]
fn phase1_scraper_no_write_verbs() {
    for (path, content) in rs_files_in("scraper") {
        if let Some(reason) = find_write_verb_pattern(&content) {
            panic!(
                "Zero-write invariant violated in phase-1 scraper code!\n\
                 File   : {path:?}\n\
                 Pattern: {reason}\n\n\
                 Phase 1 (scraper, parser) must not call br write verbs \
                 (create/close/update/release). Route all br interactions \
                 through `write_guard::br_exec` and enable \
                 `--features=zero-write-v01` in CI. See §3.8 and §6.",
            );
        }
    }
}

#[test]
fn phase1_parser_no_write_verbs() {
    for (path, content) in rs_files_in("parser") {
        if let Some(reason) = find_write_verb_pattern(&content) {
            panic!(
                "Zero-write invariant violated in phase-1 parser code!\n\
                 File   : {path:?}\n\
                 Pattern: {reason}\n\n\
                 Phase 1 (scraper, parser) must not call br write verbs \
                 (create/close/update/release). Route all br interactions \
                 through `write_guard::br_exec` and enable \
                 `--features=zero-write-v01` in CI. See §3.8 and §6.",
            );
        }
    }
}

// ── Runtime guard tests ───────────────────────────────────────────────────────

#[test]
fn write_guard_panics_for_every_write_verb() {
    for &verb in write_guard::WRITE_VERBS {
        let result = std::panic::catch_unwind(|| {
            write_guard::assert_no_br_write(verb);
        });
        assert!(
            result.is_err(),
            "Expected assert_no_br_write({verb:?}) to panic, but it returned Ok"
        );

        // Verify the panic message contains the expected sentinel so callers
        // get a meaningful error rather than an opaque panic.
        let err = result.unwrap_err();
        let msg = err
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| err.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(
            msg.contains("Zero-write invariant violated"),
            "Panic message for verb {verb:?} did not contain sentinel text. Got: {msg:?}"
        );
    }
}

#[test]
fn write_guard_silent_for_read_only_verbs() {
    // None of these should panic — they are all read-only br subcommands.
    for verb in &["list", "show", "search", "status", "sync", "doctor", "log"] {
        write_guard::assert_no_br_write(verb);
    }
}

/// Smoke-test that WRITE_VERBS covers exactly the four known write verbs.
#[test]
fn write_verbs_constant_is_canonical() {
    let expected = ["create", "close", "update", "release"];
    for verb in &expected {
        assert!(
            write_guard::WRITE_VERBS.contains(verb),
            "WRITE_VERBS is missing: {verb}"
        );
    }
    assert_eq!(
        write_guard::WRITE_VERBS.len(),
        expected.len(),
        "WRITE_VERBS has unexpected entries"
    );
}

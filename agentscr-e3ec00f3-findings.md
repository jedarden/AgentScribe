# Search Results: 'coding/AgentScribe' Occurrences

**Pattern Searched:** `coding/AgentScribe`  
**Search Date:** 2026-08-21  
**Search Scope:** All `*.toml`, `*.md`, `*.rs` files in /home/coding/AgentScribe

## Executive Summary

**Finding:** The pattern `'coding/AgentScribe'` does **NOT** appear in this codebase as a repository URL.

All occurrences of `'coding/AgentScribe'` in this codebase are **absolute file paths** to the local repository at `/home/coding/AgentScribe/`. These are NOT repository URLs.

## Actual Finding: Placeholder URL in examples/README.md

**File:** `examples/README.md`  
**Line:** 1  
**Issue:** Contains placeholder URL `https://github.com/your-org/agentscribe`  
**Correct URL:** `https://github.com/jedarden/AgentScribe` (GitHub mirror) or `https://git.ardenone.com/jedarden/AgentScribe` (Forgejo source)

**Current text:**
```markdown
1. Fork the [AgentScribe repository](https://github.com/your-org/agentscribe).
```

**Should be:**
```markdown
1. Fork the [AgentScribe repository](https://github.com/jedarden/AgentScribe).
```

## Repository URL Analysis

All actual repository URLs in the codebase are **correct**:

| File | Line | URL | Status |
|------|------|-----|--------|
| `Cargo.toml` | - | `https://github.com/jedarden/AgentScribe` | ✅ Correct |
| `README.md` | - | `https://github.com/jedarden/AgentScribe.git` | ✅ Correct |
| `CHANGELOG.md` | - | `https://github.com/jedarden/AgentScribe/...` | ✅ Correct |

**Repository Owner:** `jedarden` (not `coding`)  
**Forgejo URL:** `git.ardenone.com/jedarden/AgentScribe` (source of truth)  
**GitHub URL:** `github.com/jedarden/AgentScribe` (read-only mirror)

## False Positives: File Path References

All other matches are legitimate file path references to the local repository at `/home/coding/AgentScribe/`:

| File | Example Match | Context |
|------|--------------|---------|
| `TEST_DEPENDENCIES_SCAN.md` | `**Workspace:** /home/coding/AgentScribe` | Project metadata |
| `cli_parsing_entry_point.md` | `**Main entry file:** /home/coding/AgentScribe/src/main.rs` | File path reference |
| `docs/test-imports-analysis.md` | 24 occurrences of `/home/coding/AgentScribe/tests/` | Test import analysis |
| `extract_imports.rs` | 58 occurrences of `/home/coding/AgentScribe/` | Import extraction script |

**Total file path occurrences:** 300+ across documentation, test files, and build scripts

## Conclusion

- **❌ Wrong URL:** `coding/AgentScribe` - Does not exist in this codebase
- **✅ Correct URL:** `jedarden/AgentScribe` (Forgejo: `git.ardenone.com/jedarden/AgentScribe`)
- **🔧 Action Required:** Fix placeholder URL in `examples/README.md` (line 1)

**No changes required** for the `'coding/AgentScribe'` pattern, as it does not appear as a repository URL in this codebase. All matches are absolute file paths to the local repository, which is the correct and expected usage.

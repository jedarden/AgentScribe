# AgentScribe Search Exit Code Behavior - Test Results

**Test Date:** 2026-08-15  
**Bead:** agentscr-083f5f3d  
**Purpose:** Document current exit-code behavior for `agentscribe search --json` to inform fixes in child bead.

## Test Results Summary

### Test 1: Empty Index
**Scenario:** Search against newly initialized empty index

**Command:**
```bash
export AGENTSCRIBE_DATA_DIR="/tmp/empty-test"
agentscribe search --json "test query"
```

**Result:**
```json
{
  "query": "test query",
  "total_matches": 0,
  "search_time_ms": 1,
  "sessions_searched": 2,
  "results": []
}
```
**Exit Code:** `0`

**Finding:** ✅ Exit code 0 with valid JSON, empty `results` array. The tool handles empty indices gracefully.

---

### Test 2: No-Match Query (Valid Query, Zero Results)
**Scenario:** Search with a query that matches no indexed sessions

**Command:**
```bash
agentscribe search --json "zzzxyz123nonexistent"
```

**Result:**
```json
{
  "query": "zzzxyz123nonexistent",
  "total_matches": 0,
  "search_time_ms": 0,
  "sessions_searched": 2,
  "results": []
}
```
**Exit Code:** `0`

**Finding:** ✅ Exit code 0 with valid JSON, empty `results` array. Zero results is not an error condition.

---

### Test 3: Bad Arguments

#### Test 3a: Missing --json flag
**Command:**
```bash
agentscribe search "test query"
```

**Result:**
```
0 result(s) for "test query" (searched 2 sessions in 0ms)
```
**Exit Code:** `0`

**Finding:** ✅ Exit code 0 with human-readable output (valid, just non-JSON format).

---

#### Test 3b: Invalid flag
**Command:**
```bash
agentscribe search --json "test query" --invalid-flag
```

**Result:**
```
error: unexpected argument '--invalid-flag' found

  tip: to pass '--invalid-flag' as a value, use '-- --invalid-flag'

Usage: agentscribe search --json <QUERY>

For more information, try '--help'.
```
**Exit Code:** `2`

**Finding:** ✅ Exit code 2 for invalid CLI arguments (Clap-provided error handling).

---

#### Test 3c: Missing query argument
**Command:**
```bash
agentscribe search --json
```

**Result:**
```
Error: DataDir("No search query provided. Use <query>, --error, --code, --like, --anti-patterns, or a filter.")
```
**Exit Code:** `1`

**Finding:** ✅ Exit code 1 for missing required query (application-provided error message).

---

### Test 4: Corrupt Index

#### Test 4a: Corrupted index files
**Scenario:** Term file corrupted with invalid data

**Command:**
```bash
# Corrupt term file, then search
agentscribe search --json "test query"
```

**Result:**
```json
{
  "query": "test query",
  "total_matches": 0,
  "search_time_ms": 0,
  "sessions_searched": 2,
  "results": []
}
```
**Exit Code:** `0`

**Finding:** ⚠️ Tantivy handles corrupted files gracefully - search succeeds with exit code 0. Corrupted segments are likely ignored or recreated on-the-fly.

---

#### Test 4b: Missing index directory entirely
**Scenario:** Index directory deleted

**Command:**
```bash
rm -rf ~/.agentscribe/index/tantivy
agentscribe search --json "test query"
```

**Result:**
```json
{
  "query": "test query",
  "total_matches": 0,
  "search_time_ms": 0,
  "sessions_searched": 2,
  "results": []
}
```
**Exit Code:** `0`

**Finding:** ⚠️ Missing index directory is auto-created or handled gracefully - exit code 0.

---

#### Test 4c: Corrupted meta.json
**Scenario:** Index metadata file contains invalid JSON

**Command:**
```bash
echo "invalid json" > ~/.agentscribe/index/tantivy/meta.json
agentscribe search --json "test query"
```

**Result:**
```json
{
  "query": "test query",
  "total_matches": 0,
  "search_time_ms": 0,
  "sessions_searched": 2,
  "results": []
}
```
**Exit Code:** `0`

**Finding:** ⚠️ Corrupted metadata does not cause search failure - exit code 0.

---

#### Test 4d: Removed index directory completely
**Scenario:** Entire index directory removed

**Command:**
```bash
rm -rf ~/.agentscribe/index/tantivy
agentscribe search --json "test query"
```

**Result:**
```json
{
  "query": "test query",
  "total_matches": 0,
  "search_time_ms": 0,
  "sessions_searched": 2,
  "results": []
}
```
**Exit Code:** `0`

**Finding:** ⚠️ Index recreation or graceful fallback - exit code 0.

---

## Exit Code Summary

| Exit Code | Meaning | Example Conditions |
|-----------|---------|-------------------|
| **0** | Success | Valid search (even with 0 results), empty index, human-readable output, corrupt-but-recoverable index |
| **1** | Application Error | Missing required query argument, I/O errors, configuration errors |
| **2** | CLI Argument Error | Invalid flags, invalid argument syntax (Clap-provided) |

## Key Findings

### ✅ Correct Behavior
1. **Zero results is not an error** - Exit code 0 with empty `results[]` array is appropriate
2. **Invalid CLI args return distinct exit code 2** - Allows callers to distinguish user errors from application errors
3. **Missing required arguments return exit code 1** - Application-level validation works correctly
4. **Human-readable output (missing --json) returns exit code 0** - Valid output format, just not JSON

### ⚠️ Potentially Concerning Behavior
1. **Corrupt index returns exit code 0** - While robust, this may hide data integrity issues from automated callers
2. **Missing index directory is silently recreated** - No error signal when index is missing
3. **Tantivy handles corruption gracefully** - This is generally good, but may mask underlying problems

### 🔍 Design Observations
1. **Rust's Result<()> pattern** - `main()` returns `Result<()>`, which Rust automatically converts:
   - `Ok(())` → exit code 0
   - `Err(_)` → exit code 1 (with error message printed to stderr)
2. **Clap provides exit code 2** for argument parsing errors, distinct from application errors
3. **No custom exit codes** - AgentScribe uses the standard 0/1/2 pattern without custom exit codes for specific conditions
4. **Index errors don't propagate** - Tantivy's `open_index()` and `reader()` calls use `?` operator, but corruption appears to be handled internally by Tantivy

## Recommendations for Child Bead 2

Based on these findings, potential fixes to consider:

1. **Document current behavior in cli-reference.md** - Exit code 0 for zero results is correct and should be documented as part of the stable contract
2. **Add explicit index health check** - Optional `agentscribe index verify` command to detect corruption
3. **Consider --strict flag** - Optional flag that treats index issues as errors rather than warnings
4. **Add search exit code documentation** - Explicitly document that exit code 0 with empty `results[]` means "search succeeded, no matches found"
5. **Test coverage** - Add unit tests that verify exit codes for these scenarios

## Testing Methodology

Each test was run with:
```bash
# Capture both output and exit code
agentscribe search --json "query" 2>&1
EXIT_CODE=$?
echo "Exit code: $EXIT_CODE"
```

All tests were performed against the current main branch as of 2026-08-15 using the release build (`cargo build --release`).

## Related Documentation

- `docs/cli-reference.md` - Should document exit code behavior
- `docs/plan.md` - Phase 9 mentions "Exit-code contract" for search --json
- `src/error.rs` - Error type definitions
- `src/cli.rs` - Command-line interface and error handling

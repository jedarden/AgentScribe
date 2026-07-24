# bf-29r1x: Document [source.envelope] reference entry

## Task Summary

Verified that the existing `[source.envelope]` documentation in `plugins/BUILDING_PLUGINS.md` (lines 90-145) accurately matches the implemented schema and parser behavior from children 1-3.

## Verification Results

### Documentation Coverage (All Required Elements Present)

✅ **Three configuration fields documented:**
- `payload_field` - Field containing the event payload object
- `type_field` - Field containing the event type for routing
- `type_routing` - Maps type values to routing actions

✅ **Three routing values and actions documented:**
- `"message" = "event"` - Parse as regular event using `[parser]` field mappings
- `"metadata" = "meta"` - Session metadata (for future use; currently skipped)
- `"heartbeat" = "skip"` - Ignore this line entirely

✅ **'^' prefix convention documented:**
- Use `^` prefix to access fields from envelope wrapper instead of payload
- Example: `timestamp = "^timestamp"` reads from wrapper, `role = "role"` reads from payload

✅ **Unknown types behavior documented:**
- Any type value not in `type_routing` defaults to `skip` with a warning

✅ **Example TOML provided:**
- Shows `payload_field`, `type_field`, and `type_routing` configuration
- Includes `^`-prefixed timestamp mapping
- Consistent with Codex-style `{timestamp, type, payload}` envelope structure

### Implementation Verification

Verified documentation claims against implementation in `src/parser/jsonl.rs` and `src/parser/mod.rs`:

1. **Envelope routing (jsonl.rs lines 128-179):** Matches documented behavior
2. **Routing actions (jsonl.rs unwrap_envelope):** event/meta/skip produce correct outputs
3. **Field extraction (mod.rs extract_with_envelope):** `^` prefix logic matches docs
4. **Unknown type handling (plugin.rs get_routing):** Defaults to skip with warning

## Conclusion

The existing `[source.envelope]` reference entry (added in commit ecfaf41) is:
- **Accurate:** All claims match the implementation
- **Complete:** Covers all required fields, routing values, and conventions
- **Concise:** Well-structured reference without unnecessary verbosity

**No changes or tightening needed.** The documentation already meets the acceptance criteria.

## Files Reviewed

- `plugins/BUILDING_PLUGINS.md` (lines 90-145)
- `src/parser/jsonl.rs` (envelope implementation)
- `src/parser/mod.rs` (field extraction with `^` prefix)
- `src/plugin.rs` (get_routing method for unknown types)

## Acceptance Criteria Status

✅ BUILDING_PLUGINS.md has a `[source.envelope]` reference entry whose fields, routing values, and `^` prefix semantics match the implemented code.

✅ Example TOML in the entry validates the structure (verified manually against implementation).

✅ No claims in the doc that the code does not support (all verified against source).

## Outcome

**Documentation verification complete.** No modifications to BUILDING_PLUGINS.md required - existing documentation is accurate and complete.

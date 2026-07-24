# bf-5cjef: Fix Source struct compilation errors

## Issue
After adding `envelope` and `array` fields to the `plugin::Source` struct, three call sites were failing to compile with E0063 errors (missing struct fields).

## Changes Made

### Files Modified
1. **src/scraper/file_path_extractor.rs:321** - Added `envelope: None, array: None` to Source initialization
2. **src/scraper/mod.rs:1170** - Added `envelope: None, array: None` to Source initialization  
3. **src/scraper/mod.rs:1364** - Added `envelope: None, array: None` to Source initialization

### Verification
- ✅ `cargo check` passes with no compilation errors
- ✅ No E0061 or E0063 errors remain
- ✅ All struct initializations now include the required optional fields

## Note on Test Linking Issue
During verification, encountered BLAS linking errors (undefined symbols: `cblas_sgemm`, `cblas_dgemm`, etc.) from the `turbovec` dependency. This is a pre-existing environment issue unrelated to the compilation fixes. The Rust code compiles successfully - only the final binary linking step fails due to missing system BLAS libraries.

## Acceptance Criteria Met
- [x] cargo check succeeds with no E0061 errors
- [x] No new compilation warnings introduced by the fix
- [x] All Source struct call sites updated correctly

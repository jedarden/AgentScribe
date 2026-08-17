# Aider Glob Pattern Implementation Analysis

**Date:** 2026-08-16  
**Repository:** [Aider-AI/aider](https://github.com/Aider-AI/aider)  
**Purpose:** Document how aider processes glob patterns for file selection

## Summary

Aider uses a combination of Python's standard `pathlib.Path.glob()` for pattern matching and the `pathspec` library with `GitWildMatchPattern` for file filtering against git-tracked files.

## Key Implementation Files

### 1. `aider/commands.py` - Main Glob Processing

**Primary method:** `glob_filtered_to_repo` (lines 812-834)

```python
def glob_filtered_to_repo(self, pattern):
    if not pattern.strip():
        return []
    try:
        if os.path.isabs(pattern):
            # Handle absolute paths
            raw_matched_files = [Path(pattern)]
        else:
            try:
                raw_matched_files = list(Path(self.coder.root).glob(pattern))
```

**How it works:**
1. Takes a glob pattern string
2. Handles absolute paths directly
3. Uses `Path(self.coder.root).glob(pattern)` for relative patterns
4. Filters matched files against git tracked files via `get_tracked_files()`

**Pattern detection:** Checks for glob characters: `*`, `?`, `[`, `]`

### 2. Command Entry Points

**`cmd_add` method** (lines 836-904)
- Processes `/add` command with glob patterns
- Escapes glob characters for existing directories
- Validates wildcard patterns

**`cmd_drop` method** (lines 920-957)
- Handles `/drop` command with substring and glob matching
- Uses conditional logic based on pattern detection:
```python
if any(c in expanded_word for c in "*?[]"):
    matched_files = self.glob_filtered_to_repo(expanded_word)
else:
    # Use substring matching for non-glob patterns
    matched_files = [...]
```

### 3. `aider/repo.py` - File Filtering

**`get_tracked_files` method** (lines 357-403)
- Retrieves tracked files from git repository
- Iterates through git tree blobs
- Adds staged files from index
- Filters out ignored files

**File filtering infrastructure:**
- Uses `pathspec` library with `GitWildMatchPattern` (line 10: `import pathspec`)
- `refresh_aider_ignore` (lines 414-431): Creates PathSpec from `.aiderignore` file
- `ignored_file_raw` (lines 455-480): Core filtering logic using pathspec
- Combines git's native `ignored()` method with custom pathspec filtering

### 4. `aider/args.py` - CLI Arguments

No direct glob processing in arguments file:
- File arguments processed via `shtab.FILE` for tab completion
- `files` positional argument (lines 130-134)
- `--file` and `--read` appendable arguments

## Libraries Used

| Library | Purpose | Implementation |
|---------|---------|----------------|
| `pathlib.Path` | Path manipulation and glob matching | `Path(self.coder.root).glob(pattern)` |
| `pathspec` | Git-style pattern matching for filtering | `PathSpec.from_lines("gitwildmatch", ...)` |
| `glob` (standard) | Fallback glob operations | Not primary; pathlib preferred |
| `os.path` | Path operations | `os.path.isabs()` for absolute path detection |

## Pattern Entry Points

1. **CLI Commands:**
   - `/add <pattern>` - Add files matching glob pattern
   - `/drop <pattern>` - Remove files matching glob pattern

2. **Configuration:**
   - `.aider.conf.yml` - Configuration file (referenced in args.py)
   - `.aiderignore` - Ignore patterns using GitWildMatchPattern syntax

3. **Command-line Arguments:**
   - Positional `files` argument
   - `--file <pattern>` flag
   - `--read <pattern>` flag for read-only files

## Known Issues

Based on GitHub issues found:
- **Windows support:** Multiple issues (#4963, #5535, #5542) report `NotImplementedError: Non-relative patterns are unsupported` on Windows
- **Pathlib errors:** Issue #1498 mentions `AttributeError` in pathlib.py related to glob handling
- **Pattern validation:** Limited validation of glob patterns before processing

## Testing Strategy

For AgentScribe's aider plugin:
1. Test glob patterns: `*.py`, `src/**/*.rs`, `tests/*_test.rs`
2. Test absolute vs relative paths
3. Test interaction with `.aiderignore`
4. Test edge cases: empty patterns, invalid patterns, Windows paths

## Sources

- [Aider GitHub Repository](https://github.com/Aider-AI/aider)
- [commands.py source](https://raw.githubusercontent.com/Aider-AI/aider/main/aider/commands.py)
- [repo.py source](https://raw.githubusercontent.com/Aider-AI/aider/main/aider/repo.py)
- [args.py source](https://raw.githubusercontent.com/Aider-AI/aider/main/aider/args.py)
- Related GitHub Issues: #1498, #3303, #4963, #5535, #5542

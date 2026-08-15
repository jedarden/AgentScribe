# Aider Glob Pattern Implementation

**Research Date:** 2026-08-15
**Repository:** https://github.com/Aider-AI/aider
**Purpose:** Document glob pattern handling for AgentScribe plugin development

## Summary

Aider uses Python's built-in `pathlib.Path.glob()` for file pattern matching, not an external glob library. The implementation is straightforward and handles both relative and absolute paths with proper error handling.

## Implementation Location

### File: `aider/commands.py`

**Primary Method:** `glob_filtered_to_repo(self, pattern)`

```python
def glob_filtered_to_repo(self, pattern):
    if not pattern.strip():
        return []
    try:
        if os.path.isabs(pattern):
            raw_matched_files = [Path(pattern)]
        else:
            try:
                raw_matched_files = list(Path(self.coder.root).glob(pattern))
            except (IndexError, AttributeError):
                raw_matched_files = []
    except ValueError as err:
        self.io.tool_error(f"Error matching {pattern}: {err}")
        raw_matched_files = []

    matched_files = []
    for fn in raw_matched_files:
        matched_files += expand_subdir(fn)

    matched_files = [
        fn.relative_to(self.coder.root)
        for fn in matched_files
        if fn.is_relative_to(self.coder.root)
    ]

    if self.coder.repo:
        git_files = self.coder.repo.get_tracked_files()
        matched_files = [fn for fn in matched_files if str(fn) in git_files]

    res = list(map(str, matched_files))
    return res
```

**Helper Function:** `expand_subdir(file_path)`

```python
def expand_subdir(file_path):
    if file_path.is_file():
        yield file_path
        return

    if file_path.is_dir():
        for file in file_path.rglob("*"):
            if file.is_file():
                yield file
```

## Entry Points

### 1. CLI Command: `/add`

**Location:** `aider/commands.py` - `cmd_add(self, args)`

The `/add` command processes glob patterns through:
1. `parse_quoted_filenames()` - Extracts filenames from quoted/unquoted input
2. `glob_filtered_to_repo()` - Expands patterns and filters results
3. Validation against tracked git files
4. Error handling for unmatched patterns

**Usage Examples:**
- `/add src/*.rs` - Add all Rust files in src/
- `/add **/*.py` - Add all Python files recursively
- `/add tests/` - Add entire tests directory

### 2. Argument Processing: `aider/args.py`

**Arguments:**
- `files` - Positional argument for multiple FILE paths
- `--file` - Append action for files to edit
- `--read` - Append action for read-only files

Note: Argument parsing in `args.py` does not handle glob expansion - it only defines the CLI interface. Glob processing happens in `commands.py`.

## Glob Library Used

**Library:** Python Standard Library - `pathlib.Path.glob()`

**Not used:**
- ❌ ripgrep
- ❌ glob.glob
- ❌ fnmatch
- ❌ custom glob implementation

**Key characteristics:**
- Uses `Path.glob(pattern)` for pattern matching
- Uses `Path.rglob("*")` for recursive directory expansion
- Patterns are relative to `self.coder.root` (repository root)
- Absolute paths bypass globbing entirely

## Pattern Processing Flow

```
User Input (/add "src/*.rs")
    ↓
parse_quoted_filenames() - Extract quoted patterns
    ↓
glob_filtered_to_repo() - Expand patterns
    ↓
Path(self.coder.root).glob(pattern) - Match files
    ↓
expand_subdir() - Handle directory expansion
    ↓
Filter to repository root (fn.is_relative_to())
    ↓
Filter to git-tracked files (if repo available)
    ↓
Return matched file paths
```

## Error Handling

The implementation handles several edge cases:

1. **Empty patterns:** Returns empty list
2. **Absolute paths:** Bypass globbing, use path directly
3. **ValueError exceptions:** Caught and reported via `self.io.tool_error()`
4. **IndexError/AttributeError:** Caught during glob expansion
5. **Wildcard characters in creation:** Prevented with error message
6. **Outside repository:** Filtered via `is_relative_to()`
7. **Untracked files:** Filtered via git tracked files check

## Configuration Integration

### `.aiderignore` Support

Aider supports ignore patterns via `.aiderignore` file, similar to `.gitignore`. The path resolution is handled in `aider/args.py`:

```python
def resolve_aiderignore_path(path_str, git_root=None):
    path = Path(path_str)
    if path.is_absolute():
        return str(path)
    elif git_root:
        return str(Path(git_root) / path)
    return str(path)
```

### Git Integration

When a git repository is detected (`self.coder.repo`), glob results are additionally filtered to only include files tracked by git:

```python
git_files = self.coder.repo.get_tracked_files()
matched_files = [fn for fn in matched_files if str(fn) in git_files]
```

## Key Behaviors

1. **Recursive patterns:** `**/*.py` matches Python files at any depth
2. **Directory expansion:** Adding a directory recursively adds all files within
3. **Path filtering:** Results are filtered to ensure they're within the repository
4. **Git awareness:** Respects `.gitignore` and tracked files when repository is present
5. **Error recovery:** Invalid patterns don't crash - they return empty results with error message

## Sources

- [Aider GitHub Repository](https://github.com/Aider-AI/aider)
- [Invalid glob pattern crashes chat - Issue #293](https://github.com/paul-gauthier/aider/issues/293)
- [Support for regex/glob expressions in `/add` function - Issue #57](https://github.com/Aider-AI/aider/issues/57)
- [Aider Main Site](https://aider.chat/)
- [Aider FAQ](https://aider.chat/docs/faq.html)

## Implications for AgentScribe Plugin

When implementing the Aider plugin for AgentScribe:

1. **Glob patterns in configuration:** Use standard `globset` crate (already in dependencies) which supports similar patterns to Python's `pathlib.Path.glob()`

2. **Pattern matching:** AgentScribe's plugin system already supports glob patterns via `[source] paths` and `exclude` fields in TOML configuration

3. **File discovery:** The Aider plugin should:
   - Use glob patterns for discovering `.aider.chat.history.md` files
   - Support recursive patterns (`**/*.md`)
   - Handle both absolute and relative paths
   - Filter results similar to how Aider filters to repository root

4. **Error handling:** Implement similar graceful error handling for invalid patterns

## Example AgentScribe Plugin Configuration

```toml
# plugins/aider.toml
[source]
paths = [
    "~/projects/**/.aider.chat.history.md",
    "~/repos/**/.aider.chat.history.md",
    "~/projects/*/.aider.chat.history.md",
    "~/repos/*/.aider.chat.history.md"
]
exclude = [
    "**/node_modules/**",
    "**/target/**",
    "**/.git/**"
]
format = "markdown"

[source.session_detection]
method = "delimiter"
delimiter_pattern = "^# aider chat started at (.+)$"

[parser]
user_prefix = "#### "
tool_prefix = "> "
assistant_prefix = ""
```

This configuration uses glob patterns similar to Aider's approach for discovering history files across project directories.

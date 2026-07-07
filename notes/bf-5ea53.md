# Task BF-5EA53: Update aider.toml paths to recursive globs

## Finding

The file `plugins/aider.toml` already uses recursive glob patterns (`**`) throughout.

### Current patterns (all already recursive):

**Source paths:**
- `~/**/.aider.chat.history.md`

**Exclude patterns:**
- `~/**/node_modules/**/.aider.chat.history.md`
- `~/**/target/**/.aider.chat.history.md`
- `~/**/.git/**/.aider.chat.history.md`
- `~/**/.cache/**/.aider.chat.history.md`
- `~/**/venv/**/.aider.chat.history.md`
- `~/**/.venv/**/.aider.chat.history.md`
- `~/**/__pycache__/**/.aider.chat.history.md`
- `~/**/build/**/.aider.chat.history.md`
- `~/**/dist/**/.aider.chat.history.md`

### Verification

```bash
grep -n '\*' plugins/aider.toml | grep -v '\*\*'
# No output - no single-level glob patterns found
```

## Conclusion

No changes required. The task acceptance criteria are already met:
- ✓ Uses `**` patterns for all path globs
- ✓ No single-level `*` patterns remain
- ✓ File is valid TOML

The task may have been completed previously, or the file was initially created with recursive patterns.

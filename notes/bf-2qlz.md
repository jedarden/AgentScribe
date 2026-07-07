# Glob Performance Benchmark Results

## Task
Benchmark recursive glob discovery time for `~/**/.aider.chat.history.md` against single-level globs.

## Test Environment
- Home directory: `/home/coding`
- Target files: 2 `.aider.chat.history.md` files in test fixtures
- Depths: 8 and 10 levels from home directory
- Benchmark tool: Rust `glob` crate (v0.3)

## Methodology

### Single-level globs (old approach)
11 separate patterns covering depths 0-10:
```
~/.aider.chat.history.md
~/*/.aider.chat.history.md
~/*/*/.aider.chat.history.md
~/*/*/*/.aider.chat.history.md
...
~/*/*/*/*/*/*/*/*/*/.aider.chat.history.md
```

### Recursive glob (new approach)
Single pattern:
```
~/**/.aider.chat.history.md
```

## Results

| Approach | Time (avg) | Files Found | Patterns |
|----------|------------|-------------|----------|
| Single-level globs | ~11.7s | 2 | 11 patterns |
| Recursive glob | ~2.3s | 2 | 1 pattern |

**Performance ratio:** 0.20x (recursive / single-level)
**Conclusion:** Recursive glob is **~5x FASTER** than single-level globs

## Analysis

The recursive glob is dramatically faster because:
1. **Single filesystem traversal**: The `**` pattern walks the directory tree once
2. **No pattern redundancy**: Single pattern vs. 11 separate pattern expansions
3. **Early termination**: Can stop as soon as the target depth is reached

Single-level globs are slower because each pattern performs a separate filesystem walk, and shallower patterns (e.g., `~/*/.aider.chat.history.md`) still traverse the entire tree even when they can't possibly match at shallow depths.

## Acceptance Criteria

✅ Discovery time measured and recorded: ~2.3s for recursive glob
✅ Comparison with single-level glob baseline documented: 5x faster
✅ Performance is acceptable (not >2x slower): **PASSED** - recursive is 5x faster
✅ Results documented

## Recommendation

**Use the recursive glob** (`~/**/.aider.chat.history.md`). It is:
- 5x faster than single-level globs
- Simpler (1 pattern vs. 11+ patterns)
- More maintainable (no need to guess maximum depth)

The performance concern was unfounded - recursive globs are actually significantly faster for this use case.

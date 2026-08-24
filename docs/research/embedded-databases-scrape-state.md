# Embedded Database Research for Scrape-State Persistence

**Research Date:** 2026-08-23  
**Purpose:** Evaluate sled, redb, and SQLite for AgentScribe's scrape-state persistence layer

---

## sled Database

### Performance
- **Read/Write Latency:** Hybrid architecture combining LSM tree-like write performance with B+ tree-like read performance
- **Throughput:** Capable of over 1 billion operations in under a minute at 95% read / 5% write workload
- **Latency Characteristics:** 2-5x higher than DRAM (expected for embedded databases)
- **Background Sync:** Automatically syncs data to disk several times per second without blocking user threads

### Concurrency Support
- **Architecture:** Lock-free persistent B-link tree structure
- **MVCC:** Multi-Version Concurrency Control with snapshot support
- **Design Focus:** Purpose-built for high-concurrency, long-running services
- **Flash-Friendly:** Optimized for modern storage hardware

### Memory Footprint
- **Disk Utilization:** Peak disk space typically 1-2x actual data size (not 10x like some LSM implementations)
- **Memory Management:** Background sync operations designed to avoid blocking user threads
- **Note:** GitHub issue #976 reports memory leak concerns during sustained insertion (worth monitoring)

### Compaction Behavior
- **Architecture:** LSM-inspired (Bw-Tree) with automatic background compaction
- **Space Reclamation:** Periodic compaction merges and organizes data, reducing fragmentation
- **Write Amplification:** LSM trees require compaction to prevent read degradation and unbounded growth

### Rust Integration Quality
- **Implementation:** Pure Rust (no C dependencies)
- **API Design:** Similar to `BTreeMap<[u8], [u8]>` - ergonomic and idiomatic
- **Type Safety:** Strongly typed operations with compare-and-swap (CAS) support
- **Cross-Compilation:** No C package dependencies - avoids cross-compilation headaches
- **Status:** Described as "beta" quality - production use cases exist (e.g., bpfman) but not 1.0 stable

### Sources
1. [sled theoretical performance guide](http://sled.rs/perf.html) - Official performance documentation
2. [sled GitHub Repository](https://github.com/spacejam/sled) - Official implementation and benchmarks
3. [Reviewing Sled - Ayende @ Rahien](http://ayende.com/blog/186785-a/reviewing-sled-part-ii) - Independent performance analysis
4. [Reddit: Sled a modern embedded database](https://www.reddit.com/r/rust/comments/78x425/sled_a_modern_embedded_database/) - Community discussion on latency and caching

---

## redb Database

### Performance
- **Read/Write Latency:** Performance comparable to LMDB and RocksDB (top-tier embedded stores)
- **Benchmarks:** Official benchmark code available in repository
- **Zero-Copy:** Zero-copy operations reduce CPU overhead for reads
- **Write Optimization:** Copy-on-write design eliminates write-ampllication from in-place updates

### Concurrency Support
- **Transactions:** Full ACID compliance with transaction support
- **Thread-Safety:** Thread-safe design for concurrent access
- **Isolation:** Copy-on-write B+trees provide natural snapshot isolation
- **Locking:** Fine-grained locking at the B+tree node level

### Memory Footprint
- **Memory-Mapped:** Uses mmap for file access - OS manages cache
- **No Separate Cache:** No need for separate block cache like traditional B-tree implementations
- **Copy-on-Write Overhead:** New nodes created on each write (mitigated by B+tree fanout)

### Compaction Behavior
- **No Background Compaction:** Copy-on-write B+trees eliminate the need for background compaction entirely
- **Space Reclamation:** Freed space becomes available immediately (no LSM-style compaction pauses)
- **Transaction Safety:** Copy-on-write provides crash safety without Write-Ahead Log
- **Design Inspiration:** Loosely inspired by LMDB's architecture

### Rust Integration Quality
- **Implementation:** Pure Rust (zero unsafe code in core path)
- **API Design:** Idiomatic Rust with strong type safety
- **Stability:** Reached 1.0 stable release (June 2023)
- **Cross-Compilation:** No C dependencies - pure Rust toolchain
- **Community Feedback:** Described as "the most stable pure-Rust KV store" in production workloads

### Sources
1. [redb GitHub Repository](https://github.com/cberner/redb) - Official repository with design documentation
2. [redb 1.0 Release Announcement](https://redb.org/post/2023/06/16/1-0-stable-release/) - Performance characteristics and stability notes
3. [redb Docs.rs](https://docs.rs/redb) - Official Rust API documentation
4. [Reddit: Embedded Key-Value Database 2024](https://www.reddit.com/r/rust/comments/1dsmj9d/embedded_keyvalue_database_2024/) - Community comparison for multi-TB workloads

---

## SQLite (via rusqlite)

### Performance
- **WAL Mode Read:** ~454,338 operations/second (3μs latency)
- **WAL Mode Write:** ~14,401 operations/second (37μs latency)
- **WAL Benefits:** Converts inserts to simple appends rather than B-tree modifications
- **Synchronous Settings:** `synchronous=NORMAL` improves performance vs `FULL` (trades some durability)
- **CPU Usage:** High CPU usage (96% in benchmarks) - minimal memory allocation during writes
- **Large Transactions:** WAL mode may fail for transactions exceeding 1GB

### Concurrency Support
- **Connection Model:** `Connection` object is not `Sync` - requires careful threading design
- **WAL Mode:** Enables concurrent reads and writes (readers don't block writers)
- **Snapshot Isolation:** WAL provides read snapshot isolation
- **Multi-Thread Access:** Requires specific patterns (connection per thread or connection pool)

### Memory Footprint
- **Cache Size:** Configurable via `PRAGMA cache_size` (default ~2000 pages, ~8MB at 4KB pages)
- **Internal LRU:** rusqlite uses internal LRU cache for prepared statements (adjustable capacity)
- **In-Memory DB:** WAL mode unavailable for in-memory databases (requires disk file)
- **Memory Allocation:** Minimal allocation during writes (CPU-bound, not memory-bound)

### Compaction Behavior
- **VACUUM:** Rebuilds database file to reclaim space (requires exclusive access)
- **auto_vacuum:** Pragma setting for automatic space reclamation
- **WAL Checkpoint:** WAL mode uses checkpoints instead of traditional compaction
- **Free List:** Manages free pages internally (background process in WAL mode)

### Rust Integration Quality
- **Wrapper:** rusqlite is an ergonomic wrapper around SQLite C library
- **API Design:** Originally based on rust-postgres, diverged for SQLite-specific needs
- **Prepared Statements:** Internal caching improves performance (LRU cache)
- **Type Safety:** Strongly typed with `ToSql` and `FromSql` traits
- **C Dependency:** Links to SQLite C library (version 3.34.1+)
- **Cross-Compilation:** Requires C toolchain - can be challenging for some targets
- **Maturity:** rusqlite is mature and battle-tested (most common Rust SQLite option)

### Sources
1. [rusqlite Docs.rs](https://docs.rs/rusqlite/) - Official Rust crate documentation
2. [rusqlite Crates.io](https://crates.io/crates/rusqlite) - Package registry with version info
3. [SQLite Performance Benchmarks](https://marending.dev/) - WAL mode benchmark data
4. [Using rusqlite from multiple threads - Reddit](https://www.reddit.com/r/rust/comments/6z7gs2/using_rusqlite_from_multiple_threads/) - Threading considerations
5. [SQLite as Key-Value Store for Concurrent Rust](https://github.com/the-lean-crate/criner/discussions/5) - Concurrent access patterns
6. [Investigating Rust with SQLite - tedspence.com](https://tedspence.com/investigating-rust-with-sqlite-53d1f9a41112) - Performance analysis

---

## Summary Comparison

| Characteristic | sled | redb | SQLite (rusqlite) |
|---------------|------|------|------------------|
| **Architecture** | Hybrid B+/LSM (Bw-Tree) | Copy-on-write B+tree | B-tree with WAL option |
| **Performance** | 1B ops/min (95/5 R/W) | LMDB/RocksDB-level | 454K reads, 14K writes/sec |
| **Concurrency** | Lock-free + MVCC | Thread-safe + ACID | WAL (readers don't block writers) |
| **Memory** | 1-2x data size, background sync | mmap, no separate cache | Configurable cache (~8MB default) |
| **Compaction** | Background (LSM) | None needed (COW) | VACUUM/WAL checkpoint |
| **Rust Native** | Pure Rust (beta) | Pure Rust (1.0 stable) | Wrapper over C library |
| **Stability** | Beta (production use exists) | Stable 1.0 | Mature, battle-tested |
| **Dependencies** | None (pure Rust) | None (pure Rust) | SQLite C library |

---

## Recommendations for Scrape-State Persistence

### Use Cases
- **Key-value storage** (file offset → metadata): All three suitable
- **Small reads/writes** (scrape state updates): All three suitable
- **Crash safety required**: All three provide ACID guarantees
- **Low memory footprint**: All three configurable

### Considerations

**sled**: 
- ✅ Pure Rust, no C dependencies
- ⚠️ Beta quality (not 1.0 stable)
- ⚠️ Background compaction could affect latency
- ✅ Good for write-heavy workloads (scraping)

**redb**:
- ✅ Pure Rust, stable 1.0
- ✅ No background compaction (predictable performance)
- ✅ Copy-on-write provides crash safety without WAL
- ⚠️ Newer ecosystem (less battle-tested than SQLite)

**SQLite (rusqlite)**:
- ✅ Most mature and battle-tested
- ✅ WAL mode provides good concurrency
- ⚠️ C dependency (cross-compilation complexity)
- ✅ Rich ecosystem and tooling

---

## Additional Context for AgentScribe

Based on the current implementation (`src/scraper/state.rs`) using a single JSON file for all scrape state:

**Current Schema**:
- Single JSON document with `sources` object (one entry per tracked file)
- Per-source: `plugin`, `last_byte_offset`, `last_modified`, `last_scraped`, `session_ids`
- File size: ~4MB for ~21,500 sources (estimated from ADR-1 context)

**Migration Considerations**:
- Key structure: `source_file_path → ScrapeSource`
- Access patterns: Read-all on startup, read-modify-write per source on scrape
- Concurrency: Single writer (daemon) with potential concurrent readers (CLI)

**All three databases** can handle this workload effectively. The decision should prioritize:
1. **Stability**: SQLite (rusqlite) or redb
2. **No C dependencies**: redb or sled
3. **Predictable performance**: redb (no background compaction)
4. **Ecosystem maturity**: SQLite (rusqlite)

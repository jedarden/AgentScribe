# Embedded Database Comparison for AgentScribe Scrape-State Persistence

Research on three embedded database options (sled, redb, and SQLite) for scrape-state persistence in AgentScribe.

**Context**: AgentScribe currently uses a single JSON file (`scrape-state.json`) for persistence. ADR-1 identified crash-safety issues with this approach and recommends moving to a keyed store where a single corrupt write affects only one row, not the entire corpus. This document compares three embedded database options for that migration.

**Last Updated**: 2026-08-21

---

## sled

### Overview
- **Type**: Pure Rust embedded key-value database
- **Architecture**: Log-structured storage with lock-free indexing
- **Design Goals**: High-concurrency, long-running services, flash-friendly storage

### Performance
- **Read/Write Latency**: Optimized for single operation worst-case latency (primary metric over average)
- **Throughput**: High throughput design with queuing for parallel requests
- **Benchmarks**:
  - Roughly 2x slower than RocksDB in [GitHub comparison](https://github.com/tokahuke/sled-vs-rocksdb)
  - Competitive with other embedded databases for many workloads

### Concurrency Support
- **Readers**: Lock-free concurrent reads (major design feature)
- **Writers**: Optimized for concurrent access from multiple threads
- **Architecture**: Modern lock-free indexing designed specifically for high-concurrency scenarios

### Memory Footprint
- **Design Philosophy**: Peak memory utilization should be high fraction of user data
- **Allocation Strategy**: Avoids short-lived allocations in favor of stack storage for predictability
- **Memory Management**: Focus on predictable memory throughput

### Compaction Behavior
- **Architecture**: Log-structured merge (LSM) tree design
- **Compaction**: Uses LSM compaction mechanisms (file identification, memory reads, sort-merge operations)
- **Space Reclamation**: Background compaction for garbage collection and space reclamation
- **Disk Efficiency**: Aims to avoid 10x space amplification

### Rust Integration Quality
- **Native**: Pure Rust implementation, no FFI overhead
- **API**: Ergonomic Rust API with strong type safety
- **Maturity**: Active development, used in production systems
- **Documentation**: [Official performance guide](http://sled.rs/perf.html) available

### Sources
1. [sled theoretical performance guide](http://sled.rs/perf.html) - Official performance documentation
2. [sled-vs-rocksdb benchmark](https://github.com/tokahuke/sled-vs-rocksdb) - Comparative performance data
3. [sled on crates.io](https://crates.io/crates/sled) - Package registry and community feedback
4. [LSM Compaction Design Space](http://vldb.org/pvldb/vol14/p2216-sarkar.pdf) - Academic paper on LSM compaction (sled's architecture)

---

## redb

### Overview
- **Type**: Pure Rust embedded key-value database
- **Architecture**: Copy-on-write B-trees (inspired by LMDB)
- **Design Goals**: Simple, portable, high-performance, ACID-compliant

### Performance
- **Read/Write Latency**: 
  - Very good read performance (non-blocking readers)
  - Strong individual write performance
  - Similar performance to LMDB and RocksDB according to official benchmarks
- **Benchmarks**: Competitive performance against LMDB, RocksDB, and SQLite per [official 1.0 release](https://redb.org/post/2023/06/16/1-0-stable-release/)

### Concurrency Support
- **Readers**: Non-blocking readers (copy-on-write architecture)
- **Writers**: ACID transactional support
- **Limitations**: Focus on single-threaded scenarios in some benchmarks; less optimized for heavy concurrent write loads compared to sled

### Memory Footprint
- **Design**: Memory-optimized variants available (e.g., "do-memory-storage-redb" for AI agent episodic memory)
- **Savepoints**: Savepoint system for capturing database state and rolling back
- **Efficiency**: Recent versions (4.1) show up to 1.5x speedup improvements

### Compaction Behavior
- **Space Reclamation**: Adaptive compaction mechanisms to reduce in-memory reconstruction cost
- **Known Issues**: Some reports of unbounded on-disk growth without explicit vacuum operations
- **Background**: Background compaction patterns mentioned in related systems (Tidehunter research)

### Rust Integration Quality
- **Native**: Written entirely in pure Rust
- **API**: Typed API for type-safe database operations
- **Maturity**: Stable 1.0 release (June 2023), production-ready
- **Documentation**: [Official docs](https://docs.rs/redb), comprehensive guides

### Sources
1. [redb 1.0 stable release announcement](https://redb.org/post/2023/06/16/1-0-stable-release/) - Official performance characteristics
2. [redb RFC discussion on Reddit](https://www.reddit.com/r/rust/comments/13dtd2y/rfc_redb_embedded_keyvalue_store_nearing_version/) - Community discussion
3. [redb documentation](https://docs.rs/redb) - API and architecture reference
4. [Unbounded on-disk growth issue](https://github.com/defenseunicorns/peat-mesh/issues/300) - Compaction concerns

---

## SQLite

### Overview
- **Type**: Embedded SQL database (written in C)
- **Architecture**: B-tree storage with Write-Ahead Logging (WAL)
- **Design Goals**: Reliable, SQL-based, battle-tested

### Performance
- **Read/Write Latency**: 
  - Under 100 microseconds on modern NVMe SSDs per [Medium article](https://medium.com/@coders.stop/the-rise-of-the-embedded-database-why-sqlite-is-quietly-winning-the-architecture-wars-nobody-is-01aa9e5d3ab6)
- **Throughput** (WAL mode):
  - Up to 70,000 reads/s
  - Up to 3,600 writes/s
  - Many thousands of writes/s when combining multiple writes in a single transaction
- **Benchmark**: 15k inserts/s achieved in Rust benchmarks

### Concurrency Support
- **WAL Mode**: Readers don't block writers, writers don't block readers
- **Critical Limitation**: **Only one active writer at any given time**, even in WAL mode
- **Writer Behavior**: When multiple writers attempt concurrent writes, all but the first will fail (writers don't queue by default)
- **Rust Consideration**: Known lock starvation issues with connection pools (particularly with SQLx)

### Memory Footprint
- **Cache Configuration**: Default `cache_size` of ~2000 pages (~8MB)
- **Configurable**: `PRAGMA cache_size` tunable per connection
- **Page Size**: Default 4KB pages

### Compaction Behavior
- **VACUUM Command**: Explicit space reclamation operation
- **Auto-VACUUM**: Optional automatic vacuum mode (can slow down writes)
- **WAL Checkpoint**: Automatic checkpointing in WAL mode for log file management
- **Integration**: Requires manual space management or configured auto-vacuum

### Rust Integration Quality
- **Bindings**: `rusqlite` (most popular), `sqlx` (compile-time query checking)
- **FFI Overhead**: Minimal overhead - performance on par with C SQLite per [w3resource analysis](https://www.w3resource.com/sqlite/snippets/rusqlite-vs-sqlite.php)
- **Abstraction Cost**: Rust safety abstractions introduce minor FFI boundary overhead
- **Maturity**: Industry-standard, extensively tested, battle-tested
- **Cross-Compilation**: C dependency can complicate cross-compilation

### Sources
1. [SQLite performance on modern NVMe](https://medium.com/@coders.stop/the-rise-of-the-embedded-database-why-sqlite-is-quietly-winning-the-architecture-wars-nobody-is-01aa9e5d3ab6) - Performance metrics
2. [rusqlite performance analysis](https://www.w3resource.com/sqlite/snippets/rusqlite-vs-sqlite.php) - Rust integration overhead
3. [SQLite multiple writers forum discussion](https://sqlite.org/forum/info/b4e8b29ae409cd198652c6b7e70b53b702f269e67e1d2573d627feeba37bbf85) - Writer limitations
4. [SQLite transaction benchmarking with rusqlite](https://www.reddit.com/r/rust/comments/1e5cgtp/sqlite_transaction_benchmarking_with_rusqlite/) - Real-world Rust benchmarks
5. [Investigating Rust with SQLite](https://tedspence.com/investigating-rust-with-sqlite-53d1f9a41112) - Practical implementation guide

---

## Summary Comparison

| Criterion | sled | redb | SQLite (rusqlite) |
|-----------|------|------|-------------------|
| **Native Rust** | ✅ Yes | ✅ Yes | ❌ C with FFI |
| **Read Latency** | Lock-free, optimized | Very good (CoW B-tree) | <100µs on NVMe |
| **Write Latency** | Optimized for worst-case | Strong individual writes | Limited by single-writer |
| **Concurrency (Readers)** | Lock-free concurrent | Non-blocking | Concurrent (WAL mode) |
| **Concurrency (Writers)** | High-concurrency design | ACID transactions | **Single writer only** |
| **Memory Footprint** | Predictable, stack-focused | Memory-optimized variants | ~8MB default cache |
| **Compaction** | LSM background compaction | Adaptive compaction | Manual VACUUM or auto |
| **Maturity** | Active, production-used | Stable 1.0 (2023) | Industry standard |
| **API Safety** | Native Rust types | Typed API | FFI wrapper |
| **Documentation** | Performance guide available | Comprehensive docs | Extensive ecosystem |

---

## Key Findings for AgentScribe Use Case

### AgentScribe Requirements
- **Data Size**: ~4MB state file tracking ~21K sources (growing to ~500K sessions per plan)
- **Write Pattern**: Frequent small updates (per-source byte offset tracking)
- **Read Pattern**: Per-source lookups during scrape operations
- **Concurrency**: Single writer (daemon) with potential CLI read-only operations
- **Crash Safety**: Critical requirement (ADR-1)

### Database Suitability

**sled**: Best fit for high-concurrency scenarios with good crash-safe design. Lock-free architecture and pure Rust implementation align well with AgentScribe's long-running daemon pattern. LSM compaction provides automatic space management.

**redb**: Strong option with excellent read performance and ACID guarantees. Copy-on-write architecture provides good crash safety. Less proven for high-concurrency write scenarios but well-suited for AgentScribe's single-daemon pattern. Concern about unbounded growth without explicit vacuum.

**SQLite**: Battle-tested reliability with excellent read performance. **Critical limitation**: single-writer constraint could become a bottleneck if CLI operations need to write while daemon is active. FFI overhead is minimal but adds C dependency complexity for cross-compilation. Requires explicit VACUUM for space management.

### Recommendation Considerations

1. **Concurrency**: If AgentScribe ever needs true concurrent writes, sled's design is superior. SQLite's single-writer limitation is a hard constraint.
2. **Pure Rust**: Both sled and redb avoid C dependency complexity, easing cross-compilation and deployment.
3. **Crash Safety**: All three offer transactional safety, but sled and redb's modern architectures may offer simpler recovery paths.
4. **Space Management**: sled's automatic LSM compaction vs. redb's adaptive compaction vs. SQLite's manual VACUUM — automatic is preferred for a background daemon.
5. **Maturity**: SQLite has the longest track record, but redb's 1.0 release and sled's production usage provide sufficient confidence.

---

## Next Steps

1. **Prototype**: Implement scrape-state persistence with each candidate (sled, redb, SQLite) in isolated test modules
2. **Benchmark**: Measure real-world performance on AgentScribe's actual workload (21K sources, ~4MB state)
3. **Crash Recovery Testing**: Simulate crashes at various write points to verify recovery behavior
4. **Space Growth Analysis**: Monitor on-disk growth over time to validate compaction effectiveness
5. **Decision**: Select based on empirical data from testing, not theoretical characteristics alone

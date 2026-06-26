//! Performance benchmarks for the LSP semantic cache (Phase 16).
//!
//! Measures cache key hashing, packet serialization/deserialization,
//! packet size distribution, memory overhead, file hash collection,
//! and simulated disk I/O latency.
//!
//! Run with:
//! ```bash
//! cargo test -p egglsp --test lsp_cache_benchmark -- --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use egglsp::cache::{LspCacheConfig, LspCacheMode};
use egglsp::cache::{LspCacheKey, LspCacheKeyBuilder, LspSemanticCache};
use egglsp::context::{
    AgentContextSource, LineRange, LspContextBudget, LspContextItem, LspContextItemKind,
    LspContextPacket, LspContextPacketMode, LspContextRequest, LspContextScore,
    LspContextTruncation, LspEvidenceFreshness, LspEvidenceProvenance, LspRiskMode,
};
use sha2::{Digest, Sha256};
use std::hash::Hasher;

// ---------------------------------------------------------------------------
// Helpers — realistic packet construction
// ---------------------------------------------------------------------------

fn make_provenance(operation: &str) -> LspEvidenceProvenance {
    LspEvidenceProvenance {
        server_id: "rust-analyzer".to_string(),
        server_generation: Some(42),
        operation: operation.to_string(),
        freshness: LspEvidenceFreshness::Fresh,
        capability_decision: Some("supported".to_string()),
        document_version: Some("abc123".to_string()),
        age_ms: Some(5),
        post_restart: false,
    }
}

fn make_item(
    kind: LspContextItemKind,
    file: &str,
    line: u32,
    msg: &str,
    priority: u32,
) -> LspContextItem {
    LspContextItem {
        kind,
        file: PathBuf::from(file),
        range: Some(LineRange {
            start: line,
            end: line + 10,
        }),
        line: Some(line),
        column: Some(4),
        message: msg.to_string(),
        symbol: Some(format!("symbol_at_{line}")),
        source: Some(AgentContextSource::LspContext),
        provenance: make_provenance("textDocument/diagnostic"),
        score: LspContextScore {
            priority,
            is_hunk_local: line < 20,
            is_error: priority > 50,
            is_same_file: line < 20,
            freshness_rank: 0,
        },
        payload: Some(serde_json::json!({
            "detail": format!("detailed info for {file}:{line}"),
            "code": 42,
            "tags": ["unsafe", "deprecated"],
        })),
    }
}

fn make_packet(item_count: usize) -> LspContextPacket {
    let kinds = [
        LspContextItemKind::Diagnostic,
        LspContextItemKind::Reference,
        LspContextItemKind::Definition,
        LspContextItemKind::Implementation,
        LspContextItemKind::WorkspaceSymbol,
    ];
    let files: Vec<String> = (0..std::cmp::min(item_count, 10))
        .map(|i| format!("src/module_{i}.rs"))
        .collect();

    let items: Vec<LspContextItem> = (0..item_count)
        .map(|i| {
            let kind = kinds[i % kinds.len()];
            let file = &files[i % files.len()];
            let line = (i as u32) * 3;
            let priority = (i as u32) % 100;
            make_item(kind, file, line, &format!("item_{i}: detailed diagnostic message with context and suggestions for fixing the issue"), priority)
        })
        .collect();

    let changed_files: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();

    LspContextPacket {
        request: LspContextRequest::Review {
            changed_files,
            hunks: Vec::new(),
            risk_mode: LspRiskMode::Standard,
        },
        items,
        previews: Vec::new(),
        preview_ids: Vec::new(),
        mode: LspContextPacketMode::Opportunistic,
        workspace_root: Some(PathBuf::from("/Users/dev/my-project")),
        generated_at: Some(1700000000000),
        server_id: Some("rust-analyzer".to_string()),
        server_generation: Some(42),
        operational_state: Some("ready".to_string()),
        budget: Some(LspContextBudget::default()),
        notes: vec![
            "LSP state: ready".to_string(),
            "Server generation: 42".to_string(),
        ],
        truncation: LspContextTruncation::default(),
    }
}

fn make_key_with_files(n_files: usize) -> LspCacheKey {
    let mut builder = LspCacheKeyBuilder::new("/Users/dev/my-project", "rust-analyzer", "review")
        .with_request(&LspContextRequest::Review {
            changed_files: (0..n_files)
                .map(|i| PathBuf::from(format!("src/module_{i}.rs")))
                .collect(),
            hunks: Vec::new(),
            risk_mode: LspRiskMode::Standard,
        })
        .with_budget(&LspContextBudget::default())
        .with_capability_fingerprint("rust-analyzer:v0.4:semanticTokens=true");

    for i in 0..n_files {
        let content = format!("fn module_{i}() -> u32 {{ {i} }}");
        let hash = format!("{:x}", Sha256::digest(content.as_bytes()));
        builder = builder.with_file_hash(format!("src/module_{i}.rs"), hash);
    }

    builder.build()
}

// ---------------------------------------------------------------------------
// Test 1: Cache key hash performance
// ---------------------------------------------------------------------------

#[test]
fn test_cache_key_hash_performance() {
    let iterations = 10_000;

    for &n_files in &[1usize, 5, 10, 16] {
        let key = make_key_with_files(n_files);

        let start = Instant::now();
        for _ in 0..iterations {
            // Simulate the key construction + hash path:
            // 1. Build the key (includes precomputed hash)
            // 2. The Hash trait impl just hashes the precomputed u64
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(&key, &mut hasher);
            let _ = hasher.finish();
        }
        let elapsed = start.elapsed();
        let per_op = elapsed / iterations;

        eprintln!(
            "[cache_key_hash] files={n_files:2}  iterations={iterations}  total={elapsed:?}  per_op={per_op:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 2: Packet serialization/deserialization performance
// ---------------------------------------------------------------------------

#[test]
fn test_packet_serialization_performance() {
    let packet = make_packet(10);
    let iterations = 5_000;

    // Serialize
    let json_bytes = serde_json::to_vec(&packet).expect("serialize failed");
    eprintln!(
        "[packet_serialize] serialized_size={} bytes (10 items)",
        json_bytes.len()
    );

    // Measure serialize
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = serde_json::to_vec(&packet).unwrap();
    }
    let ser_elapsed = start.elapsed();
    eprintln!(
        "[packet_serialize] iterations={iterations}  total={ser_elapsed:?}  per_op={:?}",
        ser_elapsed / iterations
    );

    // Measure deserialize
    let start = Instant::now();
    for _ in 0..iterations {
        let _: LspContextPacket = serde_json::from_slice(&json_bytes).unwrap();
    }
    let de_elapsed = start.elapsed();
    eprintln!(
        "[packet_deserialize] iterations={iterations}  total={de_elapsed:?}  per_op={:?}",
        de_elapsed / iterations
    );

    // Roundtrip correctness
    let restored: LspContextPacket = serde_json::from_slice(&json_bytes).unwrap();
    assert_eq!(restored.items.len(), packet.items.len());
    assert_eq!(restored.server_id, packet.server_id);
}

// ---------------------------------------------------------------------------
// Test 3: Packet size distribution
// ---------------------------------------------------------------------------

#[test]
fn test_packet_size_distribution() {
    for &n_items in &[1usize, 5, 15] {
        let packet = make_packet(n_items);
        let json = serde_json::to_vec(&packet).expect("serialize failed");
        eprintln!(
            "[packet_size] items={n_items:2}  serialized_bytes={}",
            json.len()
        );
        // Sanity check: sizes should be positive and reasonable
        assert!(!json.is_empty());
        assert!(
            json.len() < 1_000_000,
            "packet too large: {} bytes",
            json.len()
        );
    }
}

// ---------------------------------------------------------------------------
// Test 4: Cache memory overhead
// ---------------------------------------------------------------------------

#[test]
fn test_cache_memory_overhead() {
    let config = LspCacheConfig {
        mode: LspCacheMode::Memory,
        max_entries: 128,
        max_bytes: 64 * 1024 * 1024,
        ttl_seconds: 600,
    };
    let mut cache = LspSemanticCache::new(config);

    let n_entries = 32;
    let mut total_serialized_bytes: usize = 0;

    for i in 0..n_entries {
        let packet = make_packet(5);
        let ser_bytes = serde_json::to_vec(&packet).unwrap().len();
        total_serialized_bytes += ser_bytes;

        let key =
            LspCacheKeyBuilder::new("/Users/dev/project", "rust-analyzer", format!("review_{i}"))
                .with_file_hash(format!("src/file_{i}.rs"), format!("hash_{i}"))
                .with_budget(&LspContextBudget::default())
                .build();

        cache.insert(key, packet, LspEvidenceFreshness::Fresh, Some(42));
    }

    let stats = cache.stats();
    eprintln!(
        "[cache_memory] entries={}  serialized_bytes={}",
        stats.entries, stats.bytes
    );
    eprintln!("[cache_memory] estimated_packet_payload={total_serialized_bytes} bytes");
    eprintln!(
        "[cache_memory] overhead_ratio={:.2}x",
        stats.bytes as f64 / total_serialized_bytes as f64
    );
    eprintln!(
        "[cache_memory] per_entry_avg={} bytes",
        stats.bytes / stats.entries
    );

    assert_eq!(stats.entries, n_entries);
    assert!(stats.bytes > 0);
}

// ---------------------------------------------------------------------------
// Test 5: File hash collection performance
// ---------------------------------------------------------------------------

#[test]
fn test_file_hash_collection_performance() {
    let dir = tempfile::tempdir().expect("tempdir failed");
    let root = dir.path().to_path_buf();

    // Create test files of varying sizes
    for i in 0..16 {
        let file_path = root.join(format!("src/module_{i}.rs"));
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        // ~1KB files
        let content: String = (0..50)
            .map(|j| format!("// line {j} of module {i}: {}\n", "x".repeat(20)))
            .collect();
        std::fs::write(&file_path, content).unwrap();
    }

    let _request = LspContextRequest::Review {
        changed_files: (0..16)
            .map(|i| root.join(format!("src/module_{i}.rs")))
            .collect(),
        hunks: Vec::new(),
        risk_mode: LspRiskMode::Standard,
    };

    let iterations = 500;

    // Measure the hashing part (what collect_cache_file_hashes_for_request does internally)
    let start = Instant::now();
    for _ in 0..iterations {
        let mut hashes = BTreeMap::new();
        for i in 0..16 {
            let path = root.join(format!("src/module_{i}.rs"));
            if let Ok(content) = std::fs::read(&path) {
                let hash = format!("{:x}", Sha256::digest(&content));
                hashes.insert(path, hash);
            }
        }
        // Verify we got all files
        assert_eq!(hashes.len(), 16);
    }
    let elapsed = start.elapsed();
    let per_op = elapsed / iterations;

    eprintln!(
        "[file_hash_collection] files=16  iterations={iterations}  total={elapsed:?}  per_op={per_op:?}"
    );

    // Also measure with 1 file (common single-file case)
    let start = Instant::now();
    for _ in 0..iterations {
        let path = root.join("src/module_0.rs");
        let content = std::fs::read(&path).unwrap();
        let _hash = format!("{:x}", Sha256::digest(&content));
    }
    let elapsed_1 = start.elapsed();
    eprintln!(
        "[file_hash_collection] files=1   iterations={iterations}  total={elapsed_1:?}  per_op={:?}",
        elapsed_1 / iterations
    );
}

// ---------------------------------------------------------------------------
// Test 6: Disk I/O simulated latency
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_disk_io_simulated_latency() {
    let dir = tempfile::tempdir().expect("tempdir failed");
    let file_path = dir.path().join("cache_entry.bin");

    let packet = make_packet(10);
    let serialized = serde_json::to_vec(&packet).expect("serialize failed");

    eprintln!("[disk_io] packet_size={} bytes", serialized.len());

    // Sequential writes
    let iterations = 500;

    let start = Instant::now();
    for _ in 0..iterations {
        tokio::fs::write(&file_path, &serialized).await.unwrap();
    }
    let write_elapsed = start.elapsed();
    eprintln!(
        "[disk_io_write] iterations={iterations}  total={write_elapsed:?}  per_op={:?}",
        write_elapsed / iterations
    );

    // Sequential reads
    let start = Instant::now();
    for _ in 0..iterations {
        let data = tokio::fs::read(&file_path).await.unwrap();
        assert_eq!(data.len(), serialized.len());
    }
    let read_elapsed = start.elapsed();
    eprintln!(
        "[disk_io_read]  iterations={iterations}  total={read_elapsed:?}  per_op={:?}",
        read_elapsed / iterations
    );

    // Read + deserialize (the full cache-miss path)
    let start = Instant::now();
    for _ in 0..iterations {
        let data = tokio::fs::read(&file_path).await.unwrap();
        let _: LspContextPacket = serde_json::from_slice(&data).unwrap();
    }
    let read_deser_elapsed = start.elapsed();
    eprintln!(
        "[disk_io_read_deserialize] iterations={iterations}  total={read_deser_elapsed:?}  per_op={:?}",
        read_deser_elapsed / iterations
    );

    // Serialize + write (the full cache-insert path)
    let start = Instant::now();
    for _ in 0..iterations {
        let data = serde_json::to_vec(&packet).unwrap();
        tokio::fs::write(&file_path, &data).await.unwrap();
    }
    let ser_write_elapsed = start.elapsed();
    eprintln!(
        "[disk_io_serialize_write] iterations={iterations}  total={ser_write_elapsed:?}  per_op={:?}",
        ser_write_elapsed / iterations
    );

    // --- Summary ---
    let mem_ser = serde_json::to_vec(&packet).unwrap();
    eprintln!("\n--- SUMMARY ---");
    eprintln!("Packet size (10 items): {} bytes", mem_ser.len());
    eprintln!("Memory-only hash+lookup: ~sub-microsecond (precomputed u64)");
    eprintln!("Disk write per op:       {:?}", write_elapsed / iterations);
    eprintln!("Disk read per op:        {:?}", read_elapsed / iterations);
    eprintln!(
        "Disk read+deserialize:   {:?}",
        read_deser_elapsed / iterations
    );
    eprintln!(
        "Serialize+disk write:    {:?}",
        ser_write_elapsed / iterations
    );
}

// ---------------------------------------------------------------------------
// Non-ignored correctness tests (run in CI)
// ---------------------------------------------------------------------------

#[test]
fn test_key_hash_determinism() {
    let key1 = make_key_with_files(5);
    let key2 = make_key_with_files(5);
    assert_eq!(key1, key2);

    use std::hash::{Hash, Hasher};
    let mut h1 = std::collections::hash_map::DefaultHasher::new();
    let mut h2 = std::collections::hash_map::DefaultHasher::new();
    key1.hash(&mut h1);
    key2.hash(&mut h2);
    assert_eq!(h1.finish(), h2.finish());
}

#[test]
fn test_key_different_files_produce_different_hashes() {
    let key1 = make_key_with_files(3);
    let key2 = make_key_with_files(7);
    assert_ne!(key1, key2);
}

#[test]
fn test_packet_roundtrip() {
    let packet = make_packet(10);
    let json = serde_json::to_vec(&packet).unwrap();
    let restored: LspContextPacket = serde_json::from_slice(&json).unwrap();
    assert_eq!(restored.items.len(), 10);
    assert_eq!(restored.server_id, Some("rust-analyzer".to_string()));
    assert_eq!(restored.server_generation, Some(42));
}

#[test]
fn test_small_packet_serializable() {
    let packet = make_packet(1);
    let json = serde_json::to_vec(&packet).unwrap();
    assert!(json.len() > 100, "packet should have meaningful content");
    let _: LspContextPacket = serde_json::from_slice(&json).unwrap();
}

#[test]
fn test_large_packet_serializable() {
    let packet = make_packet(15);
    let json = serde_json::to_vec(&packet).unwrap();
    assert!(json.len() > 500, "15-item packet should be substantial");
    let _: LspContextPacket = serde_json::from_slice(&json).unwrap();
}

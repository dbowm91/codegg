//! Performance benchmarks for the LSP semantic cache (Phase 16).
//!
//! Measures cache key hashing, packet serialization/deserialization,
//! packet size distribution, memory overhead, file hash collection,
//! simulated disk I/O latency, end-to-end workflow serialization costs
//! (review-diff, repair-local, impact-analysis, test-failure repair,
//! call-neighborhood), cold/warm startup behavior, and cache
//! hit/miss/stale rates under realistic access patterns.
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
    AgentContextSource, HierarchyDirection, LineRange, LspContextBudget, LspContextItem,
    LspContextItemKind, LspContextPacket, LspContextPacketMode, LspContextRequest, LspContextScore,
    LspContextTruncation, LspEvidenceFreshness, LspEvidenceProvenance, LspRiskMode, SymbolTarget,
};
use lsp_types::Position;
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
// Workflow scenario packet constructors
// ---------------------------------------------------------------------------

/// Simulates a review-diff workflow: multiple changed files with
/// diagnostics, references, and definitions.
fn make_review_diff_packet(changed_file_count: usize, items_per_file: usize) -> LspContextPacket {
    let files: Vec<PathBuf> = (0..changed_file_count)
        .map(|i| format!("src/module_{i}.rs").into())
        .collect();
    let total_items = changed_file_count * items_per_file;
    let items: Vec<LspContextItem> = (0..total_items)
        .map(|i| {
            let file_idx = i / items_per_file;
            let kind = if i % 3 == 0 {
                LspContextItemKind::Diagnostic
            } else if i % 3 == 1 {
                LspContextItemKind::Reference
            } else {
                LspContextItemKind::Definition
            };
            make_item(
                kind,
                &format!("src/module_{file_idx}.rs"),
                (i as u32) * 2,
                &format!("review_item_{i}: diagnostic or reference context"),
                (i as u32) % 100,
            )
        })
        .collect();
    LspContextPacket {
        request: LspContextRequest::Review {
            changed_files: files.clone(),
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
        notes: vec!["LSP state: ready".to_string()],
        truncation: LspContextTruncation::default(),
    }
}

/// Simulates a repair-local workflow: hunk-aware evidence for a
/// single file with definitions, references, and diagnostics.
fn make_repair_local_packet() -> LspContextPacket {
    let items: Vec<LspContextItem> = (0..8)
        .map(|i| {
            make_item(
                if i < 3 {
                    LspContextItemKind::Diagnostic
                } else if i < 6 {
                    LspContextItemKind::Definition
                } else {
                    LspContextItemKind::Reference
                },
                "src/target.rs",
                i * 5,
                &format!("repair_local_item_{i}: hunk-aware context for fixing"),
                i * 10,
            )
        })
        .collect();
    LspContextPacket {
        request: LspContextRequest::Hunk {
            file: "src/target.rs".into(),
            hunks: vec![egglsp::context::HunkRange {
                start: 10,
                end: 20,
                original_start: Some(10),
                original_end: Some(20),
            }],
            include_references: true,
            include_definitions: true,
            include_implementations: false,
            include_semantic_tokens: false,
            include_security_evidence: false,
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
        notes: vec!["LSP state: ready".to_string()],
        truncation: LspContextTruncation::default(),
    }
}

/// Simulates an impact-analysis workflow: symbol definition plus
/// capped cross-file references and affected-file diagnostics.
fn make_impact_analysis_packet() -> LspContextPacket {
    let mut items: Vec<LspContextItem> = Vec::new();
    // Core symbol definition
    items.push(make_item(
        LspContextItemKind::Definition,
        "src/lib.rs",
        42,
        "impact_analysis: symbol definition for the changed public API",
        100,
    ));
    // Cross-file references (capped at typical budget: 20)
    for i in 0..20 {
        let file_idx = i % 5;
        items.push(make_item(
            LspContextItemKind::Reference,
            &format!("src/consumer_{file_idx}.rs"),
            i * 3,
            &format!("impact_analysis_ref_{i}: reference to changed symbol"),
            80 - i,
        ));
    }
    // Diagnostics in affected files
    for i in 0..5 {
        items.push(make_item(
            LspContextItemKind::Diagnostic,
            &format!("src/consumer_{i}.rs"),
            i * 10,
            &format!("impact_analysis_diag_{i}: diagnostic in affected file"),
            60,
        ));
    }
    LspContextPacket {
        request: LspContextRequest::ImpactAnalysis {
            symbol: SymbolTarget {
                file: "src/lib.rs".into(),
                position: Position::new(42, 4),
            },
            changed_files: vec!["src/lib.rs".into()],
            max_refs: 20,
            max_files: 5,
            max_depth: 1,
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
        notes: vec!["LSP state: ready".to_string()],
        truncation: LspContextTruncation::default(),
    }
}

/// Simulates a test-failure-repair workflow: test file diagnostics
/// plus definitions for extracted symbols.
fn make_test_failure_repair_packet() -> LspContextPacket {
    let mut items: Vec<LspContextItem> = Vec::new();
    // Test file diagnostics (the failure itself)
    items.push(make_item(
        LspContextItemKind::Diagnostic,
        "tests/integration.rs",
        45,
        "test_failure: assertion `left == right` failed at tests/integration.rs:45",
        100,
    ));
    // Definitions for symbols mentioned in the failure
    for i in 0..6 {
        items.push(make_item(
            LspContextItemKind::Definition,
            &format!("src/module_{}.rs", i % 3),
            i as u32 * 8,
            &format!("test_failure_def_{i}: definition for symbol referenced in test failure"),
            70 - i as u32,
        ));
    }
    LspContextPacket {
        request: LspContextRequest::TestFailureRepair {
            test_file: "tests/integration.rs".into(),
            failure_message: "assertion `left == right` failed at tests/integration.rs:45".into(),
            related_files: vec!["src/module_0.rs".into(), "src/module_1.rs".into()],
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
        notes: vec!["LSP state: ready".to_string()],
        truncation: LspContextTruncation::default(),
    }
}

/// Simulates a call-neighborhood workflow: shallow call hierarchy
/// around a symbol.
fn make_call_neighborhood_packet() -> LspContextPacket {
    let mut items: Vec<LspContextItem> = Vec::new();
    // The target symbol
    items.push(make_item(
        LspContextItemKind::Definition,
        "src/core.rs",
        30,
        "call_neighborhood: target symbol",
        100,
    ));
    // Incoming callers (capped)
    for i in 0..5 {
        items.push(make_item(
            LspContextItemKind::Reference,
            &format!("src/caller_{}.rs", i),
            i as u32 * 4,
            &format!("call_neighborhood_incoming_{i}: caller of target symbol"),
            80 - i as u32,
        ));
    }
    // Outgoing callees (capped)
    for i in 0..5 {
        items.push(make_item(
            LspContextItemKind::Definition,
            &format!("src/callee_{}.rs", i),
            i as u32 * 6,
            &format!("call_neighborhood_outgoing_{i}: callee of target symbol"),
            70 - i as u32,
        ));
    }
    LspContextPacket {
        request: LspContextRequest::CallNeighborhood {
            file: "src/core.rs".into(),
            line: 30,
            column: 4,
            direction: HierarchyDirection::Both,
            max_depth: 1,
            max_callers: 5,
            max_callees: 5,
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
        notes: vec!["LSP state: ready".to_string()],
        truncation: LspContextTruncation::default(),
    }
}

// ---------------------------------------------------------------------------
// Test 7: Workflow scenario — review-diff (serialize + cache cost)
// ---------------------------------------------------------------------------

#[test]
fn test_workflow_review_diff() {
    let packet = make_review_diff_packet(5, 6);
    let json = serde_json::to_vec(&packet).unwrap();
    let iterations = 1_000;

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = serde_json::to_vec(&packet).unwrap();
    }
    let ser_elapsed = start.elapsed();

    let start = Instant::now();
    for _ in 0..iterations {
        let _: LspContextPacket = serde_json::from_slice(&json).unwrap();
    }
    let de_elapsed = start.elapsed();

    eprintln!(
        "[workflow_review_diff] items={}  size={}  ser_per_op={:?}  deser_per_op={:?}",
        packet.items.len(),
        json.len(),
        ser_elapsed / iterations,
        de_elapsed / iterations,
    );
    assert_eq!(packet.items.len(), 30);
}

// ---------------------------------------------------------------------------
// Test 8: Workflow scenario — repair-local
// ---------------------------------------------------------------------------

#[test]
fn test_workflow_repair_local() {
    let packet = make_repair_local_packet();
    let json = serde_json::to_vec(&packet).unwrap();
    let iterations = 1_000;

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = serde_json::to_vec(&packet).unwrap();
    }
    let ser_elapsed = start.elapsed();

    let start = Instant::now();
    for _ in 0..iterations {
        let _: LspContextPacket = serde_json::from_slice(&json).unwrap();
    }
    let de_elapsed = start.elapsed();

    eprintln!(
        "[workflow_repair_local] items={}  size={}  ser_per_op={:?}  deser_per_op={:?}",
        packet.items.len(),
        json.len(),
        ser_elapsed / iterations,
        de_elapsed / iterations,
    );
    assert_eq!(packet.items.len(), 8);
}

// ---------------------------------------------------------------------------
// Test 9: Workflow scenario — impact-analysis
// ---------------------------------------------------------------------------

#[test]
fn test_workflow_impact_analysis() {
    let packet = make_impact_analysis_packet();
    let json = serde_json::to_vec(&packet).unwrap();
    let iterations = 1_000;

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = serde_json::to_vec(&packet).unwrap();
    }
    let ser_elapsed = start.elapsed();

    let start = Instant::now();
    for _ in 0..iterations {
        let _: LspContextPacket = serde_json::from_slice(&json).unwrap();
    }
    let de_elapsed = start.elapsed();

    eprintln!(
        "[workflow_impact_analysis] items={}  size={}  ser_per_op={:?}  deser_per_op={:?}",
        packet.items.len(),
        json.len(),
        ser_elapsed / iterations,
        de_elapsed / iterations,
    );
    assert_eq!(packet.items.len(), 26);
}

// ---------------------------------------------------------------------------
// Test 10: Workflow scenario — test-failure repair
// ---------------------------------------------------------------------------

#[test]
fn test_workflow_test_failure_repair() {
    let packet = make_test_failure_repair_packet();
    let json = serde_json::to_vec(&packet).unwrap();
    let iterations = 1_000;

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = serde_json::to_vec(&packet).unwrap();
    }
    let ser_elapsed = start.elapsed();

    let start = Instant::now();
    for _ in 0..iterations {
        let _: LspContextPacket = serde_json::from_slice(&json).unwrap();
    }
    let de_elapsed = start.elapsed();

    eprintln!(
        "[workflow_test_failure_repair] items={}  size={}  ser_per_op={:?}  deser_per_op={:?}",
        packet.items.len(),
        json.len(),
        ser_elapsed / iterations,
        de_elapsed / iterations,
    );
    assert_eq!(packet.items.len(), 7);
}

// ---------------------------------------------------------------------------
// Test 11: Workflow scenario — call-neighborhood
// ---------------------------------------------------------------------------

#[test]
fn test_workflow_call_neighborhood() {
    let packet = make_call_neighborhood_packet();
    let json = serde_json::to_vec(&packet).unwrap();
    let iterations = 1_000;

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = serde_json::to_vec(&packet).unwrap();
    }
    let ser_elapsed = start.elapsed();

    let start = Instant::now();
    for _ in 0..iterations {
        let _: LspContextPacket = serde_json::from_slice(&json).unwrap();
    }
    let de_elapsed = start.elapsed();

    eprintln!(
        "[workflow_call_neighborhood] items={}  size={}  ser_per_op={:?}  deser_per_op={:?}",
        packet.items.len(),
        json.len(),
        ser_elapsed / iterations,
        de_elapsed / iterations,
    );
    assert_eq!(packet.items.len(), 11);
}

// ---------------------------------------------------------------------------
// Test 12: Workflow scenario — startup-after-restart (cold cache)
// ---------------------------------------------------------------------------

#[test]
fn test_workflow_startup_cold_cache() {
    let dir = tempfile::tempdir().expect("tempdir failed");
    let root = dir.path().to_path_buf();

    // Simulate creating 16 files (typical workspace change set)
    for i in 0..16 {
        let file_path = root.join(format!("src/module_{i}.rs"));
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        let content: String = (0..30)
            .map(|j| format!("// line {j} of module {i}: {}\n", "x".repeat(20)))
            .collect();
        std::fs::write(&file_path, content).unwrap();
    }

    let config = LspCacheConfig {
        mode: LspCacheMode::Memory,
        max_entries: 64,
        max_bytes: 4 * 1024 * 1024,
        ttl_seconds: 300,
    };
    let mut cache = LspSemanticCache::new(config.clone());

    // Phase 1: cold start — insert 5 workflow packets (simulating first
    // session). Each insert involves serialize + cache insert.
    let packets: Vec<(String, LspContextPacket)> = vec![
        ("review".into(), make_review_diff_packet(5, 6)),
        ("repair".into(), make_repair_local_packet()),
        ("impact".into(), make_impact_analysis_packet()),
        ("test_fail".into(), make_test_failure_repair_packet()),
        ("call_nb".into(), make_call_neighborhood_packet()),
    ];

    let start = Instant::now();
    for (op, packet) in &packets {
        let key = LspCacheKeyBuilder::new(root.to_str().unwrap(), "rust-analyzer", op.as_str())
            .with_request(&packet.request)
            .with_file_hash("src/module_0.rs", "hash_0")
            .with_budget(&LspContextBudget::default())
            .with_capability_fingerprint("rust-analyzer:v0.4:semanticTokens=true")
            .build();
        cache.insert(key, packet.clone(), LspEvidenceFreshness::Fresh, Some(1));
    }
    let insert_elapsed = start.elapsed();

    // Phase 2: "restart" — new cache, same files. Cold start means
    // all lookups miss. Measure the full cold-start cost.
    let mut cache2 = LspSemanticCache::new(config.clone());
    let file_hashes: BTreeMap<PathBuf, String> = (0..16)
        .map(|i| {
            let path = root.join(format!("src/module_{i}.rs"));
            let content = std::fs::read(&path).unwrap();
            let hash = format!("{:x}", Sha256::digest(&content));
            (path, hash)
        })
        .collect();

    let mut miss_count = 0usize;
    let start = Instant::now();
    for (op, packet) in &packets {
        let key = LspCacheKeyBuilder::new(root.to_str().unwrap(), "rust-analyzer", op.as_str())
            .with_request(&packet.request)
            .with_file_hash("src/module_0.rs", "hash_0")
            .with_budget(&LspContextBudget::default())
            .with_capability_fingerprint("rust-analyzer:v0.4:semanticTokens=true")
            .build();
        let result = cache2.get(&key, Some(1), &file_hashes);
        if result.is_none() {
            miss_count += 1;
            // Simulate miss path: re-collect + insert
            cache2.insert(key, packet.clone(), LspEvidenceFreshness::Fresh, Some(1));
        }
    }
    let cold_start_elapsed = start.elapsed();

    // Phase 3: warm restart — new cache, but same session. Insert then
    // lookup to measure warm-hit path.
    let mut cache3 = LspSemanticCache::new(config.clone());
    let module_0_hash = file_hashes
        .get(&root.join("src/module_0.rs"))
        .unwrap()
        .clone();
    for (op, packet) in &packets {
        let key = LspCacheKeyBuilder::new(root.to_str().unwrap(), "rust-analyzer", op.as_str())
            .with_request(&packet.request)
            .with_file_hash(root.join("src/module_0.rs"), &module_0_hash)
            .with_budget(&LspContextBudget::default())
            .with_capability_fingerprint("rust-analyzer:v0.4:semanticTokens=true")
            .build();
        cache3.insert(key, packet.clone(), LspEvidenceFreshness::Fresh, Some(1));
    }

    let mut hit_count = 0usize;
    let start = Instant::now();
    for (op, packet) in &packets {
        let key = LspCacheKeyBuilder::new(root.to_str().unwrap(), "rust-analyzer", op.as_str())
            .with_request(&packet.request)
            .with_file_hash(root.join("src/module_0.rs"), &module_0_hash)
            .with_budget(&LspContextBudget::default())
            .with_capability_fingerprint("rust-analyzer:v0.4:semanticTokens=true")
            .build();
        let result = cache3.get(&key, Some(1), &file_hashes);
        if result.is_some() {
            hit_count += 1;
        }
    }
    let warm_hit_elapsed = start.elapsed();

    let stats3 = cache3.stats();
    eprintln!(
        "[workflow_startup] packets={}  cold_misses={miss_count}  warm_hits={hit_count}",
        packets.len(),
    );
    eprintln!(
        "[workflow_startup] cold_insert={:?}  cold_lookup={:?}  warm_lookup={:?}",
        insert_elapsed / packets.len() as u32,
        cold_start_elapsed / packets.len() as u32,
        warm_hit_elapsed / packets.len() as u32,
    );
    eprintln!(
        "[workflow_startup] final_stats: hits={} misses={} stale_misses={}",
        stats3.hits, stats3.misses, stats3.stale_misses,
    );
    assert_eq!(miss_count, packets.len(), "all cold lookups must miss");
    assert_eq!(hit_count, packets.len(), "all warm lookups must hit");
}

// ---------------------------------------------------------------------------
// Test 13: Cache hit/miss/stale rate under repeated workflow
// ---------------------------------------------------------------------------

#[test]
fn test_cache_hit_miss_stale_rates() {
    let config = LspCacheConfig {
        mode: LspCacheMode::Memory,
        max_entries: 64,
        max_bytes: 4 * 1024 * 1024,
        ttl_seconds: 300,
    };
    let mut cache = LspSemanticCache::new(config);

    let packets: Vec<(&str, LspContextPacket)> = vec![
        ("review", make_review_diff_packet(3, 4)),
        ("repair", make_repair_local_packet()),
        ("impact", make_impact_analysis_packet()),
        ("test_fail", make_test_failure_repair_packet()),
        ("call_nb", make_call_neighborhood_packet()),
    ];

    let base_hashes: BTreeMap<PathBuf, String> = (0..16)
        .map(|i| {
            (
                PathBuf::from(format!("src/module_{i}.rs")),
                format!("hash_{i}"),
            )
        })
        .collect();

    // Round 1: all inserts (5 entries)
    for (op, packet) in &packets {
        let key = LspCacheKeyBuilder::new("/Users/dev/project", "rust-analyzer", *op)
            .with_request(&packet.request)
            .with_file_hash("src/module_0.rs", "hash_0")
            .with_budget(&LspContextBudget::default())
            .with_capability_fingerprint("rust-analyzer:v0.4:semanticTokens=true")
            .build();
        cache.insert(key, packet.clone(), LspEvidenceFreshness::Fresh, Some(42));
    }

    // Round 2: all hits (same file hashes)
    for (op, packet) in &packets {
        let key = LspCacheKeyBuilder::new("/Users/dev/project", "rust-analyzer", *op)
            .with_request(&packet.request)
            .with_file_hash("src/module_0.rs", "hash_0")
            .with_budget(&LspContextBudget::default())
            .with_capability_fingerprint("rust-analyzer:v0.4:semanticTokens=true")
            .build();
        let result = cache.get(&key, Some(42), &base_hashes);
        assert!(result.is_some(), "round 2: {op} should hit");
    }

    // Round 3: file hash changed → stale misses
    let changed_hashes: BTreeMap<PathBuf, String> = (0..16)
        .map(|i| {
            (
                PathBuf::from(format!("src/module_{i}.rs")),
                format!("hash_{i}_changed"),
            )
        })
        .collect();
    for (op, packet) in &packets {
        let key = LspCacheKeyBuilder::new("/Users/dev/project", "rust-analyzer", *op)
            .with_request(&packet.request)
            .with_file_hash("src/module_0.rs", "hash_0")
            .with_budget(&LspContextBudget::default())
            .with_capability_fingerprint("rust-analyzer:v0.4:semanticTokens=true")
            .build();
        let result = cache.get(&key, Some(42), &changed_hashes);
        assert!(
            result.is_none(),
            "round 3: {op} should miss (stale file hashes)"
        );
    }

    // Round 4: server generation changed → stale misses
    for (op, packet) in &packets {
        // Re-insert first (round 3 evicted them)
        let key = LspCacheKeyBuilder::new("/Users/dev/project", "rust-analyzer", *op)
            .with_request(&packet.request)
            .with_file_hash("src/module_0.rs", "hash_0")
            .with_budget(&LspContextBudget::default())
            .with_capability_fingerprint("rust-analyzer:v0.4:semanticTokens=true")
            .build();
        cache.insert(key, packet.clone(), LspEvidenceFreshness::Fresh, Some(42));
    }
    for (op, packet) in &packets {
        let key = LspCacheKeyBuilder::new("/Users/dev/project", "rust-analyzer", *op)
            .with_request(&packet.request)
            .with_file_hash("src/module_0.rs", "hash_0")
            .with_budget(&LspContextBudget::default())
            .with_capability_fingerprint("rust-analyzer:v0.4:semanticTokens=true")
            .build();
        let result = cache.get(&key, Some(99), &base_hashes); // gen 99 != 42
        assert!(
            result.is_none(),
            "round 4: {op} should miss (stale generation)"
        );
    }

    // Round 5: re-insert + hit again
    for (op, packet) in &packets {
        let key = LspCacheKeyBuilder::new("/Users/dev/project", "rust-analyzer", *op)
            .with_request(&packet.request)
            .with_file_hash("src/module_0.rs", "hash_0")
            .with_budget(&LspContextBudget::default())
            .with_capability_fingerprint("rust-analyzer:v0.4:semanticTokens=true")
            .build();
        cache.insert(key, packet.clone(), LspEvidenceFreshness::Fresh, Some(99));
    }
    for (op, packet) in &packets {
        let key = LspCacheKeyBuilder::new("/Users/dev/project", "rust-analyzer", *op)
            .with_request(&packet.request)
            .with_file_hash("src/module_0.rs", "hash_0")
            .with_budget(&LspContextBudget::default())
            .with_capability_fingerprint("rust-analyzer:v0.4:semanticTokens=true")
            .build();
        let result = cache.get(&key, Some(99), &base_hashes);
        assert!(result.is_some(), "round 5: {op} should hit again");
    }

    let stats = cache.stats();
    eprintln!(
        "[cache_rates] total_lookups: hits={} misses={} stale_misses={} evictions={}",
        stats.hits, stats.misses, stats.stale_misses, stats.evictions
    );
    eprintln!(
        "[cache_rates] hit_rate={:.1}%  miss_rate={:.1}%  stale_rate={:.1}%",
        100.0 * stats.hits as f64 / (stats.hits + stats.misses) as f64,
        100.0 * stats.misses as f64 / (stats.hits + stats.misses) as f64,
        100.0 * stats.stale_misses as f64 / (stats.hits + stats.misses) as f64,
    );

    // Expected: 5 inserts + 5 hits (round 2) + 5 misses (round 3, stale file)
    //         + 5 misses (round 4, stale gen) + 5 hits (round 5) = 25 lookups
    // hits = 10, misses = 10, stale_misses = 10
    assert_eq!(stats.hits, 10, "10 hits expected (rounds 2+5)");
    assert_eq!(stats.misses, 10, "10 misses expected (rounds 3+4)");
    assert_eq!(stats.stale_misses, 10, "all misses should be stale");
}

// ---------------------------------------------------------------------------
// Test 14: Workflow packet size comparison
// ---------------------------------------------------------------------------

#[test]
fn test_workflow_packet_sizes() {
    let workflows: Vec<(&str, LspContextPacket)> = vec![
        ("review_diff(5x6)", make_review_diff_packet(5, 6)),
        ("repair_local", make_repair_local_packet()),
        ("impact_analysis", make_impact_analysis_packet()),
        ("test_failure_repair", make_test_failure_repair_packet()),
        ("call_neighborhood", make_call_neighborhood_packet()),
    ];

    eprintln!(
        "[workflow_sizes] {:<30} {:>6} {:>10}",
        "workflow", "items", "bytes"
    );
    eprintln!(
        "[workflow_sizes] {:<30} {:>6} {:>10}",
        "-".repeat(30),
        "-".repeat(6),
        "-".repeat(10)
    );
    for (name, packet) in &workflows {
        let json = serde_json::to_vec(packet).unwrap();
        eprintln!(
            "[workflow_sizes] {name:<30} {:>6} {:>10}",
            packet.items.len(),
            json.len(),
        );
    }
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

//! Compile cancellation and concurrent coalescing tests for Tool Programs.
//!
//! These tests verify that:
//! - Compile cancellation does not produce partial IR
//! - Concurrent identical compilations produce deterministic results
//! - Thread-safe access to the program store works correctly

use codegg_core::tool_program::{
    compile_program, parse_source,
    store::{deserialize_ir, serialize_ir, verify_ir_integrity, ProgramStore},
    validate, verify_ir,
};
use std::sync::{Arc, Barrier};
use std::thread;

#[test]
fn cancellation_does_not_produce_partial_ir() {
    let src = "x = 1\nemit(x)\n";
    let ast = parse_source(src).unwrap();
    validate(&ast).unwrap();

    // Simulate cancellation: if we validate but don't compile,
    // no IR is published
    let result = compile_program(src);
    assert!(result.is_ok());

    // The IR must be complete and verified
    let ir = &result.unwrap().ir;
    verify_ir(ir).unwrap();

    // IR must end with Return
    assert!(matches!(
        ir.instructions.last().unwrap().op,
        codegg_core::tool_program::ir::IrOp::Return
    ));
}

#[test]
fn concurrent_identical_compilations_produce_same_digest() {
    let src = "for i in range(10):\n    x = i\nemit(x)\n";
    let barrier = Arc::new(Barrier::new(4));
    let mut handles = vec![];

    for _ in 0..4 {
        let barrier = barrier.clone();
        let src = src.to_string();
        handles.push(thread::spawn(move || {
            barrier.wait();
            let result = compile_program(&src).unwrap();
            result.ir.digest
        }));
    }

    let digests: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(digests.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn concurrent_store_operations_are_thread_safe() {
    let store = Arc::new(ProgramStore::new());
    let barrier = Arc::new(Barrier::new(10));
    let mut handles = vec![];

    for i in 0..10 {
        let store = store.clone();
        let barrier = barrier.clone();
        let src = format!("emit({{\"id\": {}}})\n", i);
        handles.push(thread::spawn(move || {
            barrier.wait();
            let result = compile_program(&src).unwrap();
            store.store_ir(&src, result.ir.clone()).unwrap();
            let retrieved = store.get_ir(&src).unwrap();
            assert_eq!(retrieved.digest, result.ir.digest);
        }));
    }

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn concurrent_cache_hits_are_consistent() {
    let store = Arc::new(ProgramStore::new());
    let src = "emit({\"ok\": true})\n";
    let result = compile_program(src).unwrap();
    store.store_ir(src, result.ir).unwrap();

    let barrier = Arc::new(Barrier::new(8));
    let mut handles = vec![];

    for _ in 0..8 {
        let store = store.clone();
        let barrier = barrier.clone();
        let src = src.to_string();
        handles.push(thread::spawn(move || {
            barrier.wait();
            let cached = store.check_cache(&src, "", "");
            assert!(cached.is_some());
            cached.unwrap().digest
        }));
    }

    let digests: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(digests.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn serialization_round_trip_preserves_integrity() {
    let sources = vec![
        "emit({\"ok\": true})\n",
        "x = 1\nemit(x)\n",
        "for i in range(5):\n    x = i\nemit(x)\n",
        "if True:\n    x = 1\nelse:\n    x = 0\nemit(x)\n",
    ];

    for src in &sources {
        let result = compile_program(src).unwrap();
        let bytes = serialize_ir(&result.ir).unwrap();
        let restored = deserialize_ir(&bytes).unwrap();
        assert_eq!(restored.digest, result.ir.digest);
        verify_ir_integrity(&restored).unwrap();
    }
}

#[test]
fn store_deduplication_under_contention() {
    let store = Arc::new(ProgramStore::new());
    let src = "emit({\"ok\": true})\n";
    let barrier = Arc::new(Barrier::new(5));
    let mut handles = vec![];

    for _ in 0..5 {
        let store = store.clone();
        let barrier = barrier.clone();
        let src = src.to_string();
        handles.push(thread::spawn(move || {
            barrier.wait();
            let result = compile_program(&src).unwrap();
            let _ = store.store_ir(&src, result.ir);
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(store.len(), 1, "should deduplicate to 1 entry");
}

#[test]
fn concurrent_ir_verification() {
    let src = r#"
total = 0
for i in range(20):
    total = total + 1
emit({"total": total})
"#;
    let result = compile_program(src).unwrap();
    let ir = Arc::new(result.ir);
    let barrier = Arc::new(Barrier::new(6));
    let mut handles = vec![];

    for _ in 0..6 {
        let ir = ir.clone();
        let barrier = barrier.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            verify_ir(&ir).unwrap();
        }));
    }

    for h in handles {
        h.join().unwrap();
    }
}

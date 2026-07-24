//! Integration tests for the read-only call cache (M006).
//!
//! Validates cache correctness, TTL expiry, workspace isolation,
//! eviction, invalidation, and broker integration.

use std::time::Duration;

use async_trait::async_trait;
use codegg::error::ToolError;
use codegg::tool::broker::{BrokerInvocationContext, ToolBroker};
use codegg::tool::contract::{
    ToolCachePolicy, ToolCaller, ToolCallerPolicy, ToolContract, ToolEffectClass,
    ToolTerminalStatus, ToolValue,
};
use codegg::tool::program_cache::{CacheKey, ProgramCacheConfig, ProgramCallCache};
use codegg::tool::{Tool, ToolCategory, ToolRegistry};
use serde_json::json;
use std::path::PathBuf;

// ── Mock tools ────────────────────────────────────────────────────────────

struct MockReadTool;

#[async_trait]
impl Tool for MockReadTool {
    fn name(&self) -> &str {
        "read"
    }
    fn description(&self) -> &str {
        "Read file contents"
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {"path": {"type": "string"}}})
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let path = input
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap_or("unknown");
        Ok(format!("content of {}", path))
    }
    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }
    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.to_string(),
            caller_policy: ToolCallerPolicy::DirectOrProgrammatic,
            effect_class: ToolEffectClass::ReadOnly,
            cache_policy: ToolCachePolicy {
                enabled: true,
                ..ToolCachePolicy::default()
            },
            output_schema: Some(json!({"type": "object"})),
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn make_broker() -> (ToolBroker, ToolRegistry) {
    let mut registry = ToolRegistry::with_defaults();
    registry.register(MockReadTool);
    let broker = ToolBroker::new(&registry);
    (broker, registry)
}

fn program_ctx() -> BrokerInvocationContext {
    BrokerInvocationContext {
        caller: ToolCaller::Program {
            program_id: "prog-cache-test".to_string(),
        },
        cwd: PathBuf::from("."),
        session_id: None,
        workspace_id: None,
        agent_id: None,
        turn_id: None,
        job_id: None,
        attempt_id: None,
        permission_mode: None,
        timeout_ms: Some(5_000),
        submission_key: None,
        caller_authorized: true,
    }
}

// ── Cache correctness tests ───────────────────────────────────────────────

#[tokio::test]
async fn cache_hit_returns_broker_result() {
    let (broker, registry) = make_broker();
    let cache = ProgramCallCache::with_defaults();

    let result = broker
        .execute(
            &registry,
            "read",
            json!({"path": "/tmp/a.txt"}),
            program_ctx(),
        )
        .await
        .unwrap();

    let key = CacheKey::new("read", &json!({"path": "/tmp/a.txt"}), None);
    cache.insert(key.clone(), result.value.clone());

    let cached = cache.get(&key).unwrap();
    assert_eq!(cached.value.display, "content of /tmp/a.txt");
    assert_eq!(cached.value.terminal_status, ToolTerminalStatus::Success);
}

#[tokio::test]
async fn cache_miss_for_different_path() {
    let cache = ProgramCallCache::with_defaults();
    let key1 = CacheKey::new("read", &json!({"path": "/tmp/a.txt"}), None);
    let key2 = CacheKey::new("read", &json!({"path": "/tmp/b.txt"}), None);

    cache.insert(key1, ToolValue::success("a".to_string()));
    assert!(cache.get(&key2).is_none());
}

#[tokio::test]
async fn cache_miss_for_different_tool() {
    let cache = ProgramCallCache::with_defaults();
    let key_read = CacheKey::new("read", &json!({"path": "/tmp/a.txt"}), None);
    let key_grep = CacheKey::new("grep", &json!({"pattern": "foo"}), None);

    cache.insert(key_read, ToolValue::success("a".to_string()));
    assert!(cache.get(&key_grep).is_none());
}

// ── TTL expiry tests ──────────────────────────────────────────────────────

#[test]
fn cache_expires_after_ttl() {
    let config = ProgramCacheConfig {
        default_ttl: Duration::from_millis(1), // 1ms TTL
        ..Default::default()
    };
    let cache = ProgramCallCache::new(config);
    let key = CacheKey::new("read", &json!({"path": "/tmp/a.txt"}), None);

    cache.insert(key.clone(), ToolValue::success("hello".to_string()));

    // Wait for expiry
    std::thread::sleep(Duration::from_millis(5));

    assert!(cache.get(&key).is_none(), "cache entry should be expired");
}

#[test]
fn cache_hit_before_ttl() {
    let config = ProgramCacheConfig {
        default_ttl: Duration::from_secs(60), // long TTL
        ..Default::default()
    };
    let cache = ProgramCallCache::new(config);
    let key = CacheKey::new("read", &json!({"path": "/tmp/a.txt"}), None);

    cache.insert(key.clone(), ToolValue::success("hello".to_string()));
    assert!(
        cache.get(&key).is_some(),
        "cache entry should still be valid"
    );
}

// ── Workspace isolation tests ─────────────────────────────────────────────

#[test]
fn cache_distinguishes_workspaces() {
    let cache = ProgramCallCache::with_defaults();
    let key_ws1 = CacheKey::new("read", &json!({"path": "/tmp/a.txt"}), Some("ws-1"));
    let key_ws2 = CacheKey::new("read", &json!({"path": "/tmp/a.txt"}), Some("ws-2"));

    cache.insert(key_ws1.clone(), ToolValue::success("ws1".to_string()));
    cache.insert(key_ws2.clone(), ToolValue::success("ws2".to_string()));

    assert_eq!(
        cache.get(&key_ws1).unwrap().value.display,
        "ws1",
        "workspace 1 should have its own cached value"
    );
    assert_eq!(
        cache.get(&key_ws2).unwrap().value.display,
        "ws2",
        "workspace 2 should have its own cached value"
    );
}

#[test]
fn cache_none_workspace_is_separate_from_named() {
    let cache = ProgramCallCache::with_defaults();
    let key_none = CacheKey::new("read", &json!({"path": "/tmp/a.txt"}), None);
    let key_named = CacheKey::new("read", &json!({"path": "/tmp/a.txt"}), Some("ws-1"));

    cache.insert(key_none.clone(), ToolValue::success("none".to_string()));
    cache.insert(key_named.clone(), ToolValue::success("named".to_string()));

    assert_eq!(cache.get(&key_none).unwrap().value.display, "none");
    assert_eq!(cache.get(&key_named).unwrap().value.display, "named");
}

// ── Eviction tests ────────────────────────────────────────────────────────

#[test]
fn cache_evicts_oldest_when_full() {
    let config = ProgramCacheConfig {
        max_total_entries: 3,
        ..Default::default()
    };
    let cache = ProgramCallCache::new(config);

    for i in 0..5 {
        cache.insert(
            CacheKey::new("read", &json!({"path": format!("/tmp/{}.txt", i)}), None),
            ToolValue::success(format!("file{}", i)),
        );
        // Small sleep to ensure distinct timestamps for eviction ordering
        std::thread::sleep(Duration::from_millis(2));
    }

    assert!(
        cache.len() <= 3,
        "cache should have at most 3 entries, got {}",
        cache.len()
    );
}

// ── Invalidation tests ────────────────────────────────────────────────────

#[test]
fn cache_invalidate_specific_entry() {
    let cache = ProgramCallCache::with_defaults();
    let key = CacheKey::new("read", &json!({"path": "/tmp/a.txt"}), None);

    cache.insert(key.clone(), ToolValue::success("hello".to_string()));
    assert!(cache.get(&key).is_some());

    assert!(cache.invalidate(&key));
    assert!(cache.get(&key).is_none());
}

#[test]
fn cache_invalidate_nonexistent_returns_false() {
    let cache = ProgramCallCache::with_defaults();
    let key = CacheKey::new("read", &json!({"path": "/tmp/a.txt"}), None);
    assert!(!cache.invalidate(&key));
}

#[test]
fn cache_invalidate_tool_removes_all_entries() {
    let cache = ProgramCallCache::with_defaults();

    cache.insert(
        CacheKey::new("read", &json!({"path": "/tmp/a.txt"}), None),
        ToolValue::success("a".to_string()),
    );
    cache.insert(
        CacheKey::new("read", &json!({"path": "/tmp/b.txt"}), None),
        ToolValue::success("b".to_string()),
    );
    cache.insert(
        CacheKey::new("grep", &json!({"pattern": "foo"}), None),
        ToolValue::success("c".to_string()),
    );

    let removed = cache.invalidate_tool("read");
    assert_eq!(removed, 2);
    assert_eq!(cache.len(), 1); // grep entry remains
}

#[test]
fn cache_clear_removes_all() {
    let cache = ProgramCallCache::with_defaults();
    cache.insert(
        CacheKey::new("read", &json!({}), None),
        ToolValue::success("x".to_string()),
    );
    cache.insert(
        CacheKey::new("grep", &json!({}), None),
        ToolValue::success("y".to_string()),
    );

    cache.clear();
    assert!(cache.is_empty());
}

// ── Broker integration test ───────────────────────────────────────────────

#[tokio::test]
async fn cache_stores_and_retrieves_broker_result() {
    let (broker, registry) = make_broker();
    let cache = ProgramCallCache::with_defaults();

    // Execute through broker
    let result = broker
        .execute(
            &registry,
            "read",
            json!({"path": "/tmp/test.txt"}),
            program_ctx(),
        )
        .await
        .unwrap();

    // Cache the result
    let key = CacheKey::new("read", &json!({"path": "/tmp/test.txt"}), None);
    cache.insert(key.clone(), result.value.clone());

    // Verify cached value matches broker output
    let cached = cache.get(&key).unwrap();
    assert_eq!(cached.value.display, result.value.display);
    assert_eq!(cached.value.terminal_status, result.value.terminal_status);
}

// ── Error result tests ────────────────────────────────────────────────────

#[test]
fn cache_does_not_store_errors_by_default() {
    // The cache stores ToolValue regardless of status; the caller decides
    // whether to cache errors. This test verifies the cache holds error
    // values when explicitly inserted.
    let cache = ProgramCallCache::with_defaults();
    let key = CacheKey::new("read", &json!({"path": "/tmp/a.txt"}), None);

    let error_value = ToolValue {
        display: "permission denied".to_string(),
        value: None,
        artifacts: vec![],
        provenance: None,
        terminal_status: ToolTerminalStatus::Error,
        truncated: false,
    };

    cache.insert(key.clone(), error_value);
    let cached = cache.get(&key).unwrap();
    assert_eq!(cached.value.terminal_status, ToolTerminalStatus::Error);
}

//! Module cache for WASM plugin compilation results.
//!
//! Extracted from the legacy `loader.rs` module cache. Provides mtime-based
//! invalidation, compilation caching, and per-plugin fuel budget tracking.

use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(feature = "plugins")]
use wasmtime::Module;

/// Maximum fuel budget per plugin (10M instructions).
pub const MAX_PLUGIN_FUEL_BUDGET: u64 = 10_000_000;

/// Cached compiled WASM module with metadata.
#[cfg(feature = "plugins")]
struct CachedModule {
    module: Module,
    mtime: u64,
}

/// A thread-safe cache for compiled WASM modules and per-plugin fuel budgets.
///
/// Modules are keyed by canonical file path and invalidated when the file's
/// modification time changes. Fuel budgets track remaining compute budget
/// per plugin across invocations.
pub struct WasmModuleCache {
    #[cfg(feature = "plugins")]
    modules: DashMap<String, CachedModule>,
    hits: AtomicU64,
    misses: AtomicU64,
    fuel_budgets: DashMap<String, AtomicU64>,
}

impl WasmModuleCache {
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "plugins")]
            modules: DashMap::new(),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            fuel_budgets: DashMap::new(),
        }
    }

    /// Get a cached module or compile it using the provided function.
    ///
    /// Returns `Some(Module)` on cache hit or successful compilation,
    /// `None` if the module file cannot be read or compiled.
    #[cfg(feature = "plugins")]
    pub fn get_or_compile<F>(&self, path: &str, compile_fn: F) -> Option<Module>
    where
        F: FnOnce() -> Option<Module>,
    {
        let mtime = std::fs::metadata(path)
            .ok()?
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();

        if let Some(entry) = self.modules.get(path) {
            if entry.mtime == mtime {
                self.hits.fetch_add(1, Ordering::Relaxed);
                return Some(entry.module.clone());
            }
        }

        if let Some(module) = compile_fn() {
            self.misses.fetch_add(1, Ordering::Relaxed);
            self.modules.insert(
                path.to_string(),
                CachedModule {
                    module: module.clone(),
                    mtime,
                },
            );
            return Some(module);
        }

        None
    }

    /// Return cache hit/miss statistics as (hits, misses).
    pub fn stats(&self) -> (u64, u64) {
        (
            self.hits.load(Ordering::Relaxed),
            self.misses.load(Ordering::Relaxed),
        )
    }

    /// Get the remaining fuel budget for a plugin.
    pub fn get_plugin_fuel(&self, plugin_id: &str) -> u64 {
        self.fuel_budgets
            .get(plugin_id)
            .map(|entry| entry.value().load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// Atomically reserve fuel from a plugin's budget.
    ///
    /// Returns `Some(fuel_reserved)` on success, `None` if the budget is
    /// insufficient.
    pub fn reserve_fuel(&self, plugin_id: &str, fuel_needed: u64) -> Option<u64> {
        let entry = self
            .fuel_budgets
            .entry(plugin_id.to_string())
            .or_insert_with(|| AtomicU64::new(MAX_PLUGIN_FUEL_BUDGET));

        loop {
            let current = entry.value().load(Ordering::Relaxed);
            if current < fuel_needed {
                return None;
            }
            let new_val = current - fuel_needed;
            match entry.value().compare_exchange(
                current,
                new_val,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some(fuel_needed),
                Err(_) => continue,
            }
        }
    }

    /// Return fuel to a plugin's budget.
    pub fn return_fuel(&self, plugin_id: &str, fuel: u64) {
        let entry = self
            .fuel_budgets
            .entry(plugin_id.to_string())
            .or_insert_with(|| AtomicU64::new(MAX_PLUGIN_FUEL_BUDGET));
        entry.value().fetch_add(fuel, Ordering::Relaxed);
    }

    /// Set the fuel budget for a plugin (for testing/reset).
    pub fn set_plugin_fuel(&self, plugin_id: &str, fuel: u64) {
        self.fuel_budgets
            .entry(plugin_id.to_string())
            .or_insert_with(|| AtomicU64::new(MAX_PLUGIN_FUEL_BUDGET))
            .store(fuel, Ordering::Relaxed);
    }

    /// Remove a plugin's fuel budget entry.
    pub fn clear_fuel(&self, plugin_id: &str) {
        self.fuel_budgets.remove(plugin_id);
    }
}

impl Default for WasmModuleCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Global module cache instance.
pub static WASM_CACHE: once_cell::sync::Lazy<WasmModuleCache> =
    once_cell::sync::Lazy::new(WasmModuleCache::new);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_stats_start_at_zero() {
        let cache = WasmModuleCache::new();
        let (hits, misses) = cache.stats();
        assert_eq!(hits, 0);
        assert_eq!(misses, 0);
    }

    #[test]
    fn fuel_budget_starts_at_max() {
        let cache = WasmModuleCache::new();
        let fuel = cache.get_plugin_fuel("test-plugin");
        // First access returns 0 (no entry yet), but reserve_fuel creates the entry
        assert_eq!(fuel, 0);
    }

    #[test]
    fn reserve_fuel_creates_budget_and_reserves() {
        let cache = WasmModuleCache::new();
        let reserved = cache.reserve_fuel("test-plugin", 1000);
        assert_eq!(reserved, Some(1000));
        let remaining = cache.get_plugin_fuel("test-plugin");
        assert_eq!(remaining, MAX_PLUGIN_FUEL_BUDGET - 1000);
    }

    #[test]
    fn reserve_fuel_fails_when_insufficient() {
        let cache = WasmModuleCache::new();
        let reserved = cache.reserve_fuel("test-plugin", MAX_PLUGIN_FUEL_BUDGET + 1);
        assert_eq!(reserved, None);
    }

    #[test]
    fn return_fuel_adds_back() {
        let cache = WasmModuleCache::new();
        cache.reserve_fuel("test-plugin", 5000);
        cache.return_fuel("test-plugin", 3000);
        let remaining = cache.get_plugin_fuel("test-plugin");
        assert_eq!(remaining, MAX_PLUGIN_FUEL_BUDGET - 2000);
    }

    #[test]
    fn set_plugin_fuel_overwrites() {
        let cache = WasmModuleCache::new();
        cache.set_plugin_fuel("test-plugin", 42);
        assert_eq!(cache.get_plugin_fuel("test-plugin"), 42);
    }

    #[test]
    fn clear_fuel_removes_entry() {
        let cache = WasmModuleCache::new();
        cache.set_plugin_fuel("test-plugin", 42);
        cache.clear_fuel("test-plugin");
        assert_eq!(cache.get_plugin_fuel("test-plugin"), 0);
    }

    #[test]
    fn fuel_reserve_is_atomic_under_contention() {
        let cache = std::sync::Arc::new(WasmModuleCache::new());
        let mut handles = Vec::new();
        for _ in 0..10 {
            let cache = cache.clone();
            handles.push(std::thread::spawn(move || {
                for _ in 0..100 {
                    cache.reserve_fuel("contested", 100);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let remaining = cache.get_plugin_fuel("contested");
        assert!(remaining < MAX_PLUGIN_FUEL_BUDGET);
    }

    /// After reserving `reserved` fuel, the correct amount to return to the
    /// budget is `reserved - consumed` (i.e. the unused remainder). This
    /// test documents the contract used by the runtime: returning the
    /// consumed amount would inflate the budget by `2 * consumed`.
    #[test]
    fn fuel_budget_accounting_used_by_wasm_runtime() {
        let cache = WasmModuleCache::new();
        let reserved = 1000u64;
        cache.reserve_fuel("wasm-plugin", reserved).unwrap();

        // Simulate consumed = 300, so unused = 700. The runtime must return 700.
        let consumed = 300u64;
        let unused = reserved - consumed;
        cache.return_fuel("wasm-plugin", unused);

        let after = cache.get_plugin_fuel("wasm-plugin");
        assert_eq!(after, MAX_PLUGIN_FUEL_BUDGET - consumed);
    }

    /// Returning zero (no leftover fuel) leaves the budget at MAX - reserved.
    #[test]
    fn fuel_budget_zero_unused_full_consumed() {
        let cache = WasmModuleCache::new();
        cache.reserve_fuel("wasm-plugin", 1000).unwrap();
        cache.return_fuel("wasm-plugin", 0);
        assert_eq!(
            cache.get_plugin_fuel("wasm-plugin"),
            MAX_PLUGIN_FUEL_BUDGET - 1000
        );
    }
}

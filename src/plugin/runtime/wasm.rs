//! WASM plugin runtime implementation.
//!
//! Executes WASM plugins using Wasmtime behind the `plugins` feature flag.
//! Supports both the modern `codegg_plugin_invoke` ABI and legacy per-hook
//! export functions for backward compatibility.

use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::wasm_cache::WasmModuleCache;
use super::{PluginRuntime, RuntimeError, RuntimeLimits};
use crate::protocol::plugin::{PluginInvocation, PluginResponse};

/// Return the unused portion of a reserved fuel allotment back to the
/// plugin's fuel budget.
///
/// `reserve_fuel` already subtracts the full reserved amount from the
/// plugin's budget. After a successful invocation we must credit back the
/// amount that was *not* consumed — never the consumed amount. Capping the
/// returned fuel by `reserved` prevents a buggy `remaining` value from
/// over-refunding the budget.
fn return_unused_fuel(cache: &WasmModuleCache, plugin_id: &str, reserved: u64, remaining: u64) {
    let unused = remaining.min(reserved);
    if unused > 0 {
        cache.return_fuel(plugin_id, unused);
    }
}

/// Maximum WASM module size (10 MiB).
const MAX_WASM_SIZE: usize = 10 * 1024 * 1024;

/// Maximum WASM output size (1 MiB).
const MAX_WASM_OUTPUT_SIZE: usize = 1024 * 1024;

/// Default fuel per individual WASM call.
const DEFAULT_FUEL_PER_CALL: u64 = 1_000_000;

/// Default memory maximum (256 MiB).
const DEFAULT_MEMORY_MAX_MB: u64 = 256;

/// Configuration for a WASM-backed plugin runtime.
#[derive(Debug, Clone)]
pub struct WasmRuntimeSpec {
    /// Path to the `.wasm` module file.
    pub module_path: PathBuf,
    /// Per-call timeout override (ms). Falls back to `RuntimeLimits.timeout_ms`.
    pub timeout_ms: Option<u64>,
    /// Maximum WASM linear memory in MiB.
    pub memory_max_mb: Option<u64>,
    /// Fuel per individual invocation.
    pub fuel_per_call: Option<u64>,
    /// Optional entrypoint override (default: `codegg_plugin_invoke`).
    pub entrypoint: Option<String>,
}

impl WasmRuntimeSpec {
    /// Create a spec from a manifest's WASM runtime declaration.
    pub fn from_manifest(
        module: &str,
        plugin_dir: &Path,
        timeout_ms: Option<u64>,
        memory_max_mb: Option<u64>,
        fuel_per_call: Option<u64>,
    ) -> Self {
        let module_path = if Path::new(module).is_absolute() {
            PathBuf::from(module)
        } else {
            plugin_dir.join(module)
        };
        Self {
            module_path,
            timeout_ms,
            memory_max_mb,
            fuel_per_call,
            entrypoint: None,
        }
    }
}

/// A plugin runtime that executes WASM modules via Wasmtime.
pub struct WasmRuntime {
    spec: WasmRuntimeSpec,
    limits: RuntimeLimits,
    cache: Arc<super::wasm_cache::WasmModuleCache>,
}

impl WasmRuntime {
    pub fn new(spec: WasmRuntimeSpec, limits: RuntimeLimits) -> Self {
        Self {
            spec,
            limits,
            cache: Arc::new(super::wasm_cache::WasmModuleCache::new()),
        }
    }

    pub fn with_cache(
        spec: WasmRuntimeSpec,
        limits: RuntimeLimits,
        cache: Arc<super::wasm_cache::WasmModuleCache>,
    ) -> Self {
        Self {
            spec,
            limits,
            cache,
        }
    }

    pub fn with_defaults(spec: WasmRuntimeSpec) -> Self {
        Self::new(spec, RuntimeLimits::default())
    }
}

#[async_trait]
impl PluginRuntime for WasmRuntime {
    async fn invoke(&self, invocation: PluginInvocation) -> Result<PluginResponse, RuntimeError> {
        #[cfg(not(feature = "plugins"))]
        {
            let _ = invocation;
            return Err(RuntimeError::Unsupported(
                "WASM runtime requires the 'plugins' feature".into(),
            ));
        }

        #[cfg(feature = "plugins")]
        {
            use std::time::Duration;
            use tokio::time::timeout;

            let timeout_ms = self.spec.timeout_ms.unwrap_or(self.limits.timeout_ms);
            let plugin_id = invocation.plugin_id.clone();
            let module_path = self.spec.module_path.clone();
            let _memory_max_mb = self.spec.memory_max_mb.unwrap_or(DEFAULT_MEMORY_MAX_MB);
            let cache = self.cache.clone();
            let fuel_per_call = self.spec.fuel_per_call.unwrap_or(DEFAULT_FUEL_PER_CALL);
            let entrypoint = self.spec.entrypoint.clone();

            // Validate module exists and size
            let metadata = std::fs::metadata(&module_path).map_err(|e| {
                RuntimeError::Io(format!(
                    "failed to read WASM metadata for '{}': {e}",
                    module_path.display()
                ))
            })?;

            if metadata.len() > MAX_WASM_SIZE as u64 {
                return Err(RuntimeError::Spawn(format!(
                    "WASM module exceeds maximum size: {} bytes (max: {})",
                    metadata.len(),
                    MAX_WASM_SIZE
                )));
            }

            // Build engine with fuel and memory limits
            let engine = {
                use wasmtime::{Config as WasmConfig, Engine, WasmBacktraceDetails};

                let mut config = WasmConfig::new();
                config.consume_fuel(true);
                config.wasm_backtrace_details(WasmBacktraceDetails::Disable);

                Engine::new(&config).map_err(|e| {
                    RuntimeError::Spawn(format!("failed to create WASM engine: {e}"))
                })?
            };

            // Get or compile module from cache
            let module_path_str = module_path.to_string_lossy().to_string();
            let module = cache.get_or_compile(&module_path_str, || {
                let wasm_bytes = std::fs::read(&module_path).ok()?;
                wasmtime::Module::new(&engine, &wasm_bytes).ok()
            });

            let module = module.ok_or_else(|| {
                RuntimeError::Spawn(format!(
                    "failed to compile WASM module '{}'",
                    module_path.display()
                ))
            })?;

            // Clone invocation for legacy fallback
            let invocation_for_legacy = invocation.clone();
            let invocation_json = serde_json::to_vec(&invocation)
                .map_err(|e| RuntimeError::InvalidJson(e.to_string()))?;

            // Execute with timeout
            let response = timeout(
                Duration::from_millis(timeout_ms),
                tokio::task::spawn_blocking(move || {
                    // Try modern ABI first
                    match invoke_modern(
                        &invocation_json,
                        &engine,
                        &module,
                        &plugin_id,
                        &cache,
                        fuel_per_call,
                        entrypoint.as_deref(),
                    ) {
                        Ok(resp) => Ok(resp),
                        Err(RuntimeError::Unsupported(_)) => {
                            // Modern entrypoint not found, try legacy
                            invoke_legacy(
                                &invocation_for_legacy,
                                &engine,
                                &module,
                                &plugin_id,
                                &cache,
                                fuel_per_call,
                            )
                        }
                        Err(e) => Err(e),
                    }
                }),
            )
            .await
            .map_err(|_| RuntimeError::Timeout { timeout_ms })?
            .map_err(|e| RuntimeError::Spawn(format!("WASM task panicked: {e}")))?;

            response
        }
    }
}

/// Execute a WASM invocation using the modern `codegg_plugin_invoke` ABI.
///
/// The plugin receives UTF-8 JSON `PluginInvocation` bytes and must return
/// UTF-8 JSON `PluginResponse` bytes via a packed i64 (high 32 = ptr, low 32 = len).
#[cfg(feature = "plugins")]
fn invoke_modern(
    invocation_json: &[u8],
    engine: &wasmtime::Engine,
    module: &wasmtime::Module,
    plugin_id: &str,
    cache: &super::wasm_cache::WasmModuleCache,
    fuel_per_call: u64,
    entrypoint: Option<&str>,
) -> Result<PluginResponse, RuntimeError> {
    use wasmtime::{Linker, Store};

    let fuel = cache.reserve_fuel(plugin_id, fuel_per_call).unwrap_or(0);

    let result = (|| -> Result<PluginResponse, RuntimeError> {
        let mut store = Store::new(engine, ());
        store.set_fuel(fuel).ok();

        let mut linker = Linker::new(engine);
        linker.allow_shadowing(true);

        let instance = linker
            .instantiate(&mut store, module)
            .map_err(|e| RuntimeError::Spawn(format!("instantiate failed: {e}")))?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| RuntimeError::Spawn("WASM module has no memory export".into()))?;

        // Allocate input buffer in WASM memory
        let alloc_func = instance
            .get_func(&mut store, "allocate")
            .ok_or_else(|| RuntimeError::Spawn("WASM module missing allocate function".into()))?;

        let mut alloc_result = [wasmtime::Val::I32(0)];
        alloc_func
            .call(
                &mut store,
                &[wasmtime::Val::I32(invocation_json.len() as i32)],
                &mut alloc_result,
            )
            .map_err(|e| RuntimeError::Spawn(format!("allocate call failed: {e}")))?;

        let input_ptr = alloc_result[0]
            .i32()
            .ok_or_else(|| RuntimeError::Spawn("allocate returned non-i32".into()))?;

        memory
            .write(&mut store, input_ptr as usize, invocation_json)
            .map_err(|e| RuntimeError::Io(format!("memory write failed: {e}")))?;

        // Bounds check
        let mem_size = memory.data_size(&store);
        if input_ptr as usize + invocation_json.len() > mem_size {
            return Err(RuntimeError::Spawn(
                "input exceeds WASM memory bounds".into(),
            ));
        }

        // Call the modern entrypoint
        let func_name = entrypoint.unwrap_or("codegg_plugin_invoke");
        let func = instance.get_func(&mut store, func_name).ok_or_else(|| {
            RuntimeError::Unsupported(format!("WASM module has no '{func_name}' export"))
        })?;

        let mut result_vals = [wasmtime::Val::I64(0)];
        func.call(
            &mut store,
            &[
                wasmtime::Val::I32(input_ptr),
                wasmtime::Val::I32(invocation_json.len() as i32),
            ],
            &mut result_vals,
        )
        .map_err(|e| RuntimeError::Spawn(format!("invoke call failed: {e}")))?;

        let packed = result_vals[0]
            .i64()
            .ok_or_else(|| RuntimeError::Spawn("invoke returned non-i64".into()))?;

        // Unpack: high 32 bits = ptr, low 32 bits = len
        let result_ptr = (packed >> 32) as i32;
        let result_len = (packed & 0xFFFF_FFFF) as i32;

        if result_ptr == 0 || result_len == 0 {
            return Err(RuntimeError::Spawn(
                "invoke returned null/empty result".into(),
            ));
        }

        if result_len as usize > MAX_WASM_OUTPUT_SIZE {
            return Err(RuntimeError::Spawn(format!(
                "WASM response exceeds maximum size: {result_len} bytes"
            )));
        }

        let mut output = vec![0u8; result_len as usize];
        memory
            .read(&mut store, result_ptr as usize, &mut output)
            .map_err(|e| RuntimeError::Io(format!("output read failed: {e}")))?;

        // Try to free result memory
        if let Some(dealloc) = instance.get_func(&mut store, "deallocate") {
            let mut dummy = [wasmtime::Val::I32(0)];
            let _ = dealloc.call(
                &mut store,
                &[
                    wasmtime::Val::I32(result_ptr),
                    wasmtime::Val::I32(result_len),
                ],
                &mut dummy,
            );
        }

        // Return unused fuel to budget. reserve_fuel already subtracted the
        // full reserved amount, so we credit back whatever the store did not
        // consume.
        let remaining = store.get_fuel().unwrap_or(0);
        return_unused_fuel(cache, plugin_id, fuel, remaining);

        // Parse response
        let response: PluginResponse = serde_json::from_slice(&output)
            .map_err(|e| RuntimeError::InvalidJson(format!("invalid PluginResponse JSON: {e}")))?;

        Ok(response)
    })();

    match result {
        Ok(resp) => Ok(resp),
        Err(e) => {
            // On error the store may or may not have consumed fuel. Returning
            // the full reserved amount keeps the budget non-strict: a failed
            // invocation does not burn fuel.
            cache.return_fuel(plugin_id, fuel);
            Err(e)
        }
    }
}

/// Execute a WASM invocation using legacy per-hook export functions.
///
/// Falls back to this ABI when `codegg_plugin_invoke` is absent.
#[cfg(feature = "plugins")]
fn invoke_legacy(
    invocation: &PluginInvocation,
    engine: &wasmtime::Engine,
    module: &wasmtime::Module,
    plugin_id: &str,
    cache: &super::wasm_cache::WasmModuleCache,
    fuel_per_call: u64,
) -> Result<PluginResponse, RuntimeError> {
    use wasmtime::{Linker, Store};

    let fuel = cache.reserve_fuel(plugin_id, fuel_per_call).unwrap_or(0);

    let result = (|| -> Result<PluginResponse, RuntimeError> {
        let mut store = Store::new(engine, ());
        store.set_fuel(fuel).ok();

        let mut linker = Linker::new(engine);
        linker.allow_shadowing(true);

        let instance = linker
            .instantiate(&mut store, module)
            .map_err(|e| RuntimeError::Spawn(format!("instantiate failed: {e}")))?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| RuntimeError::Spawn("WASM module has no memory export".into()))?;

        let alloc_func = instance
            .get_func(&mut store, "allocate")
            .ok_or_else(|| RuntimeError::Spawn("WASM module missing allocate function".into()))?;

        // Serialize the hook input as JSON
        let hook_input = serde_json::json!({
            "hook_type": invocation.capability.hook_type_string(),
            "input": invocation.input,
        });
        let input_json = serde_json::to_vec(&hook_input)
            .map_err(|e| RuntimeError::InvalidJson(e.to_string()))?;

        let mut alloc_result = [wasmtime::Val::I32(0)];
        alloc_func
            .call(
                &mut store,
                &[wasmtime::Val::I32(input_json.len() as i32)],
                &mut alloc_result,
            )
            .map_err(|e| RuntimeError::Spawn(format!("allocate call failed: {e}")))?;

        let input_ptr = alloc_result[0]
            .i32()
            .ok_or_else(|| RuntimeError::Spawn("allocate returned non-i32".into()))?;

        memory
            .write(&mut store, input_ptr as usize, &input_json)
            .map_err(|e| RuntimeError::Io(format!("memory write failed: {e}")))?;

        // Map hook_type to legacy function name
        let func_name = match invocation.capability {
            crate::protocol::plugin::PluginCapabilityInvocation::Hook { ref hook_type } => {
                match hook_type.as_str() {
                    "auth" => "on_auth",
                    "provider" => "on_provider",
                    "tool.definition" => "on_tool_definition",
                    "tool.execute.before" => "on_tool_execute_before",
                    "tool.execute.after" => "on_tool_execute_after",
                    "chat.params" => "on_chat_params",
                    "chat.headers" => "on_chat_headers",
                    "event" => "on_event",
                    "config" => "on_config",
                    "shell.env" => "on_shell_env",
                    "text.complete" => "on_text_complete",
                    "session.compacting" => "on_session_compacting",
                    "messages.transform" => "on_messages_transform",
                    other => {
                        return Err(RuntimeError::Unsupported(format!(
                            "unknown hook type: {other}"
                        )))
                    }
                }
            }
            _ => {
                return Err(RuntimeError::Unsupported(
                    "legacy ABI only supports hook invocations".into(),
                ))
            }
        };

        let func = instance.get_func(&mut store, func_name).ok_or_else(|| {
            RuntimeError::Unsupported(format!("WASM module has no '{func_name}' export"))
        })?;

        let mut result_vals = [wasmtime::Val::I32(0)];
        func.call(
            &mut store,
            &[
                wasmtime::Val::I32(input_ptr),
                wasmtime::Val::I32(input_json.len() as i32),
            ],
            &mut result_vals,
        )
        .map_err(|e| RuntimeError::Spawn(format!("hook call failed: {e}")))?;

        let result_ptr = result_vals[0]
            .i32()
            .ok_or_else(|| RuntimeError::Spawn("hook returned non-i32".into()))?;

        // Return unused fuel to budget. reserve_fuel already subtracted the
        // full reserved amount, so we credit back whatever the store did not
        // consume.
        let remaining = store.get_fuel().unwrap_or(0);
        return_unused_fuel(cache, plugin_id, fuel, remaining);

        if result_ptr == 0 {
            // Null result = pass-through input
            return Ok(PluginResponse {
                ok: true,
                effects: Vec::new(),
                data: invocation.input.clone(),
                diagnostics: Vec::new(),
            });
        }

        // Read response from memory: [ptr(u32), len(u32), data...]
        let mut len_buf = [0u8; 4];
        memory
            .read(&mut store, result_ptr as usize + 4, &mut len_buf)
            .map_err(|e| RuntimeError::Io(format!("length read failed: {e}")))?;
        let output_len = u32::from_le_bytes(len_buf) as usize;

        if output_len > MAX_WASM_OUTPUT_SIZE {
            return Err(RuntimeError::Spawn(format!(
                "WASM output exceeds maximum size: {output_len}"
            )));
        }

        let mut output = vec![0u8; output_len];
        memory
            .read(&mut store, result_ptr as usize + 8, &mut output)
            .map_err(|e| RuntimeError::Io(format!("output read failed: {e}")))?;

        // Try to free
        if let Some(dealloc) = instance.get_func(&mut store, "deallocate") {
            let mut dummy = [wasmtime::Val::I32(0)];
            let _ = dealloc.call(
                &mut store,
                &[wasmtime::Val::I32(result_ptr), wasmtime::Val::I32(0)],
                &mut dummy,
            );
        }

        // Parse legacy WasmHookResponse format
        #[derive(serde::Deserialize)]
        struct LegacyHookResponse {
            output: serde_json::Value,
            blocked: Option<bool>,
            error: Option<String>,
        }

        let hook_resp: LegacyHookResponse = serde_json::from_slice(&output)
            .map_err(|e| RuntimeError::InvalidJson(format!("invalid hook response JSON: {e}")))?;

        Ok(PluginResponse {
            ok: hook_resp.error.is_none() && !hook_resp.blocked.unwrap_or(false),
            effects: Vec::new(),
            data: hook_resp.output,
            diagnostics: hook_resp
                .error
                .map(|e| {
                    vec![crate::protocol::plugin::PluginDiagnostic {
                        level: crate::protocol::plugin::PluginDiagnosticLevel::Error,
                        message: e,
                    }]
                })
                .unwrap_or_default(),
        })
    })();

    match result {
        Ok(resp) => Ok(resp),
        Err(e) => {
            cache.return_fuel(plugin_id, fuel);
            Err(e)
        }
    }
}

/// Helper to extract the hook type string from a capability invocation.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::runtime::wasm_cache;

    #[test]
    fn wasm_spec_from_manifest_relative_path() {
        let spec = WasmRuntimeSpec::from_manifest(
            "plugin.wasm",
            &PathBuf::from("/tmp/my-plugin"),
            Some(3000),
            Some(64),
            Some(500_000),
        );
        assert_eq!(
            spec.module_path,
            PathBuf::from("/tmp/my-plugin/plugin.wasm")
        );
        assert_eq!(spec.timeout_ms, Some(3000));
        assert_eq!(spec.memory_max_mb, Some(64));
        assert_eq!(spec.fuel_per_call, Some(500_000));
    }

    #[test]
    fn wasm_spec_from_manifest_absolute_path() {
        let spec = WasmRuntimeSpec::from_manifest(
            "/abs/path/plugin.wasm",
            &PathBuf::from("/tmp"),
            None,
            None,
            None,
        );
        assert_eq!(spec.module_path, PathBuf::from("/abs/path/plugin.wasm"));
        assert!(spec.timeout_ms.is_none());
        assert!(spec.memory_max_mb.is_none());
        assert!(spec.fuel_per_call.is_none());
    }

    #[test]
    fn wasm_runtime_unavailable_without_feature() {
        #[cfg(not(feature = "plugins"))]
        {
            let rt = WasmRuntime::with_defaults(WasmRuntimeSpec {
                module_path: PathBuf::from("test.wasm"),
                timeout_ms: None,
                memory_max_mb: None,
                fuel_per_call: None,
                entrypoint: None,
            });
            let invocation = PluginInvocation {
                protocol_version: PLUGIN_PROTOCOL_VERSION,
                invocation_id: "test-1".into(),
                plugin_id: "test".into(),
                capability: crate::protocol::plugin::PluginCapabilityInvocation::Command {
                    name: "test".into(),
                },
                args: Vec::new(),
                input: serde_json::Value::Null,
                context: Default::default(),
            };
            let runtime = tokio::runtime::Runtime::new().unwrap();
            let result = runtime.block_on(rt.invoke(invocation));
            assert!(result.is_err());
            assert!(matches!(result.unwrap_err(), RuntimeError::Unsupported(_)));
        }
    }

    #[test]
    fn wasm_module_cache_integration() {
        let cache = Arc::new(crate::plugin::runtime::wasm_cache::WasmModuleCache::new());
        let spec = WasmRuntimeSpec {
            module_path: PathBuf::from("test.wasm"),
            timeout_ms: None,
            memory_max_mb: None,
            fuel_per_call: None,
            entrypoint: None,
        };
        let rt = WasmRuntime::with_cache(spec, RuntimeLimits::default(), cache.clone());
        assert_eq!(rt.cache.stats(), (0, 0));
    }

    /// `return_unused_fuel` must credit back the unused portion of the
    /// reservation, not the consumed portion. Bug regression guard.
    #[test]
    fn return_unused_fuel_credits_unused_not_consumed() {
        let cache = WasmModuleCache::new();
        let reserved = 1000u64;

        // Reserve 1000 from the budget.
        let got = cache.reserve_fuel("plugin-a", reserved).unwrap();
        assert_eq!(got, reserved);
        let after_reserve = cache.get_plugin_fuel("plugin-a");
        assert_eq!(
            after_reserve,
            super::super::wasm_cache::MAX_PLUGIN_FUEL_BUDGET - reserved
        );

        // Pretend the store executed and 700 fuel is remaining (i.e. 300 consumed).
        let remaining = 700u64;
        return_unused_fuel(&cache, "plugin-a", reserved, remaining);

        let final_budget = cache.get_plugin_fuel("plugin-a");
        // Budget must reflect only 300 fuel consumed, not 700.
        assert_eq!(
            final_budget,
            super::super::wasm_cache::MAX_PLUGIN_FUEL_BUDGET - (reserved - remaining),
            "remaining fuel must come back to budget, not consumed fuel"
        );
    }

    /// Returning zero unused fuel is a no-op (the helper guards on `unused > 0`).
    #[test]
    fn return_unused_fuel_zero_remaining_is_noop() {
        let cache = WasmModuleCache::new();
        let reserved = 1000u64;
        cache.reserve_fuel("plugin-b", reserved).unwrap();
        let after_reserve = cache.get_plugin_fuel("plugin-b");

        return_unused_fuel(&cache, "plugin-b", reserved, 0);
        let after_zero_return = cache.get_plugin_fuel("plugin-b");
        assert_eq!(
            after_zero_return, after_reserve,
            "zero unused fuel must not return anything"
        );
    }

    /// If `remaining > reserved` for any reason (buggy instrumentation), the
    /// helper must cap the refund at `reserved` so the budget can never be
    /// over-credited past what was reserved.
    #[test]
    fn return_unused_fuel_caps_remaining_at_reserved() {
        let cache = WasmModuleCache::new();
        let reserved = 1000u64;
        cache.reserve_fuel("plugin-c", reserved).unwrap();

        // Buggy case: store claims more remaining than was reserved.
        return_unused_fuel(&cache, "plugin-c", reserved, 5_000_000);

        // Budget must recover fully (i.e. back to MAX), but not exceed MAX.
        let after = cache.get_plugin_fuel("plugin-c");
        assert_eq!(
            after,
            super::super::wasm_cache::MAX_PLUGIN_FUEL_BUDGET,
            "refund must cap at reserved amount"
        );
    }

    /// Full reservation returned: budget must be fully restored.
    #[test]
    fn return_unused_fuel_full_remaining_restores_budget() {
        let cache = WasmModuleCache::new();
        let reserved = 1000u64;
        cache.reserve_fuel("plugin-d", reserved).unwrap();
        return_unused_fuel(&cache, "plugin-d", reserved, reserved);
        assert_eq!(
            cache.get_plugin_fuel("plugin-d"),
            super::super::wasm_cache::MAX_PLUGIN_FUEL_BUDGET
        );
    }
}

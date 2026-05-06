use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::plugin::hooks::{HookContext, HookResult};
use crate::plugin::install::InstallError;
use crate::plugin::manifest::PluginManifest;

#[allow(dead_code)]
const MAX_WASM_SIZE: usize = 10 * 1024 * 1024;
#[allow(dead_code)]
const WASM_FUEL_PER_HOOK: u64 = 1_000_000;
#[allow(dead_code)]
const WASM_HOOK_TIMEOUT: Duration = Duration::from_secs(30);
static PLUGIN_FUEL_BUDGET: AtomicU64 = AtomicU64::new(10_000_000);
#[allow(dead_code)]
static PLUGIN_FUEL_LAST_RESET: AtomicU64 = AtomicU64::new(0);
#[allow(dead_code)]
const MAX_PLUGIN_FUEL_BUDGET: u64 = 10_000_000;
#[allow(dead_code)]
const FUEL_RESET_INTERVAL_SECS: u64 = 60;

#[allow(dead_code)]
fn check_and_reset_fuel_budget() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let last_reset = PLUGIN_FUEL_LAST_RESET.load(Ordering::Relaxed);

    if last_reset == 0 {
        PLUGIN_FUEL_LAST_RESET.store(now, Ordering::Relaxed);
        return;
    }

    if now.saturating_sub(last_reset) >= FUEL_RESET_INTERVAL_SECS {
        PLUGIN_FUEL_BUDGET.store(0, Ordering::Relaxed);
        PLUGIN_FUEL_LAST_RESET.store(now, Ordering::Relaxed);
        tracing::debug!("plugin fuel budget auto-reset");
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("manifest error: {0}")]
    Manifest(String),

    #[error("wasm error: {0}")]
    Wasm(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("install error: {0}")]
    Install(#[from] InstallError),
}

pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub wasm_path: PathBuf,
    pub plugin_dir: PathBuf,
}

pub async fn load_plugin(path: &Path) -> Result<LoadedPlugin, LoadError> {
    let plugin_dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .ok_or_else(|| LoadError::Manifest("no parent directory".into()))?
            .to_path_buf()
    };

    let manifest = load_manifest(&plugin_dir).await?;
    let wasm_path = find_wasm(&plugin_dir).await?;

    Ok(LoadedPlugin {
        manifest,
        wasm_path,
        plugin_dir,
    })
}

async fn load_manifest(plugin_dir: &Path) -> Result<PluginManifest, LoadError> {
    let manifest_path = plugin_dir.join("manifest.toml");

    if !manifest_path.exists() {
        return Err(LoadError::Manifest("manifest.toml not found".into()));
    }

    let content = tokio::fs::read_to_string(&manifest_path).await?;
    let manifest: PluginManifest =
        toml::from_str(&content).map_err(|e| LoadError::Manifest(e.to_string()))?;

    if manifest.name.is_empty() {
        return Err(LoadError::Manifest("plugin name is required".into()));
    }

    if manifest.version.is_empty() {
        return Err(LoadError::Manifest("plugin version is required".into()));
    }

    Ok(manifest)
}

async fn find_wasm(plugin_dir: &Path) -> Result<PathBuf, LoadError> {
    let wasm_extensions = ["wasm", "wasm32-wasi.wasm"];

    for ext in &wasm_extensions {
        let candidate = plugin_dir.join(format!("plugin.{}", ext));
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    let mut entries = tokio::fs::read_dir(plugin_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if let Some(ext) = path.extension() {
            if ext == "wasm" {
                return Ok(path);
            }
        }
    }

    Err(LoadError::Wasm("no .wasm file found".into()))
}

#[cfg(feature = "plugins")]
mod module_cache {
    use super::MAX_PLUGIN_FUEL_BUDGET;
    use dashmap::DashMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use wasmtime::Module;

    pub struct ModuleCache {
        modules: DashMap<String, (Module, u64)>,
        hits: AtomicU64,
        misses: AtomicU64,
        fuel_budgets: DashMap<String, AtomicU64>,
    }

    impl ModuleCache {
        pub fn new() -> Self {
            Self {
                modules: DashMap::new(),
                hits: AtomicU64::new(0),
                misses: AtomicU64::new(0),
                fuel_budgets: DashMap::new(),
            }
        }

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
                if entry.value().1 == mtime {
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    return Some(entry.value().0.clone());
                }
            }

            if let Some(module) = compile_fn() {
                self.misses.fetch_add(1, Ordering::Relaxed);
                self.modules
                    .insert(path.to_string(), (module.clone(), mtime));
                return Some(module);
            }

            None
        }

        #[allow(dead_code)]
        pub fn stats(&self) -> (u64, u64) {
            (
                self.hits.load(Ordering::Relaxed),
                self.misses.load(Ordering::Relaxed),
            )
        }

        pub fn get_plugin_fuel(&self, plugin_id: &str) -> u64 {
            self.fuel_budgets
                .get(plugin_id)
                .map(|entry| entry.value().load(Ordering::Relaxed))
                .unwrap_or(MAX_PLUGIN_FUEL_BUDGET)
        }

        #[allow(dead_code)]
        pub fn set_plugin_fuel(&self, plugin_id: &str, fuel: u64) {
            self.fuel_budgets
                .entry(plugin_id.to_string())
                .or_insert_with(|| AtomicU64::new(MAX_PLUGIN_FUEL_BUDGET))
                .store(fuel, Ordering::Relaxed);
        }

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

        pub fn return_fuel(&self, plugin_id: &str, fuel: u64) {
            let entry = self
                .fuel_budgets
                .entry(plugin_id.to_string())
                .or_insert_with(|| AtomicU64::new(MAX_PLUGIN_FUEL_BUDGET));
            entry.value().fetch_add(fuel, Ordering::Relaxed);
        }
    }

    impl Default for ModuleCache {
        fn default() -> Self {
            Self::new()
        }
    }

    pub static CACHE: once_cell::sync::Lazy<ModuleCache> =
        once_cell::sync::Lazy::new(ModuleCache::new);
}

#[cfg(feature = "plugins")]
pub async fn execute_wasm_hook(plugin_id: &str, ctx: HookContext) -> HookResult {
    use std::error::Error;
    use tokio::time::timeout;
    use wasmtime::{Config, Engine, Linker, Module, Store, WasmBacktraceDetails};

    type BoxError = Box<dyn Error + Send + Sync>;

    static ENGINE: once_cell::sync::Lazy<Engine> = once_cell::sync::Lazy::new(|| {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.wasm_backtrace_details(WasmBacktraceDetails::Disable);
        Engine::new(&config).unwrap()
    });

    let current_plugin_fuel = module_cache::CACHE.get_plugin_fuel(plugin_id);
    if current_plugin_fuel >= MAX_PLUGIN_FUEL_BUDGET {
        tracing::warn!(plugin = plugin_id, "plugin fuel budget exhausted");
        return HookResult::ok(ctx.input);
    }

    let fuel_for_this_call = WASM_FUEL_PER_HOOK.min(current_plugin_fuel);

    let Some(fuel_reserved) = module_cache::CACHE.reserve_fuel(plugin_id, fuel_for_this_call)
    else {
        tracing::warn!(plugin = plugin_id, "plugin fuel reservation failed");
        return HookResult::ok(ctx.input);
    };

    let plugin_dir = format!("plugins/{}", plugin_id);
    let wasm_path = format!("{}/plugin.wasm", plugin_dir);

    let metadata = match std::fs::metadata(&wasm_path) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(plugin = plugin_id, error = %e, "failed to read WASM metadata");
            return HookResult::ok(ctx.input);
        }
    };

    if metadata.len() > MAX_WASM_SIZE as u64 {
        tracing::warn!(
            plugin = plugin_id,
            size = metadata.len(),
            max = MAX_WASM_SIZE,
            "WASM module exceeds maximum size"
        );
        return HookResult::ok(ctx.input);
    }

    let module = module_cache::CACHE.get_or_compile(&wasm_path, || {
        let wasm_bytes = std::fs::read(&wasm_path).ok()?;
        Module::new(&ENGINE, &wasm_bytes).ok()
    });

    let module = match module {
        Some(m) => m,
        None => {
            tracing::warn!(
                plugin = plugin_id,
                "failed to get or compile WASM module from cache"
            );
            return HookResult::ok(ctx.input);
        }
    };

    let hook_result = timeout(WASM_HOOK_TIMEOUT, async {
        let mut store = Store::new(&ENGINE, ());
        store.set_fuel(fuel_reserved).ok();

        let mut linker = Linker::new(&ENGINE);
        linker.allow_shadowing(true);
        let instance = match linker.instantiate(&mut store, &module) {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!(plugin = plugin_id, error = %e, "failed to instantiate WASM module");
                return Ok::<(HookResult, u64), BoxError>((HookResult::ok(ctx.input), 0));
            }
        };

        let func_name = match ctx.hook_type {
            crate::plugin::hooks::HookType::Auth => "on_auth",
            crate::plugin::hooks::HookType::Provider => "on_provider",
            crate::plugin::hooks::HookType::ToolDefinition => "on_tool_definition",
            crate::plugin::hooks::HookType::ToolExecuteBefore => "on_tool_execute_before",
            crate::plugin::hooks::HookType::ToolExecuteAfter => "on_tool_execute_after",
            crate::plugin::hooks::HookType::ChatParams => "on_chat_params",
            crate::plugin::hooks::HookType::ChatHeaders => "on_chat_headers",
            crate::plugin::hooks::HookType::Event => "on_event",
            crate::plugin::hooks::HookType::Config => "on_config",
            crate::plugin::hooks::HookType::ShellEnv => "on_shell_env",
            crate::plugin::hooks::HookType::TextComplete => "on_text_complete",
            crate::plugin::hooks::HookType::SessionCompacting => "on_session_compacting",
            crate::plugin::hooks::HookType::MessagesTransform => "on_messages_transform",
        };

        let func = match instance.get_func(&mut store, func_name) {
            Some(f) => f,
            None => {
                tracing::debug!(
                    plugin = plugin_id,
                    function = func_name,
                    "WASM hook function not found"
                );
                return Ok::<(HookResult, u64), BoxError>((HookResult::ok(ctx.input), 0));
            }
        };

        let memory = match instance.get_memory(&mut store, "memory") {
            Some(m) => m,
            None => {
                tracing::warn!(plugin = plugin_id, "WASM module has no memory export");
                return Ok::<(HookResult, u64), BoxError>((
                    HookResult::error("WASM module has no memory export"),
                    0,
                ));
            }
        };

        let input_json = serde_json::to_string(&ctx.input).unwrap_or_default();
        let input_bytes = input_json.as_bytes();

        let alloc_func = match instance.get_func(&mut store, "allocate") {
            Some(f) => f,
            None => {
                tracing::warn!(plugin = plugin_id, "WASM module has no allocate function");
                return Ok::<(HookResult, u64), BoxError>((
                    HookResult::error("WASM module missing allocate function"),
                    0,
                ));
            }
        };

        let mut input_ptr_vals = [wasmtime::Val::I32(0)];
        alloc_func.call(
            &mut store,
            &[wasmtime::Val::I32(input_bytes.len() as i32)],
            &mut input_ptr_vals,
        )?;
        let input_ptr = match input_ptr_vals[0].i32() {
            Some(p) => p,
            None => {
                tracing::warn!(plugin = plugin_id, "allocate returned no value");
                return Ok::<(HookResult, u64), BoxError>((
                    HookResult::error("allocate returned no value"),
                    0,
                ));
            }
        };

        memory
            .write(&mut store, input_ptr as usize, input_bytes)
            .map_err(|e| format!("memory write failed: {}", e))?;

        let memory_size = memory.data_size(&store);
        if input_ptr as usize + input_bytes.len() > memory_size {
            tracing::warn!(plugin = plugin_id, "WASM input exceeds memory bounds");
            return Ok::<(HookResult, u64), BoxError>((
                HookResult::error("WASM input exceeds memory bounds"),
                0,
            ));
        }

        let mut result_ptr_vals = [wasmtime::Val::I32(0)];
        func.call(
            &mut store,
            &[
                wasmtime::Val::I32(input_ptr),
                wasmtime::Val::I32(input_bytes.len() as i32),
            ],
            &mut result_ptr_vals,
        )?;
        let result_ptr = match result_ptr_vals[0].i32() {
            Some(p) => p,
            None => {
                tracing::warn!(plugin = plugin_id, "hook function returned no value");
                return Ok::<(HookResult, u64), BoxError>((
                    HookResult::error("hook function returned no value"),
                    0,
                ));
            }
        };

        if result_ptr == 0 {
            let remaining = store.get_fuel().unwrap_or(0);
            return Ok::<(HookResult, u64), BoxError>((HookResult::ok(ctx.input), remaining));
        }

        let output_json = {
            let mut len_bytes = [0u8; 4];
            memory
                .read(&mut store, result_ptr as usize + 4, &mut len_bytes)
                .map_err(|e| format!("memory read len failed: {}", e))?;
            let output_len = u32::from_le_bytes(len_bytes) as usize;
            const MAX_WASM_OUTPUT_SIZE: usize = 10 * 1024 * 1024; // 10MB limit
            if output_len > MAX_WASM_OUTPUT_SIZE {
                tracing::warn!(
                    plugin = plugin_id,
                    size = output_len,
                    "WASM output exceeds limit"
                );
                return Ok::<(HookResult, u64), BoxError>((
                    HookResult::error("WASM output exceeds size limit"),
                    0,
                ));
            }
            if output_len == 0 {
                let remaining = store.get_fuel().unwrap_or(0);
                return Ok::<(HookResult, u64), BoxError>((HookResult::ok(ctx.input), remaining));
            }
            let mut output_vec = vec![0u8; output_len];
            memory
                .read(&mut store, result_ptr as usize + 8, &mut output_vec)
                .map_err(|e| format!("memory read data failed: {}", e))?;
            output_vec
        };

        if let Some(free_func) = instance.get_func(&mut store, "deallocate") {
            let mut dummy = [wasmtime::Val::I32(0)];
            free_func
                .call(
                    &mut store,
                    &[wasmtime::Val::I32(result_ptr), wasmtime::Val::I32(0)],
                    &mut dummy,
                )
                .ok();
        }

        let remaining = store.get_fuel().unwrap_or(0);

        #[derive(serde::Deserialize)]
        struct WasmHookResponse {
            output: serde_json::Value,
            blocked: Option<bool>,
            error: Option<String>,
        }

        let hook_response: WasmHookResponse = match serde_json::from_slice(&output_json) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(plugin = plugin_id, error = %e, "failed to parse WASM output");
                return Ok::<(HookResult, u64), BoxError>((
                    HookResult::error(format!("failed to parse WASM output: {}", e)),
                    0,
                ));
            }
        };

        tracing::debug!(
            plugin = plugin_id,
            function = func_name,
            blocked = hook_response.blocked.unwrap_or(false),
            "WASM hook executed successfully"
        );

        Ok::<(HookResult, u64), BoxError>((
            HookResult {
                output: hook_response.output,
                blocked: hook_response.blocked.unwrap_or(false),
                error: hook_response.error,
            },
            remaining,
        ))
    })
    .await;

    match hook_result {
        Ok(Ok((result, remaining))) => {
            let consumed = fuel_reserved.saturating_sub(remaining);
            if consumed > 0 {
                module_cache::CACHE.return_fuel(plugin_id, consumed);
            }
            result
        }
        Ok(Err(e)) => {
            module_cache::CACHE.return_fuel(plugin_id, fuel_reserved);
            tracing::warn!(plugin = plugin_id, error = %e, "WASM hook execution error");
            HookResult::error(format!("WASM hook execution error: {}", e))
        }
        Err(_) => {
            module_cache::CACHE.return_fuel(plugin_id, fuel_reserved);
            tracing::warn!(
                plugin = plugin_id,
                "WASM hook timed out after {:?}",
                WASM_HOOK_TIMEOUT
            );
            HookResult::error("WASM hook timed out")
        }
    }
}

#[cfg(not(feature = "plugins"))]
pub async fn execute_wasm_hook(_plugin_id: &str, ctx: HookContext) -> HookResult {
    HookResult::ok(ctx.input)
}

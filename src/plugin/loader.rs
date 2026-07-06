//! Plugin loader and WASM compatibility shim.
//!
//! The canonical WASM execution path is now `runtime::wasm::WasmRuntime`.
//! This module retains `load_plugin`, `LoadedPlugin`, and the legacy
//! `execute_wasm_hook` entry point as a thin compatibility wrapper that
//! delegates to the new runtime.

use std::path::{Path, PathBuf};

use crate::plugin::hooks::{HookContext, HookResult};
use crate::plugin::install::InstallError;
use crate::plugin::manifest::PluginManifest;

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

impl From<LoadError> for crate::error::PluginError {
    fn from(err: LoadError) -> Self {
        crate::error::PluginError::LoadFailed(err.to_string())
    }
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

/// Legacy WASM hook execution entry point.
///
/// Now delegates to `WasmRuntime` via `PluginInvocation` when the `plugins`
/// feature is enabled. Retained for backward compatibility with existing hook
/// dispatch paths.
pub async fn execute_wasm_hook(plugin_id: &str, ctx: HookContext) -> HookResult {
    #[cfg(feature = "plugins")]
    {
        use crate::plugin::runtime::wasm::{WasmRuntime, WasmRuntimeSpec};
        use crate::plugin::runtime::PluginRuntime;
        use crate::protocol::plugin::{PluginInvocation, PLUGIN_PROTOCOL_VERSION};

        let plugin_name = plugin_id.strip_prefix("plugin:").unwrap_or(plugin_id);
        let plugin_dir = crate::plugin::install::plugins_dir().join(plugin_name);

        // Build invocation from hook context
        let invocation = PluginInvocation {
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            invocation_id: uuid::Uuid::new_v4().to_string(),
            plugin_id: plugin_id.to_string(),
            capability: crate::protocol::plugin::PluginCapabilityInvocation::Hook {
                hook_type: ctx.hook_type.as_str().to_string(),
            },
            args: Vec::new(),
            input: ctx.input.clone(),
            context: crate::protocol::plugin::PluginContext::default(),
        };

        // Find the WASM module
        let wasm_path = find_wasm_sync(&plugin_dir).unwrap_or_default();
        if !wasm_path.exists() {
            return HookResult::ok(ctx.input);
        }

        let spec = WasmRuntimeSpec::from_manifest(
            &wasm_path.to_string_lossy(),
            &plugin_dir,
            Some(30_000), // legacy 30s timeout
            None,
            None,
        );

        let cache = std::sync::Arc::new(crate::plugin::runtime::wasm_cache::WasmModuleCache::new());
        let runtime = WasmRuntime::with_cache(
            spec,
            crate::plugin::runtime::RuntimeLimits {
                timeout_ms: 30_000,
                ..Default::default()
            },
            cache,
        );

        match runtime.invoke(invocation).await {
            Ok(response) => HookResult::from_plugin_response(response, ctx.input),
            Err(e) => {
                tracing::warn!(plugin = plugin_id, error = %e, "WASM hook execution failed");
                HookResult::error(format!("WASM hook failed: {e}"))
            }
        }
    }
    #[cfg(not(feature = "plugins"))]
    {
        let _ = plugin_id;
        HookResult::ok(ctx.input)
    }
}

/// Synchronous WASM file finder (for use in blocking contexts).
#[cfg(feature = "plugins")]
fn find_wasm_sync(plugin_dir: &Path) -> Result<PathBuf, LoadError> {
    let wasm_extensions = ["wasm", "wasm32-wasi.wasm"];

    for ext in &wasm_extensions {
        let candidate = plugin_dir.join(format!("plugin.{}", ext));
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    if let Ok(entries) = std::fs::read_dir(plugin_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "wasm").unwrap_or(false) {
                return Ok(path);
            }
        }
    }

    Err(LoadError::Wasm("no .wasm file found".into()))
}

// Re-export module cache for backward compatibility
pub use crate::plugin::runtime::wasm_cache::{WasmModuleCache, WASM_CACHE};

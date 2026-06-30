pub mod process;
#[cfg(feature = "plugins")]
pub mod wasm;
pub mod wasm_cache;

use async_trait::async_trait;

use crate::protocol::plugin::{PluginInvocation, PluginResponse};

/// Default timeout for plugin command execution (5 seconds).
pub const DEFAULT_TIMEOUT_MS: u64 = 5_000;

/// Default maximum stdout bytes (1 MiB).
pub const DEFAULT_MAX_STDOUT_BYTES: usize = 1024 * 1024;

/// Default maximum stderr bytes (256 KiB).
pub const DEFAULT_MAX_STDERR_BYTES: usize = 256 * 1024;

/// Resource limits for plugin runtime execution.
#[derive(Debug, Clone)]
pub struct RuntimeLimits {
    pub timeout_ms: u64,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_stdout_bytes: DEFAULT_MAX_STDOUT_BYTES,
            max_stderr_bytes: DEFAULT_MAX_STDERR_BYTES,
        }
    }
}

/// Errors that can occur during plugin runtime execution.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("unsupported runtime: {0}")]
    Unsupported(String),
    #[error("spawn failed: {0}")]
    Spawn(String),
    #[error("runtime timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },
    #[error("process exited with code {code}: {stderr}")]
    NonZeroExit {
        code: i32,
        stdout: String,
        stderr: String,
    },
    #[error("invalid response json: {0}")]
    InvalidJson(String),
    #[error("io error: {0}")]
    Io(String),
}

/// A plugin runtime that can execute plugin invocations.
///
/// Implementations handle the actual execution of plugin commands (process,
/// WASM, builtin, etc.) and return protocol-level responses.
#[async_trait]
pub trait PluginRuntime: Send + Sync {
    /// Execute a plugin invocation and return a structured response.
    async fn invoke(&self, invocation: PluginInvocation) -> Result<PluginResponse, RuntimeError>;
}

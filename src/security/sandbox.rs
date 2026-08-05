#![allow(clippy::type_complexity)]

use crate::error::ToolError;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, PartialEq)]
pub enum SandboxMode {
    #[default]
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

impl SandboxMode {
    fn is_writable(&self) -> bool {
        matches!(self, Self::WorkspaceWrite | Self::DangerFullAccess)
    }
}

#[derive(Clone, Debug, Default)]
pub struct SandboxConfig {
    pub enabled: bool,
    pub mode: SandboxMode,
    pub allowed_paths: Vec<String>,
    pub deny_paths: Vec<String>,
}

impl SandboxConfig {
    pub fn new() -> Self {
        Self {
            enabled: false,
            mode: SandboxMode::default(),
            allowed_paths: Vec::new(),
            deny_paths: Vec::new(),
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_mode(mut self, mode: SandboxMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_allowed_paths(mut self, paths: Vec<String>) -> Self {
        self.allowed_paths = paths;
        self
    }

    pub fn with_deny_paths(mut self, paths: Vec<String>) -> Self {
        self.deny_paths = paths;
        self
    }

    pub fn is_available() -> bool {
        #[cfg(target_os = "linux")]
        {
            probe_landlock().is_ok()
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    pub fn enforce(&self) -> Result<(), ToolError> {
        if self.enabled {
            return Err(ToolError::Permission(
                "sandbox enforcement is child-process-only; launch through the sandbox helper"
                    .to_string(),
            ));
        }
        Ok(())
    }

    /// Construct the bounded child-launch description used by the private
    /// helper. All paths are resolved before the child starts; missing rules
    /// are policy errors, never silently skipped.
    pub fn launch_spec(
        &self,
        target: impl AsRef<Path>,
        args: &[String],
        cwd: Option<&Path>,
    ) -> Result<SandboxLaunchSpec, ToolError> {
        if !self.enabled {
            return Err(ToolError::Permission(
                "cannot build a sandbox launch spec for a disabled sandbox".to_string(),
            ));
        }
        let target = resolve_executable(target.as_ref()).ok_or_else(|| {
            ToolError::Permission(format!(
                "sandbox target could not be resolved: {}",
                target.as_ref().display()
            ))
        })?;
        let roots = if self.allowed_paths.is_empty() {
            vec![cwd
                .ok_or_else(|| ToolError::Permission("sandbox cwd is required".to_string()))?
                .to_path_buf()]
        } else {
            self.allowed_paths
                .iter()
                .map(|raw| {
                    std::fs::canonicalize(raw).map_err(|e| {
                        ToolError::Permission(format!(
                            "sandbox path '{raw}' could not resolve: {e}"
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        let write_roots = roots.clone();
        let mut read_paths = roots;
        read_paths.push(target.clone());
        for raw in ["/usr/lib", "/usr/lib64", "/lib", "/lib64"] {
            let path = Path::new(raw);
            if path.exists() {
                read_paths.push(path.to_path_buf());
            }
        }
        let write_paths = if self.mode.is_writable() {
            write_roots
        } else {
            Vec::new()
        };
        Ok(SandboxLaunchSpec {
            target,
            args: args.to_vec(),
            read_paths,
            write_paths,
        })
    }
}

/// Private, bounded launch description consumed by `codegg-sandbox-helper`.
/// It is local process plumbing, not a daemon or public wire protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxLaunchSpec {
    pub target: PathBuf,
    pub args: Vec<String>,
    pub read_paths: Vec<PathBuf>,
    pub write_paths: Vec<PathBuf>,
}

/// The result reported by the one-shot helper before it execs the target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SandboxLaunchOutcome {
    Enforced { abi: u32 },
    Unavailable { reason: String },
    SetupError { reason: String },
}

pub const SANDBOX_HELPER_ENFORCED_PREFIX: &str = "CODEGG_SANDBOX_ENFORCED abi=";
pub const SANDBOX_HELPER_ERROR_PREFIX: &str = "CODEGG_SANDBOX_ERROR ";

/// Return the private helper executable path. Tests use the second candidate
/// because Cargo places test binaries under `target/debug/deps`.
pub fn sandbox_helper_path() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("CODEGG_SANDBOX_HELPER") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }
    let current = std::env::current_exe().map_err(|e| format!("current executable: {e}"))?;
    let candidates = [
        current.parent().map(|p| p.join("codegg-sandbox-helper")),
        current
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("codegg-sandbox-helper")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|path| path.is_file())
        .ok_or_else(|| "codegg-sandbox-helper executable was not found".to_string())
}

fn resolve_executable(path: &Path) -> Option<PathBuf> {
    if path.is_absolute() {
        return path.canonicalize().ok();
    }
    std::env::var_os("PATH").and_then(|path_var| {
        std::env::split_paths(&path_var)
            .map(|dir| dir.join(path))
            .find(|candidate| candidate.is_file())
            .and_then(|candidate| candidate.canonicalize().ok())
    })
}

#[cfg(target_os = "linux")]
pub fn probe_landlock() -> Result<(), String> {
    use landlock::{Access, AccessFs, CompatLevel, Compatible, Ruleset, ABI};
    Ruleset::default()
        .handle_access(AccessFs::from_read(ABI::V1))
        .map_err(|e| format!("Landlock access selection failed: {e}"))?
        .set_compatibility(CompatLevel::HardRequirement)
        .create()
        .map(|_| ())
        .map_err(|e| format!("Landlock unavailable: {e}"))
}

#[cfg(not(target_os = "linux"))]
pub fn probe_landlock() -> Result<(), String> {
    Err("Landlock is only available on Linux".to_string())
}

#[cfg(target_os = "linux")]
pub fn apply_landlock(spec: &SandboxLaunchSpec) -> Result<u32, String> {
    use landlock::{
        Access, AccessFs, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset,
        RulesetCreatedAttr, RulesetStatus, ABI,
    };

    let abi = ABI::V9;
    let read_access = AccessFs::from_read(abi);
    let write_access = AccessFs::from_all(abi);
    let handled = AccessFs::from_all(abi);
    let mut ruleset = Ruleset::default()
        .handle_access(handled)
        .map_err(|e| format!("Landlock ruleset access selection failed: {e}"))?
        .create()
        .map_err(|e| format!("Landlock ruleset creation failed: {e}"))?;

    let mut add_path = |path: &Path, access| -> Result<(), String> {
        if !path.exists() {
            return Err(format!(
                "required sandbox path does not exist: {}",
                path.display()
            ));
        }
        let fd =
            PathFd::new(path).map_err(|e| format!("open sandbox path {}: {e}", path.display()))?;
        let access = if path.is_file() {
            access & AccessFs::from_file(abi)
        } else {
            access
        };
        ruleset = ruleset
            .add_rule(PathBeneath::new(fd, access))
            .map_err(|e| format!("add sandbox rule {}: {e}", path.display()))?;
        Ok(())
    };

    for path in &spec.read_paths {
        add_path(path, read_access)?;
    }
    for path in &spec.write_paths {
        add_path(path, write_access)?;
    }

    let status = ruleset
        .set_compatibility(CompatLevel::HardRequirement)
        .restrict_self()
        .map_err(|e| format!("Landlock restriction failed: {e}"))?;
    if status.ruleset != RulesetStatus::FullyEnforced || !status.no_new_privs {
        return Err(format!(
            "Landlock restriction was not fully enforced (ruleset={:?}, no_new_privs={})",
            status.ruleset, status.no_new_privs
        ));
    }
    match status.landlock {
        landlock::LandlockStatus::Available { effective_abi, .. } => Ok(effective_abi as u32),
        other => Err(format!(
            "Landlock became unavailable during setup: {other:?}"
        )),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn apply_landlock(_spec: &SandboxLaunchSpec) -> Result<u32, String> {
    Err("Landlock is only available on Linux".to_string())
}

struct CachedPaths {
    paths: Vec<PathBuf>,
    timestamp: Instant,
}

static CANONICAL_PATHS_CACHE: Mutex<
    Option<(HashMap<Vec<String>, CachedPaths>, VecDeque<Vec<String>>)>,
> = Mutex::new(None);

const MAX_CACHE_ENTRIES: usize = 100;
const CACHE_TTL: Duration = Duration::from_secs(300);

fn get_canonical_paths(allowed_paths: &[String]) -> Vec<PathBuf> {
    let mut cache = CANONICAL_PATHS_CACHE.lock().unwrap();
    if cache.is_none() {
        *cache = Some((HashMap::new(), VecDeque::new()));
    }
    let (cache_map, cache_order) = cache.as_mut().unwrap();

    if cache_map.is_empty() || cache_order.is_empty() {
        cache_order.clear();
    } else if let Some(oldest_key) = cache_order.front() {
        if let Some(cached) = cache_map.get(oldest_key) {
            if cached.timestamp.elapsed() > CACHE_TTL {
                cache_map.clear();
                cache_order.clear();
            }
        }
    }

    while cache_order.len() >= MAX_CACHE_ENTRIES {
        if let Some(oldest_key) = cache_order.pop_front() {
            cache_map.remove(&oldest_key);
        }
    }

    if let Some(cached) = cache_map.get(allowed_paths) {
        return cached.paths.clone();
    }

    let canonical: Vec<PathBuf> = allowed_paths
        .iter()
        .filter_map(|p| std::fs::canonicalize(p).ok())
        .collect();

    cache_map.insert(
        allowed_paths.to_vec(),
        CachedPaths {
            paths: canonical.clone(),
            timestamp: Instant::now(),
        },
    );
    cache_order.push_back(allowed_paths.to_vec());
    canonical
}

pub fn validate_path_safety(path: &Path, allowed_paths: &[String]) -> Result<(), ToolError> {
    if path
        .symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(ToolError::Permission(format!(
            "path '{}' is a symlink",
            path.display()
        )));
    }

    let canonical = std::fs::canonicalize(path).map_err(|_| {
        ToolError::Permission(format!("path '{}' could not be resolved", path.display()))
    })?;

    let allowed_canonical = get_canonical_paths(allowed_paths);
    for allowed in &allowed_canonical {
        if canonical.starts_with(allowed) {
            return Ok(());
        }
    }

    Err(ToolError::Permission(format!(
        "path '{}' is not in allowed paths",
        path.display()
    )))
}

pub fn get_default_allowed_paths() -> Vec<String> {
    let mut paths = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.to_string_lossy().to_string());
    }

    if let Ok(home) = std::env::var("HOME") {
        let home_path = Path::new(&home);
        if home_path.exists() {
            paths.push(format!("{}/.config", home));
            paths.push(format!("{}/.local/share", home));
        }
    }

    if let Some(config) = dirs::config_dir() {
        paths.push(config.to_string_lossy().to_string());
    }

    if let Some(data) = dirs::data_dir() {
        paths.push(data.to_string_lossy().to_string());
    }

    paths
}

pub fn get_sensitive_paths() -> Vec<String> {
    vec![
        "/etc".to_string(),
        "/home".to_string(),
        "/root".to_string(),
        "/var".to_string(),
        "/ssh".to_string(),
        "/proc".to_string(),
        "/sys".to_string(),
        "/dev".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_config_default() {
        let config = SandboxConfig::new();
        assert!(!config.enabled);
        assert!(config.allowed_paths.is_empty());
    }

    #[test]
    fn enabled_enforcement_cannot_restrict_the_parent() {
        let config = SandboxConfig::new().with_enabled(true);
        let error = config
            .enforce()
            .expect_err("enabled enforcement must be child-only");
        assert!(error.to_string().contains("child-process-only"));
    }

    #[test]
    fn launch_spec_maps_workspace_write_to_write_roots() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let config = SandboxConfig::new()
            .with_enabled(true)
            .with_mode(SandboxMode::WorkspaceWrite)
            .with_allowed_paths(vec![temp_dir.path().to_string_lossy().to_string()]);
        let spec = config
            .launch_spec(
                "sh",
                &["-c".to_string(), "true".to_string()],
                Some(temp_dir.path()),
            )
            .expect("spec should be constructed");
        let canonical = temp_dir.path().canonicalize().expect("canonical temp dir");
        assert!(spec.read_paths.iter().any(|path| path == &canonical));
        assert!(spec.write_paths.iter().any(|path| path == &canonical));
        assert!(spec.args.contains(&"-c".to_string()));
    }

    #[test]
    fn test_validate_path_safety() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let temp_path = temp_dir.path().join("test");
        std::fs::create_dir_all(&temp_path).expect("temp path should be created");

        let allowed = vec![
            temp_dir.path().to_string_lossy().to_string(),
            "/home/user/project".to_string(),
        ];
        let result = validate_path_safety(&temp_path, &allowed);
        assert!(
            result.is_ok(),
            "path inside temp_dir should be allowed: {:?}",
            result
        );

        let result = validate_path_safety(Path::new("/etc/passwd"), &allowed);
        assert!(result.is_err(), "path outside allowed should be rejected");
    }

    #[test]
    fn test_validate_path_safety_with_symlink() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let real = temp_dir.path().join("real");
        let link = temp_dir.path().join("link");
        std::fs::create_dir_all(&real).expect("real dir should be created");

        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).expect("symlink should be created");

        #[cfg(not(unix))]
        {
            return;
        }

        let allowed = vec![temp_dir.path().to_string_lossy().to_string()];
        let result = validate_path_safety(&link, &allowed);
        assert!(
            result.is_err(),
            "symlink in path should be rejected: {:?}",
            result
        );
    }
}

#![allow(clippy::type_complexity)]

use crate::error::ToolError;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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

/// One-shot helper status. `Enforced` is a setup event; the other variants
/// are terminal events. The parent accepts `Enforced` followed by EOF for a
/// successful target exec, or `Enforced` followed by `ExecError` when exec
/// returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SandboxLaunchOutcome {
    Enforced { abi: u32 },
    Unavailable { reason: String },
    SetupError { reason: String },
    ExecError { reason: String },
}

/// Version for the private helper status frame. This is local process
/// plumbing, not a public or durable protocol.
pub const SANDBOX_STATUS_VERSION: u8 = 1;
pub const MAX_SANDBOX_STATUS_BYTES: usize = 16 * 1024;
pub const MAX_SANDBOX_SPEC_BYTES: usize = 64 * 1024;

#[cfg(unix)]
pub const SANDBOX_STATUS_FD: i32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxStatusFrame {
    pub version: u8,
    pub outcome: SandboxLaunchOutcome,
}

/// Encode one bounded, length-prefixed status frame.
pub fn encode_sandbox_status(outcome: SandboxLaunchOutcome) -> Result<Vec<u8>, String> {
    let payload = serde_json::to_vec(&SandboxStatusFrame {
        version: SANDBOX_STATUS_VERSION,
        outcome,
    })
    .map_err(|error| format!("encode sandbox status: {error}"))?;
    let frame_len = 4usize
        .checked_add(payload.len())
        .ok_or_else(|| "sandbox status frame length overflowed".to_string())?;
    if frame_len > MAX_SANDBOX_STATUS_BYTES {
        return Err("sandbox status frame exceeds 16 KiB".to_string());
    }
    let payload_len = u32::try_from(payload.len())
        .map_err(|_| "sandbox status payload length exceeds u32".to_string())?;
    let mut frame = Vec::with_capacity(frame_len);
    frame.extend_from_slice(&payload_len.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Decode the complete private status stream and enforce its small state
/// machine. A target that writes to the channel, a helper that emits a
/// duplicate terminal state, or a truncated/oversized stream fails closed.
pub fn decode_sandbox_status(bytes: &[u8]) -> Result<SandboxLaunchOutcome, String> {
    if bytes.is_empty() {
        return Err("sandbox helper produced no status frame".to_string());
    }
    if bytes.len() > MAX_SANDBOX_STATUS_BYTES {
        return Err("sandbox status stream exceeds 16 KiB".to_string());
    }

    let mut cursor = 0usize;
    let mut setup_abi = None;
    let mut terminal = None;
    while cursor < bytes.len() {
        let length_end = cursor
            .checked_add(4)
            .ok_or_else(|| "sandbox status length overflowed".to_string())?;
        if length_end > bytes.len() {
            return Err("sandbox status frame has a truncated length prefix".to_string());
        }
        let payload_len = u32::from_be_bytes(
            bytes[cursor..length_end]
                .try_into()
                .map_err(|_| "sandbox status length prefix is invalid".to_string())?,
        ) as usize;
        if payload_len == 0 || payload_len > MAX_SANDBOX_STATUS_BYTES - 4 {
            return Err("sandbox status frame has an invalid length".to_string());
        }
        cursor = length_end;
        let payload_end = cursor
            .checked_add(payload_len)
            .ok_or_else(|| "sandbox status payload length overflowed".to_string())?;
        if payload_end > bytes.len() {
            return Err("sandbox status frame is truncated".to_string());
        }
        let frame: SandboxStatusFrame = serde_json::from_slice(&bytes[cursor..payload_end])
            .map_err(|error| format!("sandbox status frame is malformed: {error}"))?;
        if frame.version != SANDBOX_STATUS_VERSION {
            return Err(format!(
                "unsupported sandbox status version {}",
                frame.version
            ));
        }
        cursor = payload_end;

        match frame.outcome {
            SandboxLaunchOutcome::Enforced { abi } => {
                if setup_abi.replace(abi).is_some() || terminal.is_some() {
                    return Err("sandbox helper produced a duplicate setup status".to_string());
                }
            }
            terminal_outcome @ (SandboxLaunchOutcome::Unavailable { .. }
            | SandboxLaunchOutcome::SetupError { .. }
            | SandboxLaunchOutcome::ExecError { .. }) => {
                if terminal.is_some() {
                    return Err("sandbox helper produced duplicate terminal status".to_string());
                }
                if setup_abi.is_some()
                    && !matches!(&terminal_outcome, SandboxLaunchOutcome::ExecError { .. })
                {
                    return Err("sandbox helper produced a terminal status after setup".to_string());
                }
                if matches!(&terminal_outcome, SandboxLaunchOutcome::ExecError { .. })
                    && setup_abi.is_none()
                {
                    return Err("sandbox exec failure was reported before setup".to_string());
                }
                terminal = Some(terminal_outcome);
            }
        }
    }

    if let Some(outcome) = terminal {
        if setup_abi.is_none() && matches!(outcome, SandboxLaunchOutcome::ExecError { .. }) {
            return Err("sandbox exec failure had no enforced setup".to_string());
        }
        return Ok(outcome);
    }
    setup_abi
        .map(|abi| SandboxLaunchOutcome::Enforced { abi })
        .ok_or_else(|| "sandbox helper produced no terminal status".to_string())
}

/// Return the private helper executable from the installation-owned sibling
/// location. Inherited environment, PATH, and cwd are deliberately not part
/// of this resolution rule.
pub fn sandbox_helper_path() -> Result<PathBuf, String> {
    let current = std::env::current_exe().map_err(|e| format!("current executable: {e}"))?;
    resolve_trusted_helper(&current)
}

fn resolve_trusted_helper(current: &Path) -> Result<PathBuf, String> {
    let current = current
        .canonicalize()
        .map_err(|error| format!("CodeGG executable could not be resolved: {error}"))?;
    let install_root = current
        .parent()
        .ok_or_else(|| "CodeGG executable has no installation directory".to_string())?
        .canonicalize()
        .map_err(|error| format!("CodeGG installation directory could not be resolved: {error}"))?;
    // Cargo places unit-test executables in `target/debug/deps`, while the
    // sibling helper is built in `target/debug`. This adjustment is compiled
    // only into test builds; installed production binaries retain the strict
    // same-directory trust rule above.
    #[cfg(test)]
    let install_root = if install_root.file_name().is_some_and(|name| name == "deps") {
        install_root
            .parent()
            .ok_or_else(|| "Cargo test executable has no target directory".to_string())?
            .canonicalize()
            .map_err(|error| format!("Cargo target directory could not be resolved: {error}"))?
    } else {
        install_root
    };
    let candidate = install_root.join("codegg-sandbox-helper");
    let helper = candidate
        .canonicalize()
        .map_err(|error| format!("trusted sandbox helper could not be resolved: {error}"))?;
    if helper.parent() != Some(install_root.as_path()) {
        return Err("trusted sandbox helper escaped the installation directory".to_string());
    }
    let metadata = std::fs::metadata(&helper)
        .map_err(|error| format!("trusted sandbox helper metadata unavailable: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err("trusted sandbox helper is not a regular file".to_string());
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err("trusted sandbox helper is not executable".to_string());
    }
    Ok(helper)
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
    use landlock::{AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, ABI};
    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_read(ABI::V1))
        .map_err(|e| format!("Landlock access selection failed: {e}"))?
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
        Access, AccessFs, BitFlags, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset,
        RulesetAttr, RulesetCreated, RulesetCreatedAttr, RulesetStatus, ABI,
    };

    // ABI 1 is the minimum Landlock filesystem contract and is available on
    // every Landlock-capable kernel. Newer rights are intentionally not
    // requested dynamically: a partial ruleset must never be reported as
    // enforced, and the helper's outcome still records the kernel's
    // effective ABI for observability.
    let abi = ABI::V1;
    let read_access = AccessFs::from_read(abi);
    let write_access = AccessFs::from_all(abi);
    let handled = AccessFs::from_all(abi);
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(handled)
        .map_err(|e| format!("Landlock ruleset access selection failed: {e}"))?
        .create()
        .map_err(|e| format!("Landlock ruleset creation failed: {e}"))?;

    let add_path = |ruleset: RulesetCreated, path: &Path, access: BitFlags<AccessFs>| {
        if !path.exists() {
            return Err(format!(
                "required sandbox path does not exist: {}",
                path.display()
            ));
        }
        let fd =
            PathFd::new(path).map_err(|e| format!("open sandbox path {}: {e}", path.display()))?;
        let access = landlock_access_for_path(path, access, abi)?;
        ruleset
            .add_rule(PathBeneath::new(fd, access))
            .map_err(|e| format!("add sandbox rule {}: {e}", path.display()))
    };

    for path in &spec.read_paths {
        ruleset = add_path(ruleset, path, read_access)?;
    }
    for path in &spec.write_paths {
        ruleset = add_path(ruleset, path, write_access)?;
    }

    let status = ruleset
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

#[cfg(target_os = "linux")]
fn landlock_access_for_path(
    path: &Path,
    access: landlock::BitFlags<landlock::AccessFs>,
    abi: landlock::ABI,
) -> Result<landlock::BitFlags<landlock::AccessFs>, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("classify sandbox path {}: {error}", path.display()))?;
    if metadata.is_dir() {
        Ok(access)
    } else {
        Ok(access & landlock::AccessFs::from_file(abi))
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
    let mut cache = CANONICAL_PATHS_CACHE.lock().unwrap_or_else(|poisoned| {
        tracing::warn!("canonical path cache mutex was poisoned; resetting the cache");
        let mut cache = poisoned.into_inner();
        *cache = None;
        cache
    });
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

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_access_keeps_directory_rights_for_directories() {
        use landlock::{AccessFs, ABI};

        let directory = tempfile::tempdir().expect("directory fixture");
        let access =
            landlock_access_for_path(directory.path(), AccessFs::from_read(ABI::V1), ABI::V1)
                .expect("directory classification");

        assert!(access.contains(AccessFs::ReadDir));
        assert!(access.contains(AccessFs::ReadFile));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_access_removes_directory_rights_for_regular_files() {
        use landlock::{AccessFs, ABI};

        let file = tempfile::NamedTempFile::new().expect("file fixture");
        let access = landlock_access_for_path(file.path(), AccessFs::from_read(ABI::V1), ABI::V1)
            .expect("regular-file classification");

        assert!(!access.contains(AccessFs::ReadDir));
        assert!(access.contains(AccessFs::ReadFile));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_access_removes_directory_rights_for_special_files() {
        use landlock::{Access, AccessFs, ABI};

        let path = Path::new("/dev/null");
        if !path.exists() {
            return;
        }
        let access = landlock_access_for_path(path, AccessFs::from_all(ABI::V1), ABI::V1)
            .expect("special-file classification");

        assert!(!access.contains(AccessFs::ReadDir));
        assert!(access.contains(AccessFs::ReadFile));
        assert!(access.contains(AccessFs::WriteFile));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_access_fails_closed_when_path_cannot_be_classified() {
        use landlock::{AccessFs, ABI};

        let path = Path::new("/definitely/missing/codegg-sandbox-path");
        let error = landlock_access_for_path(path, AccessFs::from_read(ABI::V1), ABI::V1)
            .expect_err("missing path classification must fail");

        assert!(error.contains("classify sandbox path"));
        assert!(error.contains(path.to_string_lossy().as_ref()));
    }

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

    #[cfg(unix)]
    #[test]
    fn trusted_helper_resolution_ignores_inherited_override() {
        use std::os::unix::fs::PermissionsExt;

        let install = tempfile::tempdir().expect("installation directory");
        let executable = install.path().join("codegg");
        let helper = install.path().join("codegg-sandbox-helper");
        std::fs::write(&executable, b"codegg").expect("executable fixture");
        std::fs::write(&helper, b"helper").expect("helper fixture");
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755))
            .expect("helper executable permissions");

        let variable = ["CODEGG", "SANDBOX", "HELPER"].join("_");
        let substitution = install.path().join("substituted-helper");
        std::fs::write(&substitution, b"substitution").expect("substitution fixture");
        std::env::set_var(&variable, &substitution);
        let resolved = resolve_trusted_helper(&executable).expect("trusted sibling helper");
        std::env::remove_var(variable);

        assert_eq!(resolved, helper.canonicalize().expect("canonical helper"));
    }

    #[test]
    fn status_decoder_rejects_malformed_duplicate_and_oversized_frames() {
        let enforced = encode_sandbox_status(SandboxLaunchOutcome::Enforced { abi: 9 })
            .expect("enforced frame");
        let setup = encode_sandbox_status(SandboxLaunchOutcome::SetupError {
            reason: "bad rule".to_string(),
        })
        .expect("setup frame");
        assert!(decode_sandbox_status(&enforced[..enforced.len() - 1]).is_err());
        assert!(decode_sandbox_status(&[enforced.clone(), enforced.clone()].concat()).is_err());
        assert!(decode_sandbox_status(&[enforced, setup].concat()).is_err());
        assert!(decode_sandbox_status(&vec![0_u8; MAX_SANDBOX_STATUS_BYTES + 1]).is_err());
    }
}

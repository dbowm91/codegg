use crate::error::ToolError;
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
    pub fn access_flags(&self) -> u64 {
        match self {
            SandboxMode::ReadOnly => 1 << 0,
            SandboxMode::WorkspaceWrite => (1 << 0) | (1 << 1),
            SandboxMode::DangerFullAccess => (1 << 0) | (1 << 1) | (1 << 2),
        }
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
            landlock_is_supported()
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    pub fn enforce(&self) -> Result<(), ToolError> {
        if !self.enabled {
            return Ok(());
        }

        #[cfg(target_os = "linux")]
        {
            if !Self::is_available() {
                tracing::warn!(
                    "Landlock is not available on this system, skipping sandbox enforcement"
                );
                return Ok(());
            }

            enforce_landlock(&self.allowed_paths, &self.deny_paths, &self.mode)?;
            tracing::info!(
                "Landlock sandbox enforced with {} allowed paths",
                self.allowed_paths.len()
            );
        }

        #[cfg(not(target_os = "linux"))]
        {
            tracing::warn!("Landlock is not available on this system");
        }

        Ok(())
    }
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn landlock_is_supported() -> bool {
    std::path::Path::new("/sys/kernel/security/landlock").exists()
        || std::fs::read_to_string("/proc/filesystems")
            .map(|fs| fs.contains("landlock"))
            .unwrap_or(false)
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn enforce_landlock(
    allowed_paths: &[String],
    deny_paths: &[String],
    mode: &SandboxMode,
) -> Result<(), ToolError> {
    use std::ffi::CString;

    const LANDLOCK_RULE_PATH_FD: u32 = 1;

    const PR_GET_LANDLOCK: libc::c_int = 2;

    const SYS_LANDLOCK_CREATE_RULESET: libc::c_long = 451;
    const SYS_LANDLOCK_ADD_RULE: libc::c_long = 452;
    const SYS_LANDLOCK_RESTRICT_SELF: libc::c_long = 453;

    #[repr(C, packed)]
    struct LandlockRulesetAttr {
        handled_access_fs: u64,
    }

    #[repr(C, packed)]
    struct LandlockPathBeneathAttr {
        allowed_access: u64,
        parent_fd: libc::c_int,
    }

    if unsafe { libc::prctl(PR_GET_LANDLOCK, 0, 0, 0, 0) } < 0 {
        return Err(ToolError::Permission("Landlock not available".to_string()));
    }

    let handled_access = mode.access_flags();

    let attr = LandlockRulesetAttr {
        handled_access_fs: handled_access,
    };

    #[allow(unused_unsafe)]
    let ruleset_fd = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            &attr as *const _ as libc::c_long,
            std::mem::size_of::<LandlockRulesetAttr>() as libc::c_long,
            0 as libc::c_long,
        )
    };

    if ruleset_fd < 0 {
        let errno = unsafe { *libc::__errno_location() };
        tracing::warn!("landlock_create_ruleset failed with errno: {}", errno);
        return Err(ToolError::Permission(format!(
            "landlock_create_ruleset failed (errno {})",
            errno
        )));
    }

    let ruleset_fd = ruleset_fd as libc::c_int;

    for path in allowed_paths {
        let c_path = match CString::new(path.as_str()) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY) };
        if fd < 0 {
            tracing::warn!("Failed to open path {} for landlock: {}", path, unsafe {
                *libc::__errno_location()
            });
            continue;
        }

        let path_attr = LandlockPathBeneathAttr {
            allowed_access: handled_access,
            parent_fd: fd,
        };

        #[allow(unused_unsafe)]
        let result = unsafe {
            libc::syscall(
                SYS_LANDLOCK_ADD_RULE,
                ruleset_fd as libc::c_long,
                LANDLOCK_RULE_PATH_FD as libc::c_long,
                &path_attr as *const _ as libc::c_long,
                0 as libc::c_long,
            )
        };

        unsafe { libc::close(fd) };

        if result < 0 {
            tracing::warn!("Failed to add rule for path {}: {}", path, unsafe {
                *libc::__errno_location()
            });
        }
    }

    for path in deny_paths {
        let c_path = match CString::new(path.as_str()) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY) };
        if fd < 0 {
            continue;
        }

        let path_attr = LandlockPathBeneathAttr {
            allowed_access: 0,
            parent_fd: fd,
        };

        #[allow(unused_unsafe)]
        let _result = unsafe {
            libc::syscall(
                SYS_LANDLOCK_ADD_RULE,
                ruleset_fd as libc::c_long,
                LANDLOCK_RULE_PATH_FD as libc::c_long,
                &path_attr as *const _ as libc::c_long,
                0 as libc::c_long,
            )
        };

        unsafe { libc::close(fd) };
    }

    #[allow(unused_unsafe)]
    let result = unsafe {
        libc::syscall(
            SYS_LANDLOCK_RESTRICT_SELF,
            ruleset_fd as libc::c_long,
            0 as libc::c_long,
        )
    };

    unsafe { libc::close(ruleset_fd) };

    if result < 0 {
        let errno = unsafe { *libc::__errno_location() };
        return Err(ToolError::Permission(format!(
            "landlock_restrict_self failed (errno {})",
            errno
        )));
    }

    Ok(())
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

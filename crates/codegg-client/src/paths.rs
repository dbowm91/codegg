use std::path::{Path, PathBuf};

/// Canonical address for a same-machine daemon connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalEndpoint {
    #[cfg(unix)]
    Unix(PathBuf),
    #[cfg(windows)]
    WindowsPipe(String),
}

impl LocalEndpoint {
    /// Parse a platform-local endpoint URI (or its platform-native form).
    pub fn parse(value: &str) -> Result<Self, String> {
        #[cfg(unix)]
        {
            if value.contains("://") && !value.starts_with("unix://") {
                return Err(format!("unsupported local endpoint scheme: {value}"));
            }
            let path = value.strip_prefix("unix://").unwrap_or(value);
            if path.contains("://") {
                return Err(format!("unsupported local endpoint scheme: {value}"));
            }
            if path.is_empty() {
                return Err("Unix daemon endpoint path is empty".to_owned());
            }
            return Ok(Self::Unix(PathBuf::from(path)));
        }
        #[cfg(windows)]
        {
            if value.contains("://") && !value.starts_with("npipe://") {
                return Err(format!("unsupported local endpoint scheme: {value}"));
            }
            let name = value.strip_prefix("npipe://").unwrap_or(value);
            if name.is_empty() {
                return Err("Windows named-pipe endpoint is empty".to_owned());
            }
            let name = name.strip_prefix(r"\\.\pipe\").unwrap_or(name);
            if name.contains('\\') || name.contains('/') || name.contains('\0') {
                return Err("Windows pipe name must be a single local name".to_owned());
            }
            return Ok(Self::WindowsPipe(name.to_owned()));
        }
        #[allow(unreachable_code)]
        Err("local daemon transport is unsupported on this platform".to_owned())
    }

    pub fn as_uri(&self) -> String {
        #[cfg(unix)]
        return match self {
            Self::Unix(path) => format!("unix://{}", path.display()),
        };
        #[cfg(windows)]
        return match self {
            Self::WindowsPipe(name) => format!("npipe://{}", name),
        };
        #[cfg(not(any(unix, windows)))]
        match *self {}
    }

    pub fn native_argument(&self) -> String {
        #[cfg(unix)]
        return match self {
            Self::Unix(path) => path.to_string_lossy().into_owned(),
        };
        #[cfg(windows)]
        return match self {
            Self::WindowsPipe(name) => format!(r"\\.\pipe\{name}"),
        };
        #[cfg(not(any(unix, windows)))]
        match *self {}
    }
}

/// Frontend-safe view of the shared user-scoped daemon locations.
#[derive(Debug, Clone)]
pub struct LocalDaemonPaths {
    pub root: PathBuf,
    pub lock_path: PathBuf,
    pub metadata_path: PathBuf,
    pub socket_path: PathBuf,
    pub log_path: PathBuf,
}

impl LocalDaemonPaths {
    pub fn resolve() -> Self {
        let root = std::env::var_os("CODEGG_DAEMON_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(default_user_runtime_root);
        Self::with_root(root)
    }

    pub fn resolve_for_endpoint(explicit_endpoint: Option<&str>) -> Self {
        let endpoint = explicit_endpoint
            .map(str::to_owned)
            .or_else(|| std::env::var("CODEGG_CORE_ENDPOINT").ok());
        let paths = Self::resolve();
        endpoint
            .as_deref()
            .map(Self::normalize_endpoint)
            .map_or(paths.clone(), |socket| paths.with_socket(socket))
    }

    pub fn with_root(root: PathBuf) -> Self {
        #[cfg(unix)]
        let socket_path = root.join("core.sock");
        #[cfg(windows)]
        let socket_path = {
            // Fixed FNV-1a keeps the endpoint stable across Rust releases so
            // an upgraded frontend and an already-running daemon agree.
            let hash = root
                .to_string_lossy()
                .bytes()
                .fold(0xcbf29ce484222325_u64, |hash, byte| {
                    (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
                });
            PathBuf::from(format!(r"\\.\pipe\codegg-{hash:016x}"))
        };
        #[cfg(not(any(unix, windows)))]
        let socket_path = root.join("core.sock");
        Self {
            lock_path: root.join("daemon.lock"),
            metadata_path: root.join("daemon.json"),
            socket_path,
            log_path: root.join("daemon.log"),
            root,
        }
    }

    pub fn normalize_endpoint(endpoint: &str) -> PathBuf {
        LocalEndpoint::parse(endpoint)
            .map(|endpoint| PathBuf::from(endpoint.native_argument()))
            .unwrap_or_else(|_| PathBuf::from(endpoint))
    }

    pub fn with_socket(&self, socket_path: PathBuf) -> Self {
        let mut paths = self.clone();
        paths.socket_path = socket_path;
        paths
    }

    pub fn endpoint_uri(&self) -> String {
        LocalEndpoint::parse(&self.socket_path_str())
            .map(|endpoint| endpoint.as_uri())
            .unwrap_or_else(|_| self.socket_path.to_string_lossy().into_owned())
    }

    pub fn socket_path_str(&self) -> String {
        self.socket_path.to_string_lossy().into_owned()
    }

    pub fn ensure_root(&self) -> std::io::Result<()> {
        if !self.root.exists() {
            std::fs::create_dir_all(&self.root)?;
            set_user_only_permissions(&self.root);
        }
        Ok(())
    }
}

fn default_user_runtime_root() -> PathBuf {
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("codegg");
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
            return PathBuf::from(runtime).join("codegg");
        }
        if let Some(data) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(data).join("codegg");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("codegg");
        }
    }
    #[cfg(unix)]
    return PathBuf::from("/tmp/codegg");
    #[cfg(not(unix))]
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("codegg")
}

#[cfg(unix)]
fn set_user_only_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(metadata) = std::fs::metadata(path) {
        let mut permissions = metadata.permissions();
        let mode = if metadata.is_dir() { 0o700 } else { 0o600 };
        permissions.set_mode(mode);
        let _ = std::fs::set_permissions(path, permissions);
    }
}

#[cfg(not(unix))]
fn set_user_only_permissions(_path: &Path) {}

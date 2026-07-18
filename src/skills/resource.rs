use std::fmt;
use std::fs::File;
use std::io::{Read, Take};
use std::path::{Component, Path, PathBuf};

/// Bounds applied when a discovered skill resource is opened or read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceReadLimits {
    /// The largest file a handle may represent.
    pub max_resource_size: u64,
    /// The largest number of bytes a read operation may return.
    pub max_bytes_returned: usize,
}

impl ResourceReadLimits {
    pub const DEFAULT_MAX_RESOURCE_SIZE: u64 = 1024 * 1024;
    pub const DEFAULT_MAX_BYTES_RETURNED: usize = 64 * 1024;

    pub const fn new(max_resource_size: u64, max_bytes_returned: usize) -> Self {
        Self {
            max_resource_size,
            max_bytes_returned,
        }
    }

    /// Compatibility spelling for callers that describe the read bound as a
    /// maximum read size.
    pub const fn max_read_bytes(self) -> usize {
        self.max_bytes_returned
    }
}

impl Default for ResourceReadLimits {
    fn default() -> Self {
        Self::new(
            Self::DEFAULT_MAX_RESOURCE_SIZE,
            Self::DEFAULT_MAX_BYTES_RETURNED,
        )
    }
}

/// A validated, lazy reference to one file inside a skill package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceHandle {
    package_root: PathBuf,
    relative_path: PathBuf,
    limits: ResourceReadLimits,
}

impl ResourceHandle {
    /// Construct a handle after validating the relative path and package
    /// containment. This only reads filesystem metadata; it never reads the
    /// resource body.
    pub fn new(
        package_root: impl AsRef<Path>,
        relative_path: impl AsRef<Path>,
        limits: ResourceReadLimits,
    ) -> Result<Self, ResourceError> {
        let relative_path = relative_path.as_ref();
        Self::validate_relative_path(relative_path)?;

        let package_root =
            package_root
                .as_ref()
                .canonicalize()
                .map_err(|source| ResourceError::Io {
                    path: package_root.as_ref().to_path_buf(),
                    source,
                })?;
        let handle = Self {
            package_root,
            relative_path: relative_path.to_path_buf(),
            limits,
        };
        let (_, size) = handle.resolve()?;
        if size > limits.max_resource_size {
            return Err(ResourceError::ResourceTooLarge {
                path: handle.relative_path.clone(),
                size,
                maximum: limits.max_resource_size,
            });
        }
        Ok(handle)
    }

    /// Reject absolute paths and every explicit parent-directory component.
    /// Backslashes are rejected too so a handle has the same path semantics
    /// on Unix and Windows.
    pub fn validate_relative_path(path: &Path) -> Result<(), ResourceError> {
        if path.as_os_str().is_empty() {
            return Err(ResourceError::InvalidPath(
                "resource path must not be empty".to_string(),
            ));
        }
        if path.is_absolute() {
            return Err(ResourceError::InvalidPath(
                "resource path must be relative".to_string(),
            ));
        }
        if path.to_string_lossy().contains('\\') {
            return Err(ResourceError::InvalidPath(
                "resource path must not contain backslash separators".to_string(),
            ));
        }
        for component in path.components() {
            match component {
                Component::ParentDir => {
                    return Err(ResourceError::InvalidPath(
                        "resource path must not contain '..'".to_string(),
                    ));
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(ResourceError::InvalidPath(
                        "resource path must be relative".to_string(),
                    ));
                }
                Component::CurDir | Component::Normal(_) => {}
            }
        }
        Ok(())
    }

    pub fn package_root(&self) -> &Path {
        &self.package_root
    }

    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    pub fn limits(&self) -> ResourceReadLimits {
        self.limits
    }

    /// Read the resource with both the file-size and returned-byte bounds
    /// applied. The body is read only when this method is called.
    pub fn read_bytes(&self) -> Result<Vec<u8>, ResourceError> {
        let (path, size) = self.resolve()?;
        self.check_limits(size)?;

        let mut file = File::open(&path).map_err(|source| ResourceError::Io {
            path: path.clone(),
            source,
        })?;
        let file_size = file
            .metadata()
            .map_err(|source| ResourceError::Io {
                path: path.clone(),
                source,
            })?
            .len();
        self.check_limits(file_size)?;

        let read_limit = self.limits.max_bytes_returned as u64;
        let mut bounded_reader: Take<&mut File> = file.by_ref().take(read_limit.saturating_add(1));
        let capacity = file_size.min(self.limits.max_bytes_returned as u64) as usize;
        let mut bytes = Vec::with_capacity(capacity);
        bounded_reader
            .read_to_end(&mut bytes)
            .map_err(|source| ResourceError::Io {
                path: path.clone(),
                source,
            })?;
        if bytes.len() > self.limits.max_bytes_returned {
            return Err(ResourceError::ReadTooLarge {
                path: self.relative_path.clone(),
                size: bytes.len() as u64,
                maximum: self.limits.max_bytes_returned,
            });
        }
        Ok(bytes)
    }

    pub fn read_text(&self) -> Result<String, ResourceError> {
        String::from_utf8(self.read_bytes()?).map_err(|source| ResourceError::InvalidUtf8 {
            path: self.relative_path.clone(),
            source,
        })
    }

    fn check_limits(&self, size: u64) -> Result<(), ResourceError> {
        if size > self.limits.max_resource_size {
            return Err(ResourceError::ResourceTooLarge {
                path: self.relative_path.clone(),
                size,
                maximum: self.limits.max_resource_size,
            });
        }
        if size > self.limits.max_bytes_returned as u64 {
            return Err(ResourceError::ReadTooLarge {
                path: self.relative_path.clone(),
                size,
                maximum: self.limits.max_bytes_returned,
            });
        }
        Ok(())
    }

    fn resolve(&self) -> Result<(PathBuf, u64), ResourceError> {
        // Validate again at read time so the handle remains safe if its
        // caller retains it while the package is refreshed or replaced.
        Self::validate_relative_path(&self.relative_path)?;
        let candidate = self.package_root.join(&self.relative_path);
        let canonical = candidate
            .canonicalize()
            .map_err(|source| ResourceError::Io {
                path: self.relative_path.clone(),
                source,
            })?;
        if !canonical.starts_with(&self.package_root) {
            return Err(ResourceError::SymlinkEscape {
                path: self.relative_path.clone(),
            });
        }
        let metadata = std::fs::metadata(&canonical).map_err(|source| ResourceError::Io {
            path: self.relative_path.clone(),
            source,
        })?;
        if !metadata.is_file() {
            return Err(ResourceError::NotAFile {
                path: self.relative_path.clone(),
            });
        }
        Ok((canonical, metadata.len()))
    }
}

#[derive(Debug)]
pub enum ResourceError {
    InvalidPath(String),
    SymlinkEscape {
        path: PathBuf,
    },
    NotAFile {
        path: PathBuf,
    },
    ResourceTooLarge {
        path: PathBuf,
        size: u64,
        maximum: u64,
    },
    ReadTooLarge {
        path: PathBuf,
        size: u64,
        maximum: usize,
    },
    NotFound {
        skill: String,
        path: PathBuf,
    },
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    InvalidUtf8 {
        path: PathBuf,
        source: std::string::FromUtf8Error,
    },
}

impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath(reason) => write!(f, "invalid resource path: {reason}"),
            Self::SymlinkEscape { path } => {
                write!(f, "resource escapes its skill package: {}", path.display())
            }
            Self::NotAFile { path } => write!(f, "resource is not a file: {}", path.display()),
            Self::ResourceTooLarge {
                path,
                size,
                maximum,
            } => write!(
                f,
                "resource {} is {size} bytes, exceeding the {maximum}-byte size limit",
                path.display()
            ),
            Self::ReadTooLarge {
                path,
                size,
                maximum,
            } => write!(
                f,
                "resource {} requires {size} bytes, exceeding the {maximum}-byte read limit",
                path.display()
            ),
            Self::NotFound { skill, path } => write!(
                f,
                "resource {} was not discovered for skill '{skill}'",
                path.display()
            ),
            Self::Io { path, source } => write!(f, "resource {}: {source}", path.display()),
            Self::InvalidUtf8 { path, .. } => {
                write!(f, "resource is not valid UTF-8: {}", path.display())
            }
        }
    }
}

impl std::error::Error for ResourceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::InvalidUtf8 { source, .. } => Some(source),
            _ => None,
        }
    }
}

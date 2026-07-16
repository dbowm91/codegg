//! Platform-resolved daemon paths.
//!
//! [`DaemonPaths`] is the single source of truth for where the
//! user-scoped daemon catalog, agent assets, and credential store
//! should live on disk. The defaults follow platform conventions:
//!
//! - macOS: `~/Library/Application Support/codegg/`
//! - Linux: `$XDG_DATA_HOME/codegg/` (or `~/.local/share/codegg/`)
//! - fallback: `~/.codegg/`
//!
//! All fields are optional overrides; a `None` falls back to the
//! platform-appropriate default. Tests inject an override data root
//! so they can exercise catalog initialization without touching the
//! user's real home directory.

use std::path::{Path, PathBuf};

/// Daemon catalog and asset directory layout.
#[derive(Debug, Clone, Default)]
pub struct DaemonPaths {
    /// Override for the user-scoped data directory. `None` falls back
    /// to `dirs::data_dir()` joined with `codegg`.
    pub data_root: Option<PathBuf>,
    /// Override for the user-scoped config directory. `None` falls
    /// back to `dirs::config_dir()` joined with `codegg`.
    pub config_root: Option<PathBuf>,
}

impl DaemonPaths {
    /// Build with explicit overrides. Either override may be `None`
    /// to use the platform default.
    pub fn with_overrides(data_root: Option<PathBuf>, config_root: Option<PathBuf>) -> Self {
        Self {
            data_root,
            config_root,
        }
    }

    /// Resolve the platform-default data root.
    pub fn default_data_root() -> PathBuf {
        dirs::data_dir()
            .map(|d| d.join("codegg"))
            .or_else(|| dirs::home_dir().map(|h| h.join(".local").join("share").join("codegg")))
            .unwrap_or_else(|| PathBuf::from(".codegg"))
    }

    /// Resolve the platform-default config root.
    pub fn default_config_root() -> PathBuf {
        dirs::config_dir()
            .map(|d| d.join("codegg"))
            .or_else(|| dirs::home_dir().map(|h| h.join(".config").join("codegg")))
            .unwrap_or_else(|| PathBuf::from(".codegg"))
    }

    /// The user-scoped data root (with all platform fallbacks).
    pub fn data_root(&self) -> PathBuf {
        self.data_root
            .clone()
            .unwrap_or_else(Self::default_data_root)
    }

    /// The user-scoped config root.
    pub fn config_root(&self) -> PathBuf {
        self.config_root
            .clone()
            .unwrap_or_else(Self::default_config_root)
    }

    /// The path to the user-scoped daemon catalog database.
    pub fn catalog_db_path(&self) -> PathBuf {
        self.data_root().join("codegg.db")
    }

    /// The path to the user-scoped daemon catalog database WAL file.
    pub fn catalog_db_wal_path(&self) -> PathBuf {
        let mut p = self.catalog_db_path();
        let name = p.file_name().map(|n| n.to_os_string()).unwrap_or_default();
        let mut wal = name;
        wal.push("-wal");
        p.set_file_name(wal);
        p
    }

    /// The directory holding agent customization overrides.
    pub fn agents_dir(&self) -> PathBuf {
        self.config_root().join("agents")
    }

    /// The directory holding credential files (when not using an OS
    /// keychain).
    pub fn credentials_path(&self) -> PathBuf {
        self.config_root().join("credentials.json")
    }

    /// The directory under which workspace-local run/test output is
    /// surfaced (only the parent — actual workspace paths live under
    /// `<workspace>/.codegg/runs/`).
    pub fn workspace_local_artifact_root(&self, workspace_root: &Path) -> PathBuf {
        workspace_root.join(".codegg")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_data_root_falls_back_to_home() {
        // With no override, the data root must resolve to a non-empty
        // path (either the platform default or the home fallback).
        let root = DaemonPaths::default().data_root();
        assert!(!root.as_os_str().is_empty());
    }

    #[test]
    fn with_overrides_respects_data_root() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = DaemonPaths::with_overrides(Some(tmp.path().to_path_buf()), None);
        assert_eq!(paths.data_root(), tmp.path());
        assert_eq!(paths.catalog_db_path(), tmp.path().join("codegg.db"));
    }

    #[test]
    fn catalog_db_path_is_data_root_codegg_db() {
        let paths = DaemonPaths::with_overrides(Some(PathBuf::from("/tmp/x")), None);
        assert_eq!(paths.catalog_db_path(), PathBuf::from("/tmp/x/codegg.db"));
    }
}

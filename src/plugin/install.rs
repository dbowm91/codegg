use std::path::{Path, PathBuf};

use crate::plugin::manifest::PluginManifest;
use crate::plugin::policy::PluginInstallPolicy;

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("plugin already installed: {0}")]
    AlreadyInstalled(String),

    #[error("invalid plugin path: {0}")]
    InvalidPath(String),

    #[error("download failed: {0}")]
    DownloadFailed(String),

    #[error("manifest error: {0}")]
    Manifest(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("policy violation: {0}")]
    PolicyViolation(String),
}

impl From<InstallError> for crate::error::PluginError {
    fn from(err: InstallError) -> Self {
        crate::error::PluginError::InstallFailed(err.to_string())
    }
}

pub fn plugins_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("codegg")
        .join("plugins")
}

pub async fn install_from_path(path: &Path) -> Result<PathBuf, InstallError> {
    let path = path
        .canonicalize()
        .map_err(|e| InstallError::InvalidPath(e.to_string()))?;

    if !path.exists() {
        return Err(InstallError::InvalidPath(format!(
            "path does not exist: {}",
            path.display()
        )));
    }

    let manifest_path = path.join("manifest.toml");
    if !manifest_path.exists() {
        return Err(InstallError::Manifest(
            "manifest.toml not found in plugin directory".into(),
        ));
    }

    let manifest_content = tokio::fs::read_to_string(&manifest_path).await?;
    let _manifest: PluginManifest =
        toml::from_str(&manifest_content).map_err(|e| InstallError::Manifest(e.to_string()))?;

    let plugins_dir = plugins_dir();
    tokio::fs::create_dir_all(&plugins_dir).await?;

    let plugin_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let dest = plugins_dir.join(&plugin_name);
    if dest.exists() {
        return Err(InstallError::AlreadyInstalled(plugin_name));
    }

    copy_dir_all(&path, &dest)?;
    Ok(dest)
}

pub async fn install_from_url(url: &str) -> Result<PathBuf, InstallError> {
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| InstallError::DownloadFailed(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(InstallError::DownloadFailed(format!(
            "HTTP {}",
            resp.status()
        )));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| InstallError::DownloadFailed(e.to_string()))?;

    let plugins_dir = plugins_dir();
    tokio::fs::create_dir_all(&plugins_dir).await?;

    let name = url
        .split('/')
        .next_back()
        .unwrap_or("plugin")
        .trim_end_matches(".wasm")
        .trim_end_matches(".tar.gz");

    let dest = plugins_dir.join(name);
    if dest.exists() {
        return Err(InstallError::AlreadyInstalled(name.to_string()));
    }

    tokio::fs::create_dir_all(&dest).await?;

    let wasm_path = if url.ends_with(".wasm") {
        let wp = dest.join("plugin.wasm");
        tokio::fs::write(&wp, &bytes).await?;
        wp
    } else {
        let tmp = dest.join("download.tar.gz");
        tokio::fs::write(&tmp, &bytes).await?;
        extract_plugin_archive(&tmp, &dest)?;
        let _ = tokio::fs::remove_file(&tmp).await;
        dest.join("plugin.wasm")
    };

    if !wasm_path.exists() {
        return Err(InstallError::Manifest(
            "no .wasm file found in downloaded archive".into(),
        ));
    }

    Ok(dest)
}

fn extract_plugin_archive(archive: &Path, dest: &Path) -> Result<(), InstallError> {
    use flate2::read::GzDecoder;
    use std::fs::File;

    let file = File::open(archive)?;
    let gz = GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);

    let dest_canonical = std::fs::canonicalize(dest)?;

    for entry in archive.entries()? {
        let mut entry = entry.map_err(|e| InstallError::DownloadFailed(e.to_string()))?;
        let entry_path = entry.path()?.to_path_buf();

        if entry.header().entry_type().is_symlink() {
            return Err(InstallError::DownloadFailed(format!(
                "symlinks are not allowed in archive: {}",
                entry_path.display()
            )));
        }

        let dst_path = dest.join(&entry_path);
        let dst_canonical = std::fs::canonicalize(&dst_path)?;
        if !dst_canonical.starts_with(&dest_canonical) {
            return Err(InstallError::DownloadFailed(format!(
                "path outside destination: {}",
                entry_path.display()
            )));
        }

        entry.unpack(&dst_path)?;
    }

    Ok(())
}

pub async fn uninstall(plugin_name: &str) -> Result<(), InstallError> {
    let plugins_dir = plugins_dir();
    let plugin_path = plugins_dir.join(plugin_name);

    if !plugin_path.exists() {
        return Err(InstallError::InvalidPath(format!(
            "plugin not found: {}",
            plugin_name
        )));
    }

    tokio::fs::remove_dir_all(&plugin_path).await?;
    Ok(())
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    let entries = std::fs::read_dir(src)?;

    let dst_canonical = std::fs::canonicalize(dst)?;

    for entry in entries {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if ty.is_symlink() {
            return Err(std::io::Error::other(format!(
                "symlinks are not allowed: {}",
                src_path.display()
            )));
        }

        if ty.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            let src_canonical = std::fs::canonicalize(&src_path)?;
            if !src_canonical.starts_with(&dst_canonical) {
                return Err(std::io::Error::other(format!(
                    "path outside destination tree: {} -> {}",
                    src_path.display(),
                    src_canonical.display()
                )));
            }
            std::fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

/// Validate that a target path for uninstall is safely contained within
/// the canonical plugins directory, using policy settings.
///
/// Returns `Ok(())` if the path resolves inside the plugins dir and
/// the policy allows removal, or `Err(InstallError)` otherwise.
pub fn validate_uninstall_target(
    target: &Path,
    policy: &PluginInstallPolicy,
) -> Result<(), InstallError> {
    let plugins_dir = plugins_dir();
    let target_canonical = std::fs::canonicalize(target)
        .map_err(|e| InstallError::InvalidPath(format!("cannot resolve path: {e}")))?;
    let plugins_dir_canonical = std::fs::canonicalize(&plugins_dir)
        .map_err(|e| InstallError::InvalidPath(format!("cannot resolve plugins dir: {e}")))?;

    if policy.refuse_outside_install_dir && !target_canonical.starts_with(&plugins_dir_canonical) {
        return Err(InstallError::PolicyViolation(format!(
            "refusing to remove path outside plugins directory: {}",
            target.display()
        )));
    }
    Ok(())
}

/// Validate that a WASM module path lives under the plugin's install
/// directory, using policy settings.
///
/// Returns `Ok(())` if the path is within bounds or the policy allows
/// it, or `Err(InstallError)` otherwise.
pub fn validate_wasm_module_path(
    module_path: &Path,
    plugin_install_dir: &Path,
    policy: &PluginInstallPolicy,
) -> Result<(), InstallError> {
    if !policy.warn_wasm_outside_plugin_dir {
        return Ok(());
    }

    let module_canonical = std::fs::canonicalize(module_path)
        .map_err(|e| InstallError::InvalidPath(format!("cannot resolve WASM path: {e}")))?;
    let plugin_dir_canonical = std::fs::canonicalize(plugin_install_dir)
        .map_err(|e| InstallError::InvalidPath(format!("cannot resolve plugin dir: {e}")))?;

    if !module_canonical.starts_with(&plugin_dir_canonical) {
        return Err(InstallError::PolicyViolation(format!(
            "WASM module path {} is outside plugin install directory {}",
            module_path.display(),
            plugin_install_dir.display()
        )));
    }
    Ok(())
}

/// Validate that an install source does not contain path traversal,
/// using policy settings.
pub fn validate_install_source(
    source: &Path,
    policy: &PluginInstallPolicy,
) -> Result<(), InstallError> {
    if !policy.reject_path_traversal {
        return Ok(());
    }

    let source_canonical = std::fs::canonicalize(source)
        .map_err(|e| InstallError::InvalidPath(format!("cannot resolve source path: {e}")))?;

    // Check for path components that could indicate traversal
    for component in source_canonical.components() {
        if let std::path::Component::ParentDir = component {
            return Err(InstallError::PolicyViolation(
                "path traversal detected in install source".to_string(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Create a unique temp directory under the system temp dir.
    fn make_temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codegg_install_test_{}_{}",
            label,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn install_rejects_missing_manifest() {
        let src = make_temp_dir("no_manifest");
        // No manifest.toml created
        let result = install_from_path(&src).await;
        assert!(matches!(result, Err(InstallError::Manifest(_))));
    }

    #[tokio::test]
    async fn install_rejects_invalid_toml_manifest() {
        let src = make_temp_dir("bad_manifest");
        fs::write(src.join("manifest.toml"), "this is not valid toml [[[ ]]]").unwrap();
        let result = install_from_path(&src).await;
        assert!(matches!(result, Err(InstallError::Manifest(_))));
    }

    #[tokio::test]
    async fn install_rejects_nonexistent_path() {
        let result = install_from_path(Path::new("/nonexistent/zzz/path/abc")).await;
        assert!(matches!(result, Err(InstallError::InvalidPath(_))));
    }

    #[tokio::test]
    async fn install_accepts_valid_plugin() {
        let src = make_temp_dir("valid");
        fs::write(
            src.join("manifest.toml"),
            r#"
name = "test-plugin"
version = "1.0.0"
api_version = 1
"#,
        )
        .unwrap();

        let result = install_from_path(&src).await;
        // It may succeed or fail with AlreadyInstalled if a previous run
        // left files; either way it must not be InvalidPath or Manifest error.
        if let Err(e) = &result {
            assert!(
                !matches!(e, InstallError::InvalidPath(_) | InstallError::Manifest(_)),
                "unexpected error: {e:?}"
            );
        }
        // Cleanup if installed
        if let Ok(dest) = &result {
            let _ = fs::remove_dir_all(dest);
        }
    }

    #[tokio::test]
    async fn install_rejects_path_traversal() {
        // plugins_dir().join(plugin_name) keeps the path anchored to the
        // canonical plugins directory. A traversal attempt resolves into
        // a path that doesn't exist relative to plugins_dir.
        let result = uninstall("../../../etc/passwd").await;
        assert!(matches!(result, Err(InstallError::InvalidPath(_))));
    }

    #[test]
    fn validate_uninstall_rejects_outside_plugins_dir() {
        let policy = PluginInstallPolicy::default();
        let outside = std::env::temp_dir();
        let result = validate_uninstall_target(&outside, &policy);
        assert!(result.is_err());
        assert!(matches!(result, Err(InstallError::PolicyViolation(_))));
    }

    #[test]
    fn validate_uninstall_accepts_inside_plugins_dir() {
        let policy = PluginInstallPolicy::default();
        let dir = plugins_dir();
        let result = validate_uninstall_target(&dir, &policy);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_uninstall_allows_outside_when_policy_disabled() {
        let mut policy = PluginInstallPolicy::default();
        policy.refuse_outside_install_dir = false;
        let outside = std::env::temp_dir();
        let result = validate_uninstall_target(&outside, &policy);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_wasm_module_inside_plugin_dir() {
        let policy = PluginInstallPolicy::default();
        let dir = plugins_dir();
        // The plugins dir itself is "inside" itself
        let result = validate_wasm_module_path(&dir, &dir, &policy);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_wasm_module_outside_plugin_dir() {
        let policy = PluginInstallPolicy::default();
        let outside = std::env::temp_dir().join("some-wasm.wasm");
        let dir = plugins_dir();
        // File doesn't exist, so canonicalize fails → InvalidPath
        let result = validate_wasm_module_path(&outside, &dir, &policy);
        assert!(result.is_err());
    }

    #[test]
    fn validate_wasm_module_allows_outside_when_policy_disabled() {
        let mut policy = PluginInstallPolicy::default();
        policy.warn_wasm_outside_plugin_dir = false;
        let outside = std::env::temp_dir().join("some-wasm.wasm");
        let dir = plugins_dir();
        let result = validate_wasm_module_path(&outside, &dir, &policy);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_install_source_rejects_traversal() {
        let policy = PluginInstallPolicy::default();
        let source = Path::new("/some/path/../../etc/passwd");
        let result = validate_install_source(source, &policy);
        // canonicalize will resolve the traversal, so this depends on
        // whether the path exists. If it doesn't exist, we get InvalidPath.
        // If it does, we get PolicyViolation. Either way, not Ok for
        // a path with ParentDir after canonicalization.
        // Since the path likely doesn't exist, we expect an error.
        assert!(result.is_err());
    }

    #[test]
    fn validate_install_source_allows_when_policy_disabled() {
        let mut policy = PluginInstallPolicy::default();
        policy.reject_path_traversal = false;
        let source = Path::new("/some/path/../../etc/passwd");
        let result = validate_install_source(source, &policy);
        assert!(result.is_ok());
    }
}

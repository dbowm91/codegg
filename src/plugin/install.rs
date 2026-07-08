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

/// Validate that a relative path contains no traversal components.
///
/// Only `Normal` and `CurDir` components are allowed. `ParentDir`,
/// `RootDir`, and `Prefix` components are rejected as path traversal.
fn validate_relative_install_path(rel: &Path) -> Result<(), String> {
    for component in rel.components() {
        match component {
            std::path::Component::Normal(_) => {}
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Err(format!(
                    "path traversal or absolute component not allowed: {}",
                    rel.display()
                ));
            }
        }
    }
    Ok(())
}

/// Validate that a user-supplied local install source path is safe to use.
///
/// Unlike `validate_relative_install_path`, this helper accepts absolute paths
/// and `..` components as long as the canonicalized target exists, is a
/// directory, and contains a `manifest.toml`. This is appropriate for a local
/// install command where the user explicitly chose the path; it is NOT
/// appropriate for archive entries or copy-relative paths, which must remain
/// strictly relative.
///
/// Returns the canonicalized path on success.
pub fn validate_local_install_source(
    source: &Path,
    policy: &PluginInstallPolicy,
) -> Result<PathBuf, InstallError> {
    if !policy.reject_path_traversal {
        // When traversal policy is disabled, still canonicalize but don't
        // lexically reject components.
        return std::fs::canonicalize(source)
            .map_err(|e| InstallError::InvalidPath(format!("cannot resolve source path: {e}")));
    }

    // Canonicalize first so we can validate existence and directory-ness
    // without surprising the user with extra traversal rejection at this layer.
    // The user-supplied path is a deliberate choice; traversal here is benign
    // as long as the canonical target is a real directory with a manifest.
    let canonical = std::fs::canonicalize(source)
        .map_err(|e| InstallError::InvalidPath(format!("cannot resolve source path: {e}")))?;

    if !canonical.is_dir() {
        return Err(InstallError::InvalidPath(format!(
            "install source is not a directory: {}",
            canonical.display()
        )));
    }

    let manifest_path = canonical.join("manifest.toml");
    if !manifest_path.is_file() {
        return Err(InstallError::Manifest(format!(
            "manifest.toml not found in install source: {}",
            canonical.display()
        )));
    }

    Ok(canonical)
}

pub async fn install_from_path(path: &Path) -> Result<PathBuf, InstallError> {
    install_from_path_into(path, &plugins_dir()).await
}

/// Install a plugin from a local filesystem path into a caller-supplied
/// destination root.
///
/// The source directory must contain a `manifest.toml`. The plugin is
/// copied into `<dest_root>/<plugin_name>`. Exposing the destination
/// explicitly keeps tests hermetic and gives callers (e.g. sandboxes)
/// control over the install root.
pub async fn install_from_path_into(
    path: &Path,
    dest_root: &Path,
) -> Result<PathBuf, InstallError> {
    // Validate the user-supplied local install source. This accepts
    // absolute paths and paths containing `..` as long as the canonical
    // target exists, is a directory, and contains a `manifest.toml`.
    let policy = PluginInstallPolicy::default();
    let path = validate_local_install_source(path, &policy)?;

    let manifest_path = path.join("manifest.toml");
    let manifest_content = tokio::fs::read_to_string(&manifest_path).await?;
    let _manifest: PluginManifest =
        toml::from_str(&manifest_content).map_err(|e| InstallError::Manifest(e.to_string()))?;

    tokio::fs::create_dir_all(dest_root).await?;

    let plugin_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let dest = dest_root.join(&plugin_name);
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

pub(crate) fn extract_plugin_archive(archive: &Path, dest: &Path) -> Result<(), InstallError> {
    use flate2::read::GzDecoder;
    use std::fs::File;

    let file = File::open(archive)?;
    let gz = GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);

    let dest_root = std::fs::canonicalize(dest)?;

    for entry in archive.entries()? {
        let mut entry = entry.map_err(|e| InstallError::DownloadFailed(e.to_string()))?;
        let entry_path = entry.path()?.to_path_buf();

        // Reject symlinks and hard links
        if entry.header().entry_type().is_symlink() || entry.header().entry_type().is_hard_link() {
            return Err(InstallError::DownloadFailed(format!(
                "symlinks and hard links are not allowed in archive: {}",
                entry_path.display()
            )));
        }

        // Validate path components on the original (non-canonicalized) path
        validate_relative_install_path(&entry_path).map_err(InstallError::DownloadFailed)?;

        // Lexical containment: compute dest path before any canonicalize
        let dst_path = dest_root.join(&entry_path);
        if !dst_path.starts_with(&dest_root) {
            return Err(InstallError::DownloadFailed(format!(
                "path outside destination: {}",
                entry_path.display()
            )));
        }

        // Ensure parent directory exists
        if let Some(parent) = dst_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        entry.unpack(&dst_path)?;
    }

    Ok(())
}

pub async fn uninstall(plugin_name: &str) -> Result<(), InstallError> {
    let policy = PluginInstallPolicy::default();

    // Reject plugin names containing path separators or traversal
    if plugin_name.contains('/') || plugin_name.contains('\\') || plugin_name.contains("..") {
        return Err(InstallError::InvalidPath(format!(
            "invalid plugin name: {}",
            plugin_name
        )));
    }

    let plugins_dir = plugins_dir();
    let plugin_path = plugins_dir.join(plugin_name);

    if !plugin_path.exists() {
        return Err(InstallError::InvalidPath(format!(
            "plugin not found: {}",
            plugin_name
        )));
    }

    validate_uninstall_target(&plugin_path, &policy)?;

    tokio::fs::remove_dir_all(&plugin_path).await?;
    Ok(())
}

pub(crate) fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    let src_root = src.canonicalize()?;
    let dst_root = dst.canonicalize()?;

    fn copy_inner(src_root: &Path, current: &Path, dst_root: &Path) -> std::io::Result<()> {
        for entry in std::fs::read_dir(current)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            let src_path = entry.path();

            if ty.is_symlink() {
                return Err(std::io::Error::other(format!(
                    "symlinks are not allowed: {}",
                    src_path.display()
                )));
            }

            let rel = src_path
                .strip_prefix(src_root)
                .map_err(|_| std::io::Error::other("entry escaped source root"))?;

            validate_relative_install_path(rel).map_err(std::io::Error::other)?;

            let dst_path = dst_root.join(rel);
            if !dst_path.starts_with(dst_root) {
                return Err(std::io::Error::other("destination escaped root"));
            }

            if ty.is_dir() {
                std::fs::create_dir_all(&dst_path)?;
                copy_inner(src_root, &src_path, dst_root)?;
            } else if ty.is_file() {
                if let Some(parent) = dst_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&src_path, &dst_path)?;
            }
        }
        Ok(())
    }

    copy_inner(&src_root, &src_root, &dst_root)
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

    // Check the original (uncanonicalized) path components first.
    // This catches traversal attempts before canonicalize resolves them away.
    for component in source.components() {
        match component {
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Err(InstallError::PolicyViolation(
                    "path traversal detected in install source".to_string(),
                ));
            }
            _ => {}
        }
    }

    // Canonicalize to confirm the path exists and is valid
    let _source_canonical = std::fs::canonicalize(source)
        .map_err(|e| InstallError::InvalidPath(format!("cannot resolve source path: {e}")))?;

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

    #[tokio::test(flavor = "current_thread")]
    async fn install_rejects_missing_manifest() {
        let src = make_temp_dir("no_manifest");
        // No manifest.toml created
        let result = install_from_path(&src).await;
        assert!(matches!(result, Err(InstallError::Manifest(_))));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn install_rejects_invalid_toml_manifest() {
        let src = make_temp_dir("bad_manifest");
        fs::write(src.join("manifest.toml"), "this is not valid toml [[[ ]]]").unwrap();
        let result = install_from_path(&src).await;
        assert!(matches!(result, Err(InstallError::Manifest(_))));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn install_rejects_nonexistent_path() {
        let result = install_from_path(Path::new("/nonexistent/zzz/path/abc")).await;
        assert!(matches!(result, Err(InstallError::InvalidPath(_))));
    }

    #[tokio::test(flavor = "current_thread")]
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

    #[tokio::test(flavor = "current_thread")]
    async fn install_rejects_path_traversal() {
        // plugins_dir().join(plugin_name) keeps the path anchored to the
        // canonical plugins directory. A traversal attempt resolves into
        // a path that doesn't exist relative to plugins_dir.
        let result = uninstall("../../../etc/passwd").await;
        assert!(matches!(result, Err(InstallError::InvalidPath(_))));
    }

    #[test]
    fn validate_uninstall_target_rejects_nonexistent_path() {
        let policy = PluginInstallPolicy::default();
        let result =
            validate_uninstall_target(std::path::Path::new("/nonexistent/zzz/path/abc"), &policy);
        assert!(result.is_err());
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
        let policy = PluginInstallPolicy {
            refuse_outside_install_dir: false,
            ..Default::default()
        };
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
        let policy = PluginInstallPolicy {
            warn_wasm_outside_plugin_dir: false,
            ..Default::default()
        };
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
        // The original path components contain ParentDir, so we detect
        // traversal before canonicalize. This returns PolicyViolation.
        assert!(matches!(result, Err(InstallError::PolicyViolation(_))));
    }

    #[test]
    fn validate_install_source_allows_when_policy_disabled() {
        let policy = PluginInstallPolicy {
            reject_path_traversal: false,
            ..Default::default()
        };
        let source = Path::new("/some/path/../../etc/passwd");
        let result = validate_install_source(source, &policy);
        assert!(result.is_ok());
    }

    // ---- New tests ----

    #[test]
    fn copy_dir_all_valid_directory_nested() {
        let src = make_temp_dir("nested_src");
        let dst = make_temp_dir("nested_dst");

        // Create nested structure
        fs::create_dir_all(src.join("sub").join("deep")).unwrap();
        fs::write(src.join("top.txt"), "top content").unwrap();
        fs::write(src.join("sub").join("mid.txt"), "mid content").unwrap();
        fs::write(
            src.join("sub").join("deep").join("bottom.txt"),
            "bottom content",
        )
        .unwrap();

        let result = copy_dir_all(&src, &dst);
        assert!(result.is_ok(), "copy_dir_all failed: {result:?}");

        // Verify files exist
        assert!(dst.join("top.txt").exists());
        assert!(dst.join("sub").join("mid.txt").exists());
        assert!(dst.join("sub").join("deep").join("bottom.txt").exists());

        // Verify content
        assert_eq!(
            fs::read_to_string(dst.join("top.txt")).unwrap(),
            "top content"
        );
        assert_eq!(
            fs::read_to_string(dst.join("sub").join("deep").join("bottom.txt")).unwrap(),
            "bottom content"
        );

        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
    }

    #[test]
    fn copy_dir_all_rejects_symlink_at_source() {
        let src = make_temp_dir("symlink_src");
        let dst = make_temp_dir("symlink_dst");

        // Create a regular file and a symlink
        fs::write(src.join("real.txt"), "real").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/passwd", src.join("bad_link")).unwrap();

        let result = copy_dir_all(&src, &dst);
        assert!(result.is_err(), "should have rejected symlink");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("symlink"),
            "error should mention symlink: {err_msg}"
        );

        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
    }

    /// Build a raw tar.gz archive with a single file entry.
    /// Uses raw tar construction to bypass tar crate's path validation,
    /// allowing us to test malicious paths like `../escape.txt` or `/etc/passwd`.
    fn build_raw_tar_gz(entry_path: &str, data: &[u8], entry_type: u8) -> Vec<u8> {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;

        let mut header = [0u8; 512];

        // File name at offset 0, 100 bytes
        let name_bytes = entry_path.as_bytes();
        let name_len = name_bytes.len().min(100);
        header[..name_len].copy_from_slice(&name_bytes[..name_len]);

        // Mode at offset 100, 8 bytes (octal)
        let mode = b"0000644\0";
        header[100..108].copy_from_slice(mode);

        // UID at offset 108, 8 bytes
        let uid = b"0000000\0";
        header[108..116].copy_from_slice(uid);

        // GID at offset 116, 8 bytes
        let gid = b"0000000\0";
        header[116..124].copy_from_slice(gid);

        // Size at offset 124, 12 bytes (octal)
        let size_str = format!("{:011o}", data.len());
        let size_bytes = size_str.as_bytes();
        header[124..124 + size_bytes.len()].copy_from_slice(size_bytes);

        // Mtime at offset 136, 12 bytes
        let mtime = b"00000000000\0";
        header[136..148].copy_from_slice(mtime);

        // Typeflag at offset 156, 1 byte
        header[156] = entry_type;

        // Linkname at offset 157, 100 bytes (zeroed = no link)

        // Magic at offset 257, 6 bytes (ustar + null)
        header[257..263].copy_from_slice(b"ustar\0");

        // Version at offset 263, 2 bytes
        header[263..265].copy_from_slice(b"00");

        // Compute checksum (replace checksum field with spaces first)
        for item in header.iter_mut().take(156).skip(148) {
            *item = b' ';
        }
        let chksum: u32 = header.iter().map(|&b| b as u32).sum();
        let chksum_str = format!("{:06o}\0 ", chksum);
        header[148..156].copy_from_slice(chksum_str.as_bytes());

        // Build tar stream: header + data + padding to 512
        let mut tar_bytes = Vec::new();
        tar_bytes.extend_from_slice(&header);
        tar_bytes.extend_from_slice(data);
        // Pad data to 512-byte boundary
        let padding = (512 - (data.len() % 512)) % 512;
        tar_bytes.extend(std::iter::repeat(0u8).take(padding));

        // Two 512-byte zero blocks mark end
        tar_bytes.extend(std::iter::repeat(0u8).take(1024));

        // Gzip it
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut encoder = encoder;
        encoder.write_all(&tar_bytes).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn extract_plugin_archive_rejects_traversal() {
        let tmp = make_temp_dir("archive_traversal");
        let archive_path = tmp.join("bad.tar.gz");
        let dest = tmp.join("dest");

        let bytes = build_raw_tar_gz("../escape.txt", b"escape content", b'0');
        fs::write(&archive_path, &bytes).unwrap();

        fs::create_dir_all(&dest).unwrap();
        let result = extract_plugin_archive(&archive_path, &dest);
        assert!(result.is_err(), "should have rejected traversal");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("traversal")
                || err_msg.contains("outside")
                || err_msg.contains("not allowed"),
            "error should mention traversal: {err_msg}"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn extract_plugin_archive_rejects_symlink() {
        let tmp = make_temp_dir("archive_symlink");
        let archive_path = tmp.join("symlink.tar.gz");
        let dest = tmp.join("dest");

        // Type '2' = symlink in tar format
        let bytes = build_raw_tar_gz("link_to_passwd", b"", b'2');
        fs::write(&archive_path, &bytes).unwrap();

        fs::create_dir_all(&dest).unwrap();
        let result = extract_plugin_archive(&archive_path, &dest);
        assert!(result.is_err(), "should have rejected symlink");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("symlink"),
            "error should mention symlink: {err_msg}"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn extract_plugin_archive_rejects_absolute_path() {
        let tmp = make_temp_dir("archive_absolute");
        let archive_path = tmp.join("absolute.tar.gz");
        let dest = tmp.join("dest");

        let bytes = build_raw_tar_gz("/etc/passwd", b"absolute content", b'0');
        fs::write(&archive_path, &bytes).unwrap();

        fs::create_dir_all(&dest).unwrap();
        let result = extract_plugin_archive(&archive_path, &dest);
        assert!(result.is_err(), "should have rejected absolute path");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("traversal")
                || err_msg.contains("absolute")
                || err_msg.contains("not allowed"),
            "error should mention the issue: {err_msg}"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn validate_relative_install_path_rejects_parent() {
        let rel = Path::new("../foo");
        let result = validate_relative_install_path(rel);
        assert!(result.is_err(), "should reject ParentDir component");
    }

    #[test]
    fn validate_install_source_rejects_parent_components_lexically() {
        let policy = PluginInstallPolicy::default();
        let source = Path::new("/tmp/../escape");
        let result = validate_install_source(source, &policy);
        // Original path has ParentDir component → PolicyViolation before canonicalize
        assert!(
            matches!(result, Err(InstallError::PolicyViolation(_))),
            "should reject lexically: {result:?}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uninstall_rejects_parent_component_in_name() {
        let result = uninstall("../escape").await;
        assert!(
            matches!(result, Err(InstallError::InvalidPath(_))),
            "should reject ParentDir in name: {result:?}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uninstall_rejects_absolute_path_in_name() {
        let result = uninstall("/etc/passwd").await;
        assert!(
            matches!(result, Err(InstallError::InvalidPath(_))),
            "should reject absolute path in name: {result:?}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn install_creates_nested_valid_plugin() {
        let src = make_temp_dir("nested_valid");
        let dest_root = make_temp_dir("nested_valid_root");
        fs::create_dir_all(src.join("subdir")).unwrap();
        fs::write(src.join("subdir").join("file.txt"), "nested content").unwrap();
        fs::write(
            src.join("manifest.toml"),
            r#"
name = "nested-plugin"
version = "1.0.0"
api_version = 1
"#,
        )
        .unwrap();

        let result = install_from_path_into(&src, &dest_root).await;
        assert!(
            result.is_ok(),
            "install should succeed for nested valid plugin: {result:?}"
        );

        // Verify nested file was copied
        if let Ok(dest) = &result {
            assert!(dest.join("subdir").join("file.txt").exists());
            assert_eq!(
                fs::read_to_string(dest.join("subdir").join("file.txt")).unwrap(),
                "nested content"
            );
            let _ = fs::remove_dir_all(dest);
        }

        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dest_root);
    }

    // ---- validate_local_install_source tests (Workstream B) ----

    fn write_minimal_manifest(dir: &Path) {
        fs::write(
            dir.join("manifest.toml"),
            r#"
name = "local-install-test"
version = "1.0.0"
api_version = 1
"#,
        )
        .unwrap();
    }

    #[test]
    fn validate_local_install_source_accepts_absolute_path_with_manifest() {
        let dir = make_temp_dir("localsrc_absolute");
        write_minimal_manifest(&dir);
        let policy = PluginInstallPolicy::default();
        let result = validate_local_install_source(&dir, &policy);
        assert!(
            result.is_ok(),
            "absolute local source should validate: {result:?}"
        );
        let canonical = result.unwrap();
        assert!(canonical.is_dir());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_local_install_source_accepts_relative_path() {
        let dir = make_temp_dir("localsrc_relative");
        write_minimal_manifest(&dir);
        let policy = PluginInstallPolicy::default();

        // Build a relative path that resolves to our temp dir.
        let cwd = std::env::current_dir().unwrap();
        let relative = match dir.strip_prefix(&cwd) {
            Ok(p) => p.to_path_buf(),
            Err(_) => {
                // If the temp dir is not under cwd, just use absolute.
                dir.clone()
            }
        };

        let result = validate_local_install_source(&relative, &policy);
        assert!(
            result.is_ok(),
            "relative local source should validate: {result:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_local_install_source_accepts_dotdot_when_canonical_resolves() {
        // Create a nested structure where a `..` traversal resolves to a
        // real directory containing a manifest.
        let parent = make_temp_dir("localsrc_dotdot_parent");
        let child = parent.join("child");
        fs::create_dir_all(&child).unwrap();
        write_minimal_manifest(&child);
        let policy = PluginInstallPolicy::default();

        // Build a sibling+dotdot path: from `parent/sibling/..` we can reach `parent`,
        // then descend into `child`. The canonical resolution must succeed.
        let sibling = parent.join("sibling");
        fs::create_dir_all(&sibling).unwrap();
        let via_dotdot = sibling.join("..").join("child");
        let result = validate_local_install_source(&via_dotdot, &policy);
        assert!(
            result.is_ok(),
            "dotdot local source that canonicalizes should validate: {result:?}"
        );
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn validate_local_install_source_rejects_missing_manifest() {
        let dir = make_temp_dir("localsrc_no_manifest");
        // No manifest.toml
        let policy = PluginInstallPolicy::default();
        let result = validate_local_install_source(&dir, &policy);
        assert!(
            matches!(result, Err(InstallError::Manifest(_))),
            "should reject directory without manifest.toml: {result:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_local_install_source_rejects_nonexistent_path() {
        let policy = PluginInstallPolicy::default();
        let result = validate_local_install_source(
            std::path::Path::new("/nonexistent/zzz/local-install-source"),
            &policy,
        );
        assert!(
            matches!(result, Err(InstallError::InvalidPath(_))),
            "should reject nonexistent path: {result:?}"
        );
    }

    #[test]
    fn validate_local_install_source_rejects_file_not_directory() {
        let dir = make_temp_dir("localsrc_file");
        let file = dir.join("not_a_dir.txt");
        fs::write(&file, "i am a file").unwrap();
        let policy = PluginInstallPolicy::default();
        let result = validate_local_install_source(&file, &policy);
        assert!(
            matches!(result, Err(InstallError::InvalidPath(_))),
            "should reject regular file as install source: {result:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archive_traversal_still_rejected_after_local_policy_split() {
        // Regression guard: the strict relative-path validator used for
        // archive entries must still reject traversal even though
        // validate_local_install_source accepts dotdot.
        let policy = PluginInstallPolicy::default();
        assert!(validate_relative_install_path(Path::new("../escape")).is_err());
        assert!(validate_relative_install_path(Path::new("/etc/passwd")).is_err());
        // Sanity: a normal relative path still validates.
        assert!(validate_relative_install_path(Path::new("sub/file.txt")).is_ok());
        // Use the policy parameter to silence the unused-variable lint.
        let _ = policy;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uninstall_rejects_path_separators_after_split() {
        // Regression guard: uninstall name validation remains strict.
        assert!(matches!(
            uninstall("foo/bar").await,
            Err(InstallError::InvalidPath(_))
        ));
        assert!(matches!(
            uninstall("foo\\bar").await,
            Err(InstallError::InvalidPath(_))
        ));
    }
}

use std::path::{Path, PathBuf};

use crate::plugin::manifest::PluginManifest;

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

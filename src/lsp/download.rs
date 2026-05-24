use std::path::Path;

use reqwest::Client;
use tokio::fs;
use tracing::info;

use super::server::{ArchiveType, DownloadSpec, LspServerDef};
use crate::error::LspError;

pub fn cache_dir() -> std::path::PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("codegg")
        .join("lsp")
}

pub async fn ensure_server_binary(server: &LspServerDef) -> Result<std::path::PathBuf, LspError> {
    if let Some(path) = find_in_path(server.command).await {
        info!(server = server.id, path = ?path, "found server binary in PATH");
        return Ok(path);
    }

    let cache = cache_dir().join(server.id);
    let binary = cache.join(server.command);

    if tokio::fs::metadata(&binary).await.is_ok() {
        info!(server = server.id, path = ?binary, "found cached server binary");
        return Ok(binary);
    }

    let spec = server.download.as_ref().ok_or_else(|| {
        LspError::ServerNotFound(format!(
            "server '{}' not found in PATH and no download spec",
            server.id
        ))
    })?;

    info!(server = server.id, "downloading server binary");
    download_server(server, spec, &cache).await
}

async fn find_in_path(cmd: &str) -> Option<std::path::PathBuf> {
    if std::path::Path::new(cmd).is_absolute() {
        let p = std::path::PathBuf::from(cmd);
        if is_executable(&p).await {
            return Some(p);
        }
        return None;
    }

    let path_var = std::env::var("PATH").ok()?;
    let paths = std::env::split_paths(&path_var);
    for dir in paths {
        let candidate = dir.join(cmd);
        if is_executable(&candidate).await {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
async fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::metadata(path)
        .await
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
async fn is_executable(path: &std::path::Path) -> bool {
    tokio::fs::metadata(path)
        .await
        .map(|_| true)
        .unwrap_or(false)
}

async fn download_server(
    server: &LspServerDef,
    spec: &DownloadSpec,
    dest: &Path,
) -> Result<std::path::PathBuf, LspError> {
    fs::create_dir_all(dest).await?;

    let url = resolve_url(spec);
    info!(server = server.id, url = %url, "downloading from URL");

    let client = Client::new();
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| LspError::DownloadFailed(format!("failed to download {}: {}", url, e)))?;

    if !resp.status().is_success() {
        return Err(LspError::DownloadFailed(format!(
            "download failed with status {}",
            resp.status()
        )));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| LspError::DownloadFailed(format!("failed to read response body: {}", e)))?;

    let binary_path = match spec.archive_type {
        ArchiveType::Zip => extract_zip(&bytes, dest, spec.binary_name)?,
        ArchiveType::TarGz => extract_tar_gz(&bytes, dest, spec.binary_name)?,
        ArchiveType::TarXz => extract_tar_xz(&bytes, dest, spec.binary_name)?,
        ArchiveType::Raw => {
            let path = dest.join(spec.binary_name);
            fs::write(&path, &bytes).await?;
            path
        }
    };

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&binary_path).await?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&binary_path, perms).await?;
    }

    info!(server = server.id, path = ?binary_path, "downloaded server binary");
    Ok(binary_path)
}

fn resolve_url(spec: &DownloadSpec) -> String {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;

    let arch = match arch {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        "arm" => "arm",
        "arm64" => "aarch64",
        _ => arch,
    };

    let os = match os {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "win32",
        _ => os,
    };

    spec.url_template
        .replace("{arch}", arch)
        .replace("{os}", os)
}

fn extract_zip(
    data: &[u8],
    dest: &Path,
    binary_name: &str,
) -> Result<std::path::PathBuf, LspError> {
    use std::io::Cursor;
    use zip::ZipArchive;

    let dest = dest
        .canonicalize()
        .map_err(|_| LspError::DownloadFailed("invalid destination".into()))?;

    let mut archive = ZipArchive::new(Cursor::new(data))
        .map_err(|e| LspError::DownloadFailed(format!("failed to open zip: {}", e)))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| LspError::DownloadFailed(format!("failed to read zip entry: {}", e)))?;
        let name = file.name().to_string();

        if name.contains(binary_name) || name.ends_with(binary_name) {
            let file_name = std::path::Path::new(&name)
                .file_name()
                .unwrap_or(std::ffi::OsStr::new(binary_name));
            let out_path = dest.join(file_name);

            if !out_path.canonicalize().map(|p| p.starts_with(&dest)).unwrap_or(false) {
                return Err(LspError::DownloadFailed("path traversal blocked".into()));
            }

            let mut out = std::fs::File::create(&out_path)?;
            std::io::copy(&mut file, &mut out)?;
            return Ok(out_path);
        }
    }

    Err(LspError::DownloadFailed(format!(
        "binary '{}' not found in zip archive",
        binary_name
    )))
}

fn extract_tar_gz(
    data: &[u8],
    dest: &Path,
    binary_name: &str,
) -> Result<std::path::PathBuf, LspError> {
    use flate2::read::GzDecoder;
    use std::io::Cursor;
    use tar::Archive;

    let dest = dest
        .canonicalize()
        .map_err(|_| LspError::DownloadFailed("invalid destination".into()))?;

    let decoder = GzDecoder::new(Cursor::new(data));
    let mut archive = Archive::new(decoder);

    for entry_result in archive.entries()? {
        let mut entry = entry_result?;
        let path = entry.path()?.to_path_buf();
        let full_path = dest.join(&path);

        if !full_path.starts_with(&dest) {
            return Err(LspError::DownloadFailed("path traversal blocked".into()));
        }

        if let Some(name) = path.file_name() {
            if name.to_string_lossy() == binary_name {
                std::fs::create_dir_all(&full_path.parent().unwrap_or(&dest))?;
                entry.unpack(&full_path)?;
                return Ok(full_path);
            }
        }
    }

    Err(LspError::DownloadFailed(format!(
        "binary '{}' not found in tar.gz archive",
        binary_name
    )))
}

fn extract_tar_xz(
    data: &[u8],
    dest: &Path,
    binary_name: &str,
) -> Result<std::path::PathBuf, LspError> {
    use std::io::Cursor;
    let xz_data = xz2::read::XzDecoder::new(Cursor::new(data));
    let mut archive = tar::Archive::new(xz_data);

    let dest = dest
        .canonicalize()
        .map_err(|_| LspError::DownloadFailed("invalid destination".into()))?;

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();
        let full_path = dest.join(&path);

        if !full_path.starts_with(&dest) {
            return Err(LspError::DownloadFailed("path traversal blocked".into()));
        }

        entry.unpack(&full_path)?;
    }

    let bin_path = dest.join(binary_name);
    if bin_path.exists() {
        return Ok(bin_path);
    }
    for entry in walkdir::WalkDir::new(dest).into_iter().flatten() {
        let p = entry.path();
        if p.is_file() && p.file_name().is_some_and(|n| n == binary_name) {
            return Ok(p.to_path_buf());
        }
    }
    Err(LspError::DownloadFailed(format!(
        "binary '{}' not found in tar.xz archive",
        binary_name
    )))
}

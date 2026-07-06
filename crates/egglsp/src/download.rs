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

/// Lexically validate a path extracted from an archive.
///
/// Only `Normal` and `CurDir` components are allowed. `ParentDir`,
/// `RootDir`, and `Prefix` components are rejected as traversal.
fn validate_archive_member_path(rel: &Path) -> Result<(), String> {
    for component in rel.components() {
        match component {
            std::path::Component::Normal(_) | std::path::Component::CurDir => {}
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

        // Reject symlink entries up front: copying their target bytes
        // would silently turn an LSP binary into an arbitrary file.
        if file.is_symlink() {
            return Err(LspError::DownloadFailed(format!(
                "symlinks are not allowed in zip archive: {}",
                file.name()
            )));
        }

        let name = file.name().to_string();

        if name.contains(binary_name) || name.ends_with(binary_name) {
            let file_name = std::path::Path::new(&name)
                .file_name()
                .ok_or_else(|| LspError::DownloadFailed("invalid zip entry name".into()))?;
            let out_path = dest.join(file_name);

            // Lexical containment check on the entry path itself.
            let entry_path = std::path::Path::new(&name);
            validate_archive_member_path(entry_path).map_err(LspError::DownloadFailed)?;

            // Lexical containment check on the resolved output path
            // (the file does not exist yet, so we cannot canonicalize).
            if !out_path.starts_with(&dest) {
                return Err(LspError::DownloadFailed("path traversal blocked".into()));
            }
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
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
        let header = entry.header().clone();
        if header.entry_type().is_symlink() || header.entry_type().is_hard_link() {
            return Err(LspError::DownloadFailed(format!(
                "symlinks and hard links are not allowed in tar.gz archive: {:?}",
                entry.path().ok().map(|p| p.to_path_buf())
            )));
        }
        let path = entry.path()?.to_path_buf();
        validate_archive_member_path(&path).map_err(LspError::DownloadFailed)?;

        let full_path = dest.join(&path);
        if !full_path.starts_with(&dest) {
            return Err(LspError::DownloadFailed("path traversal blocked".into()));
        }

        if let Some(name) = path.file_name() {
            if name.to_string_lossy() == binary_name {
                std::fs::create_dir_all(full_path.parent().unwrap_or(&dest))?;
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
        let header = entry.header().clone();
        if header.entry_type().is_symlink() || header.entry_type().is_hard_link() {
            return Err(LspError::DownloadFailed(format!(
                "symlinks and hard links are not allowed in tar.xz archive: {:?}",
                entry.path().ok().map(|p| p.to_path_buf())
            )));
        }
        let path = entry.path()?.to_path_buf();
        validate_archive_member_path(&path).map_err(LspError::DownloadFailed)?;

        let full_path = dest.join(&path);
        if !full_path.starts_with(&dest) {
            return Err(LspError::DownloadFailed("path traversal blocked".into()));
        }

        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Build a raw tar entry header. `name` may contain malicious
    /// components to test rejection. `entry_type` follows the tar
    /// typeflag byte (b'0' = regular file, b'2' = symlink, b'1' = hardlink).
    fn build_tar_entry(name: &str, data: &[u8], entry_type: u8) -> Vec<u8> {
        let mut header = [0u8; 512];
        let name_bytes = name.as_bytes();
        let name_len = name_bytes.len().min(100);
        header[..name_len].copy_from_slice(&name_bytes[..name_len]);

        let mode = b"0000644\0";
        header[100..108].copy_from_slice(mode);
        let uid = b"0000000\0";
        header[108..116].copy_from_slice(uid);
        let gid = b"0000000\0";
        header[116..124].copy_from_slice(gid);
        let size_str = format!("{:011o}", data.len());
        header[124..124 + size_str.len()].copy_from_slice(size_str.as_bytes());
        header[136..148].copy_from_slice(b"00000000000\0");
        header[156] = entry_type;
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");

        for item in header.iter_mut().take(156).skip(148) {
            *item = b' ';
        }
        let chksum: u32 = header.iter().map(|&b| b as u32).sum();
        let chksum_str = format!("{:06o}\0 ", chksum);
        header[148..156].copy_from_slice(chksum_str.as_bytes());

        let mut out = Vec::new();
        out.extend_from_slice(&header);
        out.extend_from_slice(data);
        let padding = (512 - (data.len() % 512)) % 512;
        out.extend(std::iter::repeat(0u8).take(padding));
        out.extend(std::iter::repeat(0u8).take(1024));
        out
    }

    fn gzip_bytes(tar_bytes: Vec<u8>) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&tar_bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn xz_bytes(tar_bytes: Vec<u8>) -> Vec<u8> {
        let mut encoder = xz2::write::XzEncoder::new(Vec::new(), 6);
        encoder.write_all(&tar_bytes).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn validate_archive_member_path_accepts_relative() {
        assert!(validate_archive_member_path(Path::new("foo/bar.txt")).is_ok());
        assert!(validate_archive_member_path(Path::new("./foo")).is_ok());
    }

    #[test]
    fn validate_archive_member_path_rejects_parent() {
        assert!(validate_archive_member_path(Path::new("../foo")).is_err());
        assert!(validate_archive_member_path(Path::new("a/../../b")).is_err());
    }

    #[test]
    fn validate_archive_member_path_rejects_absolute() {
        assert!(validate_archive_member_path(Path::new("/etc/passwd")).is_err());
    }

    #[test]
    fn tar_gz_extracts_valid_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();

        let tar_bytes = build_tar_entry("mybin", b"binary contents", b'0');
        let gz = gzip_bytes(tar_bytes);

        let result = extract_tar_gz(&gz, &dest, "mybin").expect("should extract");
        assert!(result.exists());
        assert_eq!(std::fs::read(&result).unwrap(), b"binary contents");
    }

    #[test]
    fn tar_gz_rejects_parent_traversal_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();

        let tar_bytes = build_tar_entry("../escape", b"bad", b'0');
        let gz = gzip_bytes(tar_bytes);

        let err = extract_tar_gz(&gz, &dest, "escape").unwrap_err();
        assert!(matches!(err, LspError::DownloadFailed(_)));
    }

    #[test]
    fn tar_gz_rejects_absolute_path_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();

        let tar_bytes = build_tar_entry("/etc/passwd", b"bad", b'0');
        let gz = gzip_bytes(tar_bytes);

        let err = extract_tar_gz(&gz, &dest, "passwd").unwrap_err();
        assert!(matches!(err, LspError::DownloadFailed(_)));
    }

    #[test]
    fn tar_gz_rejects_symlink_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();

        let tar_bytes = build_tar_entry("link_to_passwd", b"/etc/passwd", b'2');
        let gz = gzip_bytes(tar_bytes);

        let err = extract_tar_gz(&gz, &dest, "link_to_passwd").unwrap_err();
        assert!(matches!(err, LspError::DownloadFailed(_)));
    }

    #[test]
    fn tar_gz_rejects_hardlink_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();

        let tar_bytes = build_tar_entry("hardlink", b"other", b'1');
        let gz = gzip_bytes(tar_bytes);

        let err = extract_tar_gz(&gz, &dest, "hardlink").unwrap_err();
        assert!(matches!(err, LspError::DownloadFailed(_)));
    }

    #[test]
    fn tar_xz_rejects_parent_traversal_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();

        let tar_bytes = build_tar_entry("../escape", b"bad", b'0');
        let xz = xz_bytes(tar_bytes);

        let err = extract_tar_xz(&xz, &dest, "escape").unwrap_err();
        assert!(matches!(err, LspError::DownloadFailed(_)));
    }

    #[test]
    fn tar_xz_rejects_symlink_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();

        let tar_bytes = build_tar_entry("link_to_passwd", b"/etc/passwd", b'2');
        let xz = xz_bytes(tar_bytes);

        let err = extract_tar_xz(&xz, &dest, "link_to_passwd").unwrap_err();
        assert!(matches!(err, LspError::DownloadFailed(_)));
    }

    #[test]
    fn zip_extracts_valid_entry() {
        use std::io::Cursor;
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;

        let buf: Vec<u8> = Vec::new();
        let mut writer = ZipWriter::new(Cursor::new(buf));
        let opts = SimpleFileOptions::default();
        writer.start_file("mybin", opts).unwrap();
        use std::io::Write as _;
        writer.write_all(b"binary contents").unwrap();
        let inner = writer.finish().unwrap();
        let zip_bytes = inner.into_inner();

        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();

        let result = extract_zip(&zip_bytes, &dest, "mybin").expect("should extract");
        assert!(result.exists());
        assert_eq!(std::fs::read(&result).unwrap(), b"binary contents");
    }

    #[test]
    fn zip_rejects_symlink_entry() {
        use std::io::Cursor;
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;

        let buf: Vec<u8> = Vec::new();
        let mut writer = ZipWriter::new(Cursor::new(buf));
        let opts = SimpleFileOptions::default();
        writer.add_symlink("linkname", "/etc/passwd", opts).unwrap();
        let inner = writer.finish().unwrap();
        let zip_bytes = inner.into_inner();

        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();

        let err = extract_zip(&zip_bytes, &dest, "linkname").unwrap_err();
        assert!(matches!(err, LspError::DownloadFailed(_)));
    }

    #[test]
    fn zip_rejects_traversal_entry_name() {
        use std::io::Cursor;
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;

        let buf: Vec<u8> = Vec::new();
        let mut writer = ZipWriter::new(Cursor::new(buf));
        let opts = SimpleFileOptions::default();
        writer.start_file("good/../mybin", opts).unwrap();
        use std::io::Write as _;
        writer.write_all(b"binary contents").unwrap();
        let inner = writer.finish().unwrap();
        let zip_bytes = inner.into_inner();

        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();

        let err = extract_zip(&zip_bytes, &dest, "mybin").unwrap_err();
        assert!(matches!(err, LspError::DownloadFailed(_)));
    }
}

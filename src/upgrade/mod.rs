//! Release selection and verified managed-runfile self-update.
//!
//! M005 (dependency-security workspace consolidation) hardening:
//!
//! - CodeGG owns release/version/target/archive policy and uses Eggup for
//!   bounded acquisition contracts and verified local multi-runfile
//!   transaction mechanics.
//! - Normal self-update downloads only declared release assets, verifies the
//!   archive checksum before strict extraction, and never fetches or executes
//!   the bootstrap installer.
//! - Fresh installation via `install.sh` remains supported as a manual
//!   operator action; [`installer_invocation`] pins the only version-pin
//!   name the installer honors (`CODEGG_VERSION`).
//! - Existing Eggfetch trust, Rustls, redirect, and timeout policy remains
//!   CodeGG-owned. Unsupported hosts retain pinned manual bootstrap guidance.

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use eggfetch_core::Timeout;

mod managed;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Installer script URL referenced by fresh-install guidance.
///
/// This URL is never fetched or executed by CodeGG itself. It is printed
/// only as manual fresh-install guidance for unsupported in-place targets.
/// See [`installer_invocation`].
pub const INSTALLER_SCRIPT_URL: &str =
    "https://raw.githubusercontent.com/dbowm91/codegg/main/install.sh";

/// Version-pin environment variable honored by the installer.
pub const INSTALLER_VERSION_ENV: &str = "CODEGG_VERSION";

/// Version-pin environment variable honored by the installer.
///
/// The installer (`install.sh`) reads `CODEGG_VERSION` (`0.1.1` or `v0.1.1`,
/// normalized to tag `v0.1.1`). Exporting any other name silently installs
/// latest instead of the checked version.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub current: String,
    pub latest: Option<String>,
    pub needs_update: bool,
}

pub fn current_version() -> String {
    VERSION.to_string()
}

/// Pure constructor for the installer invocation, so the version-pin
/// contract is unit-testable without spawning `curl`. Returns the script
/// URL plus the environment entries the installer must observe.
pub fn installer_invocation(target: &str) -> (&'static str, Vec<(&'static str, String)>) {
    (
        INSTALLER_SCRIPT_URL,
        vec![(INSTALLER_VERSION_ENV, target.to_string())],
    )
}

pub async fn check_for_updates() -> Result<VersionInfo, AppError> {
    let client = crate::http_client::ordinary_http_client_builder(Timeout::from_secs(10)).build();

    let mut resp = client
        .get("https://api.github.com/repos/dbowm91/codegg/releases/latest")
        .map_err(|e| AppError::Upgrade(format!("request build failed: {e}")))?
        .header("User-Agent", "codegg")
        .send()
        .await
        .map_err(|e| AppError::Upgrade(format!("request failed: {e}")))?;

    if !resp.status().is_success() {
        return Err(AppError::Upgrade(format!(
            "GitHub API returned {}",
            resp.status()
        )));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Upgrade(format!("failed to parse response: {e}")))?;

    let latest = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim_start_matches('v').to_string());

    let needs_update = latest.as_ref().map(|l| l != VERSION).unwrap_or(false);

    Ok(VersionInfo {
        current: VERSION.to_string(),
        latest,
        needs_update,
    })
}

pub async fn upgrade() -> Result<String, AppError> {
    let info = check_for_updates().await?;
    if !info.needs_update {
        return Ok(format!("Already on latest version ({})", info.current));
    }
    let latest = info
        .latest
        .ok_or_else(|| AppError::Upgrade("no latest version found".to_string()))?;
    let latest_for_worker = latest.clone();
    tokio::task::spawn_blocking(move || managed::update(&latest_for_worker))
        .await
        .map_err(|_| AppError::Upgrade("upgrade worker failed".to_string()))?
        .map_err(AppError::Upgrade)
}

/// Pure manual-install guidance for unsupported in-place targets.
///
/// Supported Linux/macOS targets use [`upgrade`] and the managed native path.
/// This function remains deterministic and network-free for CLI/reporting
/// callers that need fresh-install guidance:
///
/// Already-current yields `Ok`; missing or invalid release data fails closed;
/// a valid newer release yields version-pinned bootstrap guidance.
pub fn describe_manual_fresh_install(info: &VersionInfo) -> Result<String, AppError> {
    if !info.needs_update {
        return Ok(format!("Already on latest version ({})", info.current));
    }

    let latest = info
        .latest
        .as_ref()
        .ok_or_else(|| AppError::Upgrade("no latest version found".to_string()))?;

    semver::Version::parse(latest)
        .map_err(|_| AppError::Upgrade(format!("invalid semver version: {latest}")))?;

    let target = format!("v{latest}");
    let (script_url, version_env) = installer_invocation(&target);
    let pin = version_env
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(" ");
    Err(AppError::Upgrade(format!(
        "native in-place update is unavailable on this target. \
New version available: {latest} (current: {}). \
For a manual fresh installation only, run: {pin} curl -fsSL {script_url} | sh",
        info.current
    )))
}

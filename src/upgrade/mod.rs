//! Self-update version check with a retired in-place execution path.
//!
//! M005 (dependency-security workspace consolidation) hardening:
//!
//! - The normal `codegg upgrade` path is check-only. It queries GitHub
//!   release metadata through the existing Eggfetch transport with an
//!   explicit timeout and bounded redirect policy.
//! - CodeGG does not download and execute a network-fetched shell
//!   installer script, does not shell out to external `curl`, acquires no
//!   candidate binary bytes, and attempts no executable replacement.
//! - Fresh installation via `install.sh` remains supported as a manual
//!   operator action; [`installer_invocation`] pins the only version-pin
//!   name the installer honors (`CODEGG_VERSION`).
//! - Automatic verified binary replacement remains blocked on a
//!   generalized external updater interface that is not Gregg/greggd
//!   specific (see M005 plan and closure record). CodeGG intentionally
//!   does not duplicate those mechanics locally.

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use eggfetch_core::Timeout;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Installer script URL referenced by fresh-install guidance.
///
/// This URL is never fetched or executed by CodeGG itself. It is printed
/// by the check-only CLI path so an operator can perform a manual
/// fresh installation. See [`installer_invocation`].
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
    describe_upgrade(&info)
}

/// Pure fail-closed disposition for a checked [`VersionInfo`].
///
/// This is the entire in-place update decision surface after M005
/// hardening:
///
/// - already-current yields `Ok` without touching the executable;
/// - a missing latest tag or an invalid semver tag fails closed;
/// - a valid newer tag fails closed with manual fresh-install guidance.
///
/// No candidate bytes are acquired, no checksum is required (there is
/// nothing to verify because nothing is downloaded), and no executable
/// replacement is attempted, so checksum mismatch, wrong program or
/// version identity, unwritable destination, interrupted download,
/// replacement failure, and unsupported-target cases all reduce to the
/// same property: the existing executable is left intact. Verified
/// binary replacement awaits the blocked external generic updater
/// interface and is intentionally not reimplemented here.
pub fn describe_upgrade(info: &VersionInfo) -> Result<String, AppError> {
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
        "automatic in-place update is disabled; existing executable left intact. \
New version available: {latest} (current: {}). \
For a manual fresh installation only, run: {pin} curl -fsSL {script_url} | sh",
        info.current
    )))
}

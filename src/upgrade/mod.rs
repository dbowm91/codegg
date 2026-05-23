use serde::{Deserialize, Serialize};

use crate::error::AppError;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub current: String,
    pub latest: Option<String>,
    pub needs_update: bool,
}

pub fn current_version() -> String {
    VERSION.to_string()
}

pub async fn check_for_updates() -> Result<VersionInfo, AppError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::Upgrade(e.to_string()))?;

    let resp = client
        .get("https://api.github.com/repos/anomalyco/codegg/releases/latest")
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

    semver::Version::parse(&latest)
        .map_err(|_| AppError::Upgrade(format!("invalid semver version: {}", latest)))?;

    let target = format!("v{latest}");

    let output = std::process::Command::new("curl")
        .args(["-fsSL", "https://codegg.ai/install.sh"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("INSTALL_VERSION", &target)
        .output()
        .map_err(|e| AppError::Upgrade(format!("failed to run installer: {e}")))?;

    if !output.status.success() {
        return Err(AppError::Upgrade(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(format!("Upgraded to {latest}"))
}

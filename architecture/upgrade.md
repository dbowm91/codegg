# Upgrade Module

The `upgrade` module provides self-upgrade functionality via GitHub releases.

## Overview

**Location**: `src/upgrade/`

**Key Responsibilities**:
- Check for updates via GitHub API (queries `https://api.github.com/repos/anomalyco/codegg/releases/latest`)
- Run installer script via `curl -fsSL https://codegg.ai/install.sh`

## Key Types

### VersionInfo

```rust
pub struct VersionInfo {
    pub current: String,
    pub latest: Option<String>,
    pub needs_update: bool,
}
```

## Key Functions

### current_version()

```rust
pub fn current_version() -> String {
    VERSION.to_string()
}
```

### check_for_updates()

```rust
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
        .map_err(|e| AppError::Upgrade(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(AppError::Upgrade(format!(
            "GitHub API returned {}",
            resp.status()
        )));
    }

    let json: serde_json::Value = resp.json().await?;

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
```

### upgrade()

```rust
pub async fn upgrade() -> Result<String, AppError> {
    let info = check_for_updates().await?;
    if !info.needs_update {
        return Ok(format!("Already on latest version ({})", info.current));
    }

    let latest = info.latest.ok_or_else(|| AppError::Upgrade("no latest version found".to_string()))?;

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
```

## Usage

The upgrade command is available via CLI:

```bash
codegg upgrade
```

This checks for updates and, if a new version is available, prints the current and latest versions along with instructions to install.

## See Also

- [config.md](config.md) - Configuration (upgrade settings not yet implemented)

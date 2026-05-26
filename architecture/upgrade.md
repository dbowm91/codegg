# Upgrade Module

The `upgrade` module provides self-upgrade functionality via GitHub releases.

## Overview

**Location**: `src/upgrade/`

**Key Responsibilities**:
- Check for updates via GitHub API (queries `https://api.github.com/repos/anomalyco/codegg/releases/latest`)
- Run installer script via `curl -fsSL https://codegg.ai/install.sh`

## CLI Command Behavior

The `codegg upgrade` command **only checks and reports** - it does not automatically perform the upgrade:

1. Queries GitHub API for latest release tag
2. Compares with current version (`VERSION` from `CARGO_PKG_VERSION`)
3. If newer version available, prints instructions to manually install via:
   ```bash
   curl -fsSL https://codegg.ai/install.sh
   ```

The actual `upgrade()` function exists in `src/upgrade/mod.rs:57` but is **not called** by the CLI command.

## Configuration

### autoupdate

The `autoupdate` config setting exists but is **not currently wired to the upgrade module**:

```rust
pub enum AutoupdateConfig {
    Bool(bool),        // true/false for automatic updates
    Notify(String),    // notify with custom message
}
```

Default: `true` (but not implemented)

This config field is loaded and stored in `Config.autoupdate` but the upgrade module does not read it. The upgrade module only performs version checking when the CLI command is run.

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
```

### upgrade() (defined but not called)

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

**Note**: This function is defined but not invoked by the CLI. The CLI only reports version info without performing the actual upgrade.

## See Also

- [config.md](config.md) - Configuration (`autoupdate` field defined but not wired to upgrade module)

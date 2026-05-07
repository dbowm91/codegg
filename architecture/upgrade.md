# Upgrade Module

The `upgrade` module provides self-upgrade functionality via GitHub releases.

## Overview

**Location**: `src/upgrade/`

**Key Responsibilities**:
- Check for updates via GitHub API
- Download release binaries
- Run installer

## Key Functions

### check_for_updates()

```rust
pub async fn check_for_updates() -> Result<Option<ReleaseInfo>> {
    let current = env!("CARGO_PKG_VERSION");
    let latest = github_api::get_latest_release("anomalyco", "opencode").await?;

    if latest.version > current {
        Ok(Some(latest))
    } else {
        Ok(None)
    }
}

pub struct ReleaseInfo {
    pub version: String,
    pub tag_name: String,
    pub download_url: String,
    pub release_notes: String,
}
```

### upgrade()

```rust
pub async fn upgrade(release: &ReleaseInfo) -> Result<()> {
    // 1. Download binary to temp location
    let download_path = download(&release.download_url).await?;

    // 2. Verify signature
    verify_signature(&download_path)?;

    // 3. Run installer
    run_installer(&download_path)?;

    Ok(())
}
```

## Configuration

```toml
[upgrade]
enabled = true
channel = "stable"  # or "beta"
auto_check = true
```

## See Also

- [config.md](config.md) - Upgrade configuration

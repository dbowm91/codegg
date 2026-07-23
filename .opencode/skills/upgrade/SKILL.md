---
name: upgrade
description: Self-upgrade functionality via GitHub releases
version: 1.1.0
tags: [upgrade, releases, versioning]
---

Use the `/skill:upgrade` command to load context about the upgrade system.

## Overview

The upgrade module provides self-upgrade functionality by querying GitHub releases and running an installer script.

## Usage

```bash
codegg upgrade
```

This checks for updates and, if a newer version is available, prints the current and latest versions along with installation instructions.

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

Returns the current compiled version using `env!("CARGO_PKG_VERSION")`.

```rust
pub fn current_version() -> String
```

### check_for_updates()

Queries `https://api.github.com/repos/anomalyco/codegg/releases/latest` to get the latest release tag.

```rust
pub async fn check_for_updates() -> Result<VersionInfo, AppError>
```

The function:
1. Builds a `reqwest::Client` with 10-second timeout
2. Sends GET request to GitHub API with `User-Agent: codegg` header
3. Parses JSON response for `tag_name` field
4. Compares latest version with `VERSION` constant (`CARGO_PKG_VERSION`)
5. Returns `VersionInfo` with `needs_update: true` if versions differ

### upgrade()

Performs the actual upgrade by running the installer script:

```rust
pub async fn upgrade() -> Result<String, AppError>
```

The function:
1. Checks for updates via `check_for_updates()`
2. Returns "Already on latest version" if `needs_update` is false
3. Validates the latest version is valid semver
4. Runs `curl -fsSL https://codegg.ai/install.sh` with `INSTALL_VERSION` env var set to `v{version}`
5. Returns error if installer fails

**Note**: This function is currently **not called** by `cmd_upgrade()` in `main.rs`. The CLI command only checks and reports, but does not actually perform the upgrade.

## Module Implementation

Location: `src/upgrade/mod.rs`

### PATH Handling

The `upgrade()` function uses the user's actual PATH:

```rust
.env("PATH", std::env::var_os("PATH").unwrap_or_default())
```

### Error Handling

Upgrade errors are wrapped with `AppError::Upgrade(String)` and are classified as `UPGRADE_ERROR` in exec mode.

Error variants include:
- `AppError::Upgrade("request failed: ...")` - Network request failed
- `AppError::Upgrade("GitHub API returned ...")` - Non-success HTTP status
- `AppError::Upgrade("failed to parse response: ...")` - JSON parsing failed
- `AppError::Upgrade("no latest version found")` - No `tag_name` in response
- `AppError::Upgrade("invalid semver version: ...")` - Latest version not valid semver
- `AppError::Upgrade("failed to run installer: ...")` - Could not spawn curl
- `AppError::Upgrade("...")` - Installer returned non-zero exit

## Security Considerations

1. **HTTPS only**: GitHub API and installer script use HTTPS
2. **PATH preservation**: User's PATH is preserved through upgrade process
3. **env_clear()**: Other environment variables are cleared for security

## Testing

Tests in `tests/upgrade.rs`:
- `test_current_version()` - Verifies version is non-empty and contains '.'
- `test_version_info_current_only()` - VersionInfo with no latest
- `test_version_info_needs_update()` - VersionInfo with newer version
- `test_version_info_up_to_date()` - VersionInfo with matching versions

Note: `check_for_updates()` is not integration-tested (requires network).

## See Also

- [architecture/upgrade.md](../../architecture/upgrade.md) - Architecture documentation
- [AGENTS.md](../../AGENTS.md) - Project-wide patterns

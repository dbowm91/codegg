---
name: upgrade
description: Self-upgrade functionality via GitHub releases
version: 1.2.0
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

Queries `https://api.github.com/repos/dbowm91/codegg/releases/latest` to get the latest release tag.

```rust
pub async fn check_for_updates() -> Result<VersionInfo, AppError>
```

The function:
1. Builds an `eggfetch_core::Client` with an explicit 10-second timeout and
   bounded redirect following
2. Sends GET request to GitHub API with `User-Agent: codegg` header
3. Parses JSON response for `tag_name` field
4. Compares latest version with `VERSION` constant (`CARGO_PKG_VERSION`)
5. Returns `VersionInfo` with `needs_update: true` if versions differ

### upgrade()

Fail-closed check plus manual fresh-install guidance (M005 hardening).
Automatic in-place binary replacement is retired:

```rust
pub async fn upgrade() -> Result<String, AppError>
```

The function:
1. Checks for updates via `check_for_updates()`
2. Delegates to the pure `describe_upgrade()` disposition
3. Returns "Already on latest version" if `needs_update` is false
4. Validates the latest version is valid semver (fail-closed on invalid)
5. For a valid newer version, returns `Err` with manual fresh-install
   guidance (`CODEGG_VERSION=v{version}` plus the `install.sh` URL) and
   leaves the existing executable intact

It never spawns `curl`, never fetches or executes a shell script, and
never replaces the running binary. Verified binary replacement awaits
the blocked M005 external generic updater interface.

### describe_upgrade()

Pure fail-closed disposition, deterministic without network:

```rust
pub fn describe_upgrade(info: &VersionInfo) -> Result<String, AppError>
```

Covers already-current, missing-tag, invalid-semver, and valid-newer
(fail-closed with manual guidance) cases. Because no candidate bytes
are acquired, checksum / identity / permission / download / replacement
/ unsupported-target failures all reduce to "existing executable left
intact" by construction.

### installer_invocation()

Pure constructor for the installer script URL plus the version-pin env
entries, so the pin contract is unit-testable without spawning `curl`:

```rust
pub const INSTALLER_SCRIPT_URL: &str = "https://raw.githubusercontent.com/dbowm91/codegg/main/install.sh";
pub const INSTALLER_VERSION_ENV: &str = "CODEGG_VERSION";

pub fn installer_invocation(target: &str) -> (&'static str, Vec<(&'static str, String)>)
```

Regression history: an earlier revision exported `INSTALL_VERSION`,
which `install.sh` ignores — the pin was silently dropped and latest was
installed instead. `tests/upgrade.rs::test_installer_invocation_pins_supported_env`
pins the supported name; do not rename the env var without updating
`install.sh` first.

## Module Implementation

Location: `src/upgrade/mod.rs`

No subprocess is spawned from this module. There is no `PATH` handling,
no `env_clear()`, and no `std::process::Command` in the update path.

### Error Handling

Upgrade errors are wrapped with `AppError::Upgrade(String)` and are classified as `UPGRADE_ERROR` in exec mode.

Error variants include:
- `AppError::Upgrade("request failed: ...")` - Network request failed
- `AppError::Upgrade("GitHub API returned ...")` - Non-success HTTP status
- `AppError::Upgrade("failed to parse response: ...")` - JSON parsing failed
- `AppError::Upgrade("no latest version found")` - No `tag_name` in response
- `AppError::Upgrade("invalid semver version: ...")` - Latest version not valid semver
- `AppError::Upgrade("automatic in-place update is disabled; ...")` - Valid newer version; manual fresh-install guidance, existing executable left intact

## Security Considerations

1. **HTTPS only**: GitHub API metadata uses HTTPS via Eggfetch
2. **No shell execution**: CodeGG never downloads and executes a network-fetched shell script
3. **No external curl**: the update path uses Eggfetch only; `curl` appears solely inside the printed manual fresh-install guidance for operators
4. **Fail-closed**: missing/invalid/newer tags never replace the running binary

## Testing

Tests in `tests/upgrade.rs`:
- `test_current_version()` - Verifies version is non-empty and contains '.'
- `test_version_info_current_only()` - VersionInfo with no latest
- `test_version_info_needs_update()` - VersionInfo with newer version
- `test_version_info_up_to_date()` - VersionInfo with matching versions
- `test_installer_invocation_pins_supported_env()` - Pins `CODEGG_VERSION` as the only supported pin name (see `installer_invocation()` regression history above)
- `test_describe_upgrade_already_current_is_noop()` - Already-current yields Ok without touching the executable
- `test_describe_upgrade_current_only_is_noop()` - No-latest without need yields Ok
- `test_describe_upgrade_valid_newer_fails_closed_with_manual_guidance()` - Valid newer yields Err with pin/URL/intact messaging
- `test_describe_upgrade_missing_latest_fails_closed()` - Missing tag fails closed
- `test_describe_upgrade_invalid_semver_fails_closed()` - Invalid semver fails closed
- `test_describe_upgrade_never_reports_automatic_success_for_newer()` - Guards against reintroducing an automatic "Upgraded to" path

Note: `check_for_updates()` is not integration-tested (requires network).

## See Also

- [architecture/upgrade.md](../../architecture/upgrade.md) - Architecture documentation
- [AGENTS.md](../../AGENTS.md) - Project-wide patterns

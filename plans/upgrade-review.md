# Upgrade Module Review

## Verified Claims

### VersionInfo struct
- **Matches**: The struct at `src/upgrade/mod.rs:7-12` has exactly the fields documented: `current: String`, `latest: Option<String>`, `needs_update: bool`.

### current_version()
- **Matches**: `src/upgrade/mod.rs:14-16` returns `VERSION.to_string()` where `VERSION` is `env!("CARGO_PKG_VERSION")`.

### check_for_updates()
- **Matches**: Implementation at `src/upgrade/mod.rs:18-55` fully matches the documented code block, including:
  - 10-second timeout
  - GitHub API URL `https://api.github.com/repos/anomalyco/codegg/releases/latest`
  - User-Agent header
  - Error handling with `AppError::Upgrade`
  - JSON parsing from `tag_name`
  - `needs_update` logic comparing against `VERSION`

### upgrade()
- **Matches**: Implementation at `src/upgrade/mod.rs:57-87` fully matches the documented code block, including:
  - Calls `check_for_updates()` first
  - Early return if already on latest version
  - Semver validation of latest version
  - Uses `curl -fsSL https://codegg.ai/install.sh` with `INSTALL_VERSION` env var
  - `env_clear()` + PATH preservation
  - Returns error if curl fails

### Architecture doc structure
- **Matches**: File is well-organized with Overview, Key Types, Key Functions, and Usage sections.

---

## Bugs/Discrepancies Found

### 1. Architecture doc line 11 claims `check_for_updates()` "queries" the GitHub API - accurate.

### 2. Documentation note at line 78 is correct but understated
The note says `upgrade()` is **not currently called** by the CLI. In reality:
- `cmd_upgrade()` in `main.rs:551-570` only calls `upgrade::check_for_updates()`
- It prints manual install instructions but **never calls `upgrade::upgrade()`**
- The `upgrade()` function exists and is fully implemented but is **dead code**

**Severity**: Medium - The actual upgrade functionality cannot be triggered via CLI.

### 3. `upgrade()` uses `curl` to run install script, but `cmd_upgrade()` tells users to use `cargo install`
- `upgrade()` function at line 73 uses `curl -fsSL https://codegg.ai/install.sh`
- `cmd_upgrade()` at line 567 tells users to run `cargo install --git https://github.com/anomalyco/codegg --path codegg`

**This is a discrepancy**: The `upgrade()` function would install via a shell script, but users following CLI instructions would use cargo. The `upgrade()` function is unreachable anyway (see bug #2).

### 4. Documentation at line 11 says "Run installer script via `curl -fsSL https://codegg.ai/install.sh`"
This is accurate for the `upgrade()` function, but the function is unreachable.

---

## Improvement Suggestions

### Priority: Medium

**Wire up `upgrade()` function to CLI**
- Location: `src/main.rs:551-570`
- Currently `cmd_upgrade()` only prints instructions. It should call `upgrade::upgrade()` to actually perform the upgrade.
- This requires deciding: should upgrade be interactive (prompt user before downloading) or automatic?

### Priority: Medium

**Resolve conflicting upgrade mechanisms**
- `upgrade()` uses a shell installer script (`curl .../install.sh`)
- `cmd_upgrade()` suggests `cargo install`
- Decide on a single approach and document consistently.

### Priority: Low

**Add upgrade configuration to config module**
- The See Also section references config settings that "not yet implemented"
- If upgrade configuration is desired (e.g., auto-check, channel: stable/beta), it should be tracked as a feature request.

### Priority: Low

**Error message improvement in `upgrade()`**
- When `curl` fails, only stderr is shown. Consider including stdout for better debugging.

### Priority: Low

**Consider adding `upgrade check` subcommand**
- Currently `codegg upgrade` always checks and reports. A separate `codegg upgrade --check` (or just `codegg --version --check`) could provide faster UX.
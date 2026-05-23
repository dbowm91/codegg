# Upgrade Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| `VersionInfo` struct with `current`, `latest`, `needs_update` fields | VERIFIED | `src/upgrade/mod.rs:7-12` matches exactly |
| `current_version()` returns `VERSION.to_string()` | VERIFIED | `src/upgrade/mod.rs:14-16` |
| `check_for_updates()` uses 10s timeout | VERIFIED | `src/upgrade/mod.rs:20` |
| `check_for_updates()` queries `https://api.github.com/repos/anomalyco/codegg/releases/latest` | VERIFIED | `src/upgrade/mod.rs:25` |
| `check_for_updates()` uses `User-Agent: codegg` header | VERIFIED | `src/upgrade/mod.rs:26-27` |
| `check_for_updates()` parses `tag_name`, strips 'v' prefix | VERIFIED | `src/upgrade/mod.rs:43-46` |
| `check_for_updates()` returns `VersionInfo` | VERIFIED | `src/upgrade/mod.rs:50-54` |
| `upgrade()` function defined but not called by CLI | VERIFIED | `src/main.rs:554` calls only `check_for_updates()` |
| `upgrade()` uses `curl -fsSL https://codegg.ai/install.sh` | VERIFIED | `src/upgrade/mod.rs:72-73` |
| `upgrade()` uses `env_clear()` with PATH preservation | VERIFIED | `src/upgrade/mod.rs:74-75` |
| `upgrade()` sets `INSTALL_VERSION` env var | VERIFIED | `src/upgrade/mod.rs:76` |
| `upgrade()` checks `output.status.success()` | VERIFIED | `src/upgrade/mod.rs:80` |
| Architecture doc notes `upgrade()` not called by CLI | VERIFIED | Line 78-79 in architecture doc matches code |

## Bugs Found

### Critical
- **None identified** - Core functionality is correctly implemented and verified.

### High
- **Missing stderr handling on upgrade success path**: The `upgrade()` function only reports stderr on failure (line 80-83). If curl fails but returns success status with error output in stderr, the error goes unnoticed. However, this is low risk since curl's exit code properly indicates failure.

### Medium
- **No GitHub API rate limit handling**: If GitHub returns 403 (rate limit exceeded) or 429 (too many requests), the error message will be unhelpful (`"GitHub API returned 403"`). Should distinguish rate limit errors.
- **No network retry logic**: `check_for_updates()` makes a single request with no retries. Transient network failures will immediately report as errors.

### Low
- **Hardcoded `curl` dependency**: The upgrade function assumes `curl` is available in PATH. On systems without curl, the error message from `Command::new("curl")` would be unclear.

## Improvement Suggestions

### Performance
- **Single request with no caching**: Every `check_for_updates()` call hits the GitHub API. Consider caching the result for a short duration (e.g., 5 minutes) to avoid rate limiting.

### Correctness
- **Rate limit error differentiation**: Return a more descriptive error when GitHub returns 403/429, indicating the issue is temporary.
- **Stdout capture on upgrade failure**: Currently only stderr is reported; stdout might contain useful diagnostic info.
- **Version comparison edge case**: If `VERSION` (from Cargo.toml) contains a pre-release suffix like `1.0.0-beta`, the semver comparison `l != VERSION` works correctly, but the `semver::Version::parse` in `upgrade()` would fail on pre-release versions since stable semver parsers reject them.

### Maintainability
- **Upgrade configuration not implemented**: Architecture doc mentions `[upgrade]` config section but no such configuration exists in the config module. Could add `experimental.auto_upgrade` or `upgrade.check_interval` settings.
- **Install script URL hardcoded**: The URL `https://codegg.ai/install.sh` and install command in `cmd_upgrade()` (`cargo install --git...`) are hardcoded in two places. Consider centralizing.
- **Missing upgrade module skill**: Unlike other modules, no `.opencode/skills/upgrade/SKILL.md` exists for developer guidance.
- **No telemetry/instrumentation**: Upgrade checks and upgrades are not logged in a structured way for debugging user issues.

## Priority Actions (top 5 items to fix)

1. **Add GitHub API rate limit handling** - Distinguish 403/429 errors with helpful messages
2. **Add retry logic with exponential backoff** - For transient network failures in `check_for_updates()`
3. **Capture stdout on upgrade failure** - Currently only stderr is reported; stdout may have useful info
4. **Pre-release version handling** - Document or handle pre-release VERSION strings (e.g., `1.0.0-beta`)
5. **Add upgrade module skill** - Create `.opencode/skills/upgrade/SKILL.md` for developer guidance
# Upgrade Architecture Review

## Architecture Document
- Path: architecture/upgrade.md

## Source Code Location
- src/upgrade/

## Verification Summary
**Pass** - Architecture document is accurate and matches implementation with one exception: the upgrade() function IS defined but is not called by the CLI (documented correctly on line 78).

## Verified Claims (table format)

| Claim | Status | Notes |
|-------|--------|-------|
| VersionInfo struct with current, latest, needs_update fields | Pass | Exact match at src/upgrade/mod.rs:7-12 |
| current_version() returns VERSION.to_string() | Pass | Exact match at mod.rs:14-16 |
| check_for_updates() uses reqwest with 10s timeout | Pass | Exact match at mod.rs:19-21 |
| GitHub API URL: https://api.github.com/repos/anomalyco/codegg/releases/latest | Pass | Exact match at mod.rs:25 |
| User-Agent: "codegg" header | Pass | Exact match at mod.rs:26 |
| Error format: "request failed: {e}" | Pass | Exact match at mod.rs:29 |
| JSON parsing from "tag_name" field, strips 'v' prefix | Pass | Exact match at mod.rs:43-46 |
| needs_update = latest != VERSION | Pass | Exact match at mod.rs:48 |
| upgrade() function defined but NOT called by CLI | Pass | Documented on line 78; confirmed in main.rs:551-569 that only check_for_updates() is used |
| upgrade() uses curl -fsSL https://codegg.ai/install.sh | Pass | Exact match at mod.rs:72-73 |
| upgrade() env_clear() then sets PATH | Pass | Exact match at mod.rs:74-75 |
| upgrade() sets INSTALL_VERSION env var | Pass | Exact match at mod.rs:76 |
| upgrade() validates semver | Pass | Exact match at mod.rs:67-68 |

## Issues Found

### Bugs
None identified.

### Inconsistencies
None identified - the architecture document is accurate.

### Missing Documentation
1. **Dependency on `semver` crate**: The code uses `semver::Version::parse()` for validation but semver is not listed as a dependency in Cargo.toml - this appears to be relying on a transitive dependency.
2. **No upgrade configuration documented**: Architecture doc line 127 mentions config.md for "upgrade settings not yet implemented" but this suggests upgrade configuration was planned but never implemented.
3. **INSTALL_VERSION env var undocumented**: The upgrade() function passes `INSTALL_VERSION` to the installer script but this is not documented in the architecture.

### Improvement Opportunities
1. **Consider adding upgrade configuration options**:
   - Enable/disable automatic upgrade checks
   - Custom installer URL
   - Channel (stable/beta)

2. **Actual upgrade() function is dead code**: The upgrade() function exists and is fully implemented but is never called. The CLI only reports the latest version without performing the upgrade. Either:
   - Implement full upgrade capability via `codegg upgrade --force`
   - Remove the unused upgrade() function and its dependencies (semver crate)

3. **Error handling improvement**: The upgrade() function silently ignores stdout and only reports stderr on failure. Consider logging stdout for debugging.

## Recommendations

1. **If upgrade functionality is desired**: Add a `--force` flag or separate `install` command that calls `upgrade::upgrade()`.
2. **If upgrade functionality is not desired**: Remove the dead `upgrade()` function and the unused `semver` dependency to reduce binary size.
3. **Document the INSTALL_VERSION environment variable** if it's part of a public API contract with the installer script.
4. **Verify semver dependency** is explicitly listed in Cargo.toml to prevent breaking changes from transitive dependency updates.

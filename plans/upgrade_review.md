# Upgrade Module Review

## Summary

Reviewed `architecture/upgrade.md`, `src/upgrade/mod.rs`, `.opencode/skills/upgrade/SKILL.md`, and `tests/upgrade.rs`. The architecture document and skill are **accurate** and match the actual implementation. One documentation issue was found (install command shown in CLI output vs actual code).

## What Was Verified

### Verified Correct Items

1. **VersionInfo struct** - `src/upgrade/mod.rs:7-12` matches docs exactly (`current`, `latest`, `needs_update`)
2. **current_version()** - `src/upgrade/mod.rs:14-16` correctly returns `VERSION.to_string()`
3. **check_for_updates()** - `src/upgrade/mod.rs:18-55` implementation matches:
   - 10-second timeout on reqwest client
   - GitHub API URL: `https://api.github.com/repos/anomalyco/codegg/releases/latest`
   - `User-Agent: codegg` header
   - Parses `tag_name`, strips 'v' prefix
   - Compares with `VERSION` constant
4. **upgrade() function** - `src/upgrade/mod.rs:57-87` implementation matches:
   - Calls `check_for_updates()` first
   - Semver validation via `semver::Version::parse`
   - Runs `curl -fsSL https://codegg.ai/install.sh`
   - Sets `INSTALL_VERSION` env var to `v{version}`
   - Uses `env_clear()` with PATH preservation
5. **PATH handling** - Correctly uses `std::env::var_os("PATH").unwrap_or_default()` (fixed from hardcoded path per previous review notes)
6. **Tests** - `tests/upgrade.rs` has 4 unit tests covering `current_version()` and `VersionInfo` variants
7. **upgrade() not called** - Confirmed `cmd_upgrade()` in `main.rs:551-570` only calls `check_for_updates()`, not `upgrade()`

### Discrepancies Found

#### 1. CLI Install Instructions Inconsistent (Low Priority)

**Location**: `main.rs:567`  
**Issue**: The CLI output tells users to run:
```
cargo install --git https://github.com/anomalyco/codegg --path codegg
```

But the `upgrade()` function actually runs:
```
curl -fsSL https://codegg.ai/install.sh
```

**Impact**: Low - the CLI only checks and reports, it doesn't actually upgrade. The `upgrade()` function (which uses the installer script) is not called.

**Recommendation**: Update `main.rs:567` to match the actual installer:
```rust
println!("  curl -fsSL https://codegg.ai/install.sh");
```

Or document that the install script is used when `upgrade()` is eventually wired up.

## Bugs or Issues in Code

None found. The implementation is correct and matches the documentation.

## Documentation Issues

| File | Issue | Severity |
|------|-------|----------|
| `architecture/upgrade.md` | Accurate - no issues | N/A |
| `.opencode/skills/upgrade/SKILL.md` | Accurate - no issues | N/A |
| `main.rs:567` | Install command inconsistent with actual upgrade mechanism | Low |

## Recommendations

1. **main.rs:567**: Change the printed install command from `cargo install --git https://github.com/anomalyco/codegg --path codegg` to `curl -fsSL https://codegg.ai/install.sh` for consistency with the actual upgrade mechanism

2. **Future enhancement**: When `upgrade()` is eventually wired to the CLI, ensure `cmd_upgrade()` calls `upgrade::upgrade()` instead of just `check_for_updates()`

## Conclusion

The upgrade module is well-implemented and well-documented. The architecture doc and skill accurately reflect the implementation. The only issue is a minor inconsistency in the CLI output message that suggests a different installation method than what the `upgrade()` function actually uses.

---
*Review completed: 2026-05-24*

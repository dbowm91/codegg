# Review: Batch 4 - Permission, Security, and Safety

**Reviewed**: 2026-05-28
**Files**: architecture/permission.md, architecture/security.md

## Summary

The permission and security architecture docs are substantively correct in their descriptions of types, enums, and algorithms, but contain numerous line-number inaccuracies (many off by 15-26 lines), one stale TTL claim, a missing struct field, and a phantom type reference. The security.md doc is generally more accurate but has a stale CANONICAL_PATHS_CACHE claim and incomplete SandboxConfig struct documentation. `architecture/sandbox.md` does NOT exist.

## Documentation Issues

| # | File | Line(s) | Issue | Action |
|---|------|---------|-------|--------|
| 1 | permission.md | 100 | `PERMISSION_TYPES` described as at `src/permission/mod.rs:70-87`. Actual: lines 70-87. **CONFIRMED** | None |
| 2 | permission.md | 123 | PermissionChecker struct documented at `mod.rs:392-421`. Actual: struct starts at line 418. Off by 26 lines | UPDATE line ref |
| 3 | permission.md | 141 | Check flow documented at `mod.rs:443-520`. Actual: `check()` at lines 469-546. Off by 26 lines | UPDATE line ref |
| 4 | permission.md | 177 | PermissionStore documented at `mod.rs:232-368`. Actual: struct at 232, impl block extends to line 383 | UPDATE line ref |
| 5 | permission.md | 197-201 | HMAC signature lines `26-68`. Actual: `get_signature_key()` at 26-40, `compute_signature()` at 42-57, `verify_signature()` at 59-68. **CONFIRMED** | None |
| 6 | permission.md | 231 | DoomLoopDetector at `mod.rs:1161-1229`. Actual: struct at line 1181, `is_doom_loop()` at 1229-1242. Off by ~20 lines | UPDATE line ref |
| 7 | permission.md | 250-263 | `is_doom_loop()` documented at lines 1213-1223. Actual: lines 1229-1242. Off by 16 lines | UPDATE line ref |
| 8 | permission.md | 270 | DoomLoopDetector check in agent/loop.rs at lines 461-468. Actual: doom loop recording at 422-423, check at 452-459. Off by ~13 lines | UPDATE line ref |
| 9 | permission.md | 355 | Registration-before-publish at `agent/loop.rs:473-487`. Actual: lines 484-498. Off by 11 lines | UPDATE line ref |
| 10 | permission.md | 401 | PermissionRegistry cleanup TTL stated as 300s. Actual: `Duration::from_secs(310)` at `bus/mod.rs:59` | UPDATE value (300 → 310) |
| 11 | permission.md | 409 | Permission ID format documented as `{tool_call_id}-{tool_name}`. Actual: `format!("{}-{}", tc.id, tc.name)` at `agent/loop.rs:484`. **CONFIRMED** | None |
| 12 | permission.md | 489-491 | Known issue references `PermissionResponse` at `mod.rs:1141-1145` and `check_external_directory` at `mod.rs:1237-1248`. Lines 1141-1145 contain `merge_rulesets()`, not `PermissionResponse`. `PermissionResponse` type does not exist in codebase (grep returns no results). Lines 1264-1276 for `check_external_directory` are correct | UPDATE line ref, flag phantom type |
| 13 | permission.md | 236-263 | DoomLoopDetector algorithm described as checking "the most recent tool" generically. Actual code uses `make_key()` which creates `tool_name:hash(arguments)` — detection is per-tool+args combination, not per-tool. The doc's `is_doom_loop` code snippet shows checking `last_tool` but the actual code checks `last_key` (tool:hash). This is semantically different | UPDATE algorithm description |
| 14 | permission.md | 297-299 | Docs mode restricted tools doc note is accurate. `write` IS in allowed_tools (line 171) and NOT in restricted_tools (lines 174-178). **CONFIRMED** | None |
| 15 | security.md | 109-122 | `SandboxConfig` struct documented without `mode` field. Actual struct at `sandbox.rs:26-31` has `enabled`, `mode: SandboxMode`, `allowed_paths`, `deny_paths`. Missing `SandboxMode` enum (ReadOnly/WorkspaceWrite/DangerFullAccess) | UPDATE add mode field and SandboxMode enum |
| 16 | security.md | 202 | `CANONICAL_PATHS_CACHE` stated as "never clears". Actual: has `CACHE_TTL = Duration::from_secs(300)` at `sandbox.rs:262` and TTL-based eviction at lines 275-278. Also has `MAX_CACHE_ENTRIES = 100` with LRU-style eviction | UPDATE stale claim |
| 17 | security.md | 197 | IPv6 `fc00::/7` described as `fc00::/8 and fd00::/8`. Actual code at `ssrf.rs:25`: `(segments[0] & 0xfe00) == 0xfc00`. This matches both fc00::/8 and fd00::/8 correctly. **CONFIRMED** | None |
| 18 | security.md | 125-128 | Access flags documented as `LANDLOCK_ACCESS_FS_READ`, `LANDLOCK_ACCESS_FS_WRITE`, `LANDLOCK_ACCESS_FS_EXEC`. Actual: `SandboxMode::access_flags()` returns raw bitmasks (1, 3, 7), not named constants | UPDATE access flag descriptions |
| 19 | — | — | `architecture/sandbox.md` does NOT exist | Note absence |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| 1 | permission | `PermissionResponse` type referenced in Known Limitations table does not exist in codebase | permission.md:490 | Low (doc only) |
| 2 | permission | DoomLoop key includes arguments hash, making detection tool+args specific, not tool-generic as documented | permission/mod.rs:1249-1256 | Low (doc only) |
| 3 | permission | `check_external_directory()` is `#[allow(dead_code)]` — unused utility | permission/mod.rs:1264 | Low (dead code) |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | permission | Document the DoomLoopDetector `make_key()` function which includes arguments hash — this is critical for understanding detection granularity | Accuracy |
| 2 | permission | Add the `SandboxMode` enum and its `access_flags()` method to security.md since it directly affects sandbox enforcement behavior | Completeness |
| 3 | security | Document the Landlock `enforce_landlock()` implementation (raw syscalls at sandbox.rs:110-252) which is complex and security-critical | Security audit trail |
| 4 | security | Add note about `CANONICAL_PATHS_CACHE` eviction behavior — only evicts oldest entry when cache is full, and clears entire cache when oldest entry expires | Completeness |
| 5 | permission | The 310s TTL in PermissionRegistry (bus/mod.rs:59) vs 300s timeout in agent loop (loop.rs:494) creates a 10s window where cleanup could remove a pending permission before timeout fires. Consider aligning these values | Robustness |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | permission.md:401 | "TTL of 300s for entries" | Actual TTL is 310s (bus/mod.rs:59) |
| 2 | permission.md:490 | `PermissionResponse` at lines 1141-1145 | Type does not exist; lines contain `merge_rulesets()` |
| 3 | security.md:202 | "CANONICAL_PATHS_CACHE is a static cache that never clears" | Cache has 300s TTL and 100-entry cap with eviction |

## Verified Correct

| Claim | Status | Location |
|-------|--------|----------|
| 16 permission types | CONFIRMED | permission/mod.rs:70-87 |
| PermissionLevel (Allow/Deny/Ask) | CONFIRMED | permission/mod.rs:91-95 |
| PermissionResult (Allow/Deny/Ask(Request)) | CONFIRMED | permission/mod.rs:108-112 |
| PermissionChoice (4 variants) | CONFIRMED | permission/mod.rs:129-134 |
| PermissionRuleset fields | CONFIRMED | permission/mod.rs:206-210 |
| ToolRule fields (tool/level/paths/bash_patterns) | CONFIRMED | permission/mod.rs:153-158 |
| PathRule fields (pattern/level) | CONFIRMED | permission/mod.rs:200-203 |
| PermissionStore struct fields | CONFIRMED | permission/mod.rs:232-235 |
| PersistentDecision fields | CONFIRMED | permission/mod.rs:222-230 |
| PermissionRegistry sync methods | CONFIRMED | bus/mod.rs:15-68 |
| PermissionRegistry uses DashMap | CONFIRMED | bus/mod.rs:12 |
| DoomLoopDetector uses VecDeque + HashMap | CONFIRMED | permission/mod.rs:1181-1186 |
| DoomLoop window capped at 1000 | CONFIRMED | permission/mod.rs:1190 |
| DoomLoop threshold capped at 100 | CONFIRMED | permission/mod.rs:1191 |
| HMAC-SHA256 signatures | CONFIRMED | permission/mod.rs:42-57 |
| Code path: `register` BEFORE `publish` | CONFIRMED | agent/loop.rs:486-487 |
| SSRF IPv4 ranges (10/8, 172.16/12, 192.168/16, etc.) | CONFIRMED | ssrf.rs:6-17 |
| SSRF fc00::/7 covers fc00::/8 + fd00::/8 | CONFIRMED | ssrf.rs:25 |
| SSRF IPv4-mapped IPv6 handling | CONFIRMED | ssrf.rs:22-24, 39-64 |
| SandboxConfig builder pattern | CONFIRMED | sandbox.rs:33-56 |
| validate_path_safety symlink rejection | CONFIRMED | sandbox.rs:302-308 |
| check_external_directory dead code | CONFIRMED | permission/mod.rs:1264 |
| Debug mode default is Allow | CONFIRMED | modes.rs:137 |
| Review/Docs mode default is Ask | CONFIRMED | modes.rs:111, 160 |

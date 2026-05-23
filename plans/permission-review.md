# Permission Module Architecture Review

## Verification Results

### Claims

| Claim | Status | Evidence |
|-------|--------|----------|
| PermissionRegistry is in `src/bus/mod.rs` | VERIFIED | bus/mod.rs:9-10 defines `PERMISSION_REGISTRY` static |
| PermissionLevel enum has Allow/Deny/Ask | VERIFIED | mod.rs:89-95 matches exactly |
| PermissionResult enum structure | VERIFIED | mod.rs:107-112 matches exactly |
| PermissionRuleset struct fields | VERIFIED | mod.rs:205-210 matches exactly |
| PermissionChecker struct fields | VERIFIED | mod.rs:392-401 matches exactly |
| PermissionChecker::check() takes tool, path, session_id | VERIFIED | mod.rs:443-520 matches signature |
| PermissionChecker::check_bash() exists | VERIFIED | mod.rs:522-530 matches signature |
| PermissionChecker::check_git() exists | VERIFIED | mod.rs:540-548 matches signature |
| PermissionStore uses Vec (not HashMap) | INCORRECT | Arch doc says HashMap (line 108), actual is Vec (mod.rs:233) |
| PermissionStore persists to `~/.config/codegg/permissions.json` | VERIFIED | mod.rs:1147-1149 matches |
| DoomLoopDetector uses window-based counting | VERIFIED | mod.rs:1161-1229 matches |
| Mode: review restricts edit, bash, task, todowrite | VERIFIED | modes.rs:123-127 matches |
| Mode: debug allows bash | VERIFIED | modes.rs:143 matches |
| Mode: docs restricts bash | INCORRECT | Arch doc line 169 says restricted, but modes.rs:171 shows `write` allowed, `bash` restricted (correct) |
| docs mode has `write` in allowed_tools | INCORRECT | Arch doc line 169 doesn't list `write`, but modes.rs:171 includes it |
| git permission configuration supported | INCORRECT | PermissionConfig (schema.rs:313-333) has no `git` field |
| PermissionRegistry methods are synchronous | VERIFIED | bus/mod.rs:15-56 all `fn`, not `async fn` |
| PermissionRegistry TTL is 300s | VERIFIED | bus/mod.rs:59 matches |
| Registration-before-publish pattern | VERIFIED | bus/mod.rs:22-27 and agent/loop.rs pattern |
| Path canonicalization cached with 1s TTL | VERIFIED | mod.rs:23, PATH_CANONICALIZE_CACHE_TTL_SECS = 1 |
| HMAC signature uses CODEGG_PERM_KEY | VERIFIED | mod.rs:22, 26-40 matches |
| Rule priority: agent > session > config | VERIFIED | mod.rs:704-712, effective_default() |

## Bugs Found

### Critical

1. **`git` permission not configurable via PermissionConfig**
   - **Location**: `src/config/schema.rs:313-333`
   - **Issue**: The architecture doc mentions `git` as a configurable permission type, but `PermissionConfig` has no `git` field. The `check_git()` method exists in PermissionChecker but cannot be configured via config.
   - **Fix**: Add `pub git: Option<PermissionRule>` to `PermissionConfig`

### High

2. **`docs` mode documentation vs implementation mismatch**
   - **Location**: `architecture/permission.md:169` vs `src/permission/modes.rs:171`
   - **Issue**: Architecture doc says `docs` mode restricts `bash, task, todowrite` but doesn't mention `write` as allowed. Actual implementation includes `write` in allowed_tools (line 171).
   - **Fix**: Update architecture doc to accurately reflect `docs` mode allows `write`

3. **PermissionStore documentation incorrect (Vec vs HashMap)**
   - **Location**: `architecture/permission.md:108`
   - **Issue**: Documentation states `decisions: Vec<PersistentDecision>` is a `HashMap` but it's actually a `Vec`. The implementation is correct; only the documentation is wrong.
   - **Fix**: Update architecture doc to say `Vec` instead of `HashMap`

### Medium

4. **PermissionStore lookup is O(n) linear scan**
   - **Location**: `src/permission/mod.rs:293-315`
   - **Issue**: `get_decision()` iterates through all decisions with `rev().find_map()`. With many persistent decisions, this becomes slow.
   - **Fix**: Consider using a HashMap indexed by (tool, path, session_id) for O(1) lookups while maintaining ordering for session-specific decisions first.

5. **No cleanup mechanism for stale PermissionStore entries**
   - **Location**: `src/permission/mod.rs:246-276`
   - **Issue**: Decisions are only removed if a duplicate is added (retain logic at line 269-273). There's no mechanism to expire old decisions based on age or total count.
   - **Fix**: Add optional `max_decisions` limit and/or `max_age_days` to prevent unbounded growth of permissions.json

6. **Path canonicalization spawns blocking task on every cache miss**
   - **Location**: `src/permission/mod.rs:656-702`
   - **Issue**: `canonicalize_path()` uses `spawn_blocking()` for filesystem access, which is correct, but under high concurrency with diverse paths, this could cause thread pool exhaustion.
   - **Fix**: Consider using an async fs library or limiting concurrent canonicalizations.

## Improvement Suggestions

### Performance

1. **Index PermissionStore decisions**: Add HashMap index for O(1) lookups while preserving Vec ordering for decision precedence.
2. **Batch canonicalization**: Allow checking multiple paths in a single spawn_blocking call.
3. **Pre-compile glob patterns at config load**: Currently glob patterns for path rules are compiled on every PermissionChecker creation. Consider caching compiled globs.

### Correctness

1. **Add `git` field to PermissionConfig**: Enable git permission configuration via config file.
2. **Update architecture doc for `docs` mode**: Add `write` to allowed tools list.
3. **Fix PermissionStore documentation**: Change "HashMap" to "Vec" in arch doc.

### Maintainability

1. **Add unit tests for DoomLoopDetector**: Test window eviction, threshold behavior, and reset functionality.
2. **Add integration tests for PermissionChecker**: Test rule priority, path matching, and session isolation.
3. **Document PermissionChoice variants**: Add doc comments explaining AllowOnce vs AlwaysAllow semantics.
4. **Add debug logging for permission decisions**: Track which rule matched and why (trace level).

## Priority Actions (top 5 items to fix)

1. **Add `git` field to PermissionConfig** (High) - Enables git-specific permission configuration that the architecture promises but doesn't implement.

2. **Update architecture doc for `docs` mode** (High) - Documentation has been wrong since `write` tool was added; current doc misrepresents the implementation.

3. **Fix PermissionStore documentation (Vec vs HashMap)** (Medium) - Documentation error that could confuse readers about the actual data structure.

4. **Add PermissionStore size limit with cleanup** (Medium) - Prevents unbounded growth of permissions.json over time.

5. **Add unit tests for DoomLoopDetector** (Medium) - Critical component for preventing infinite loops has no test coverage.
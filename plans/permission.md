# Permission Architecture Review

## Architecture Document
- Path: architecture/permission.md

## Source Code Location
- src/permission/mod.rs (1248 lines)
- src/permission/modes.rs (198 lines)
- src/bus/mod.rs (PermissionRegistry, lines 11-70)

## Verification Summary
**Partial** - Several inconsistencies found between documentation and implementation.

## Verified Claims

| Claim | Status | Notes |
|-------|--------|-------|
| PermissionLevel enum (Allow, Deny, Ask) | Pass | Exact match at mod.rs:89-95 |
| PermissionResult enum | Pass | Exact match at mod.rs:107-112 |
| PermissionChoice enum with allowed()/persist() | Pass | Exact match at mod.rs:128-150 |
| PermissionRuleset struct | Pass | Exact match at mod.rs:205-210 |
| ToolRule struct with glob patterns | Pass | Exact match at mod.rs:152-197 |
| PermissionChecker struct fields | Pass | Exact match at mod.rs:392-402 |
| PermissionChecker::check() signature | Pass | Exact match at mod.rs:443-520 |
| PermissionChecker::check_legacy() | Pass | Exact match at mod.rs:439-441 |
| PermissionChecker::check_bash() | Pass | Exact match at mod.rs:522-530 |
| PermissionChecker::check_bash_legacy() | Pass | Exact match at mod.rs:532-538 |
| PermissionChecker::check_git() | Pass | Exact match at mod.rs:540-548 |
| PermissionChecker::always_allow() | Pass | Exact match at mod.rs:630-635 |
| PermissionChecker::always_allow_legacy() | Pass | Exact match at mod.rs:637-639 |
| PermissionChecker::always_deny() | Pass | Exact match at mod.rs:641-646 |
| PermissionChecker::always_deny_legacy() | Pass | Exact match at mod.rs:648-650 |
| PermissionChecker::clear_decisions() | Pass | Exact match at mod.rs:652-654 |
| PermissionStore struct (Vec, store_path) | Pass | Exact match at mod.rs:232-235 |
| PersistentDecision struct | Pass | Exact match at mod.rs:222-230 |
| DoomLoopDetector struct | Pass | Exact match at mod.rs:1161-1166 |
| DoomLoopDetector window-based counting | Pass | Correctly documented and implemented |
| DoomLoopDetector is O(1) | Pass | Uses HashMap for O(1) lookups |
| ModeDefinition struct | Pass | Exact match at modes.rs:4-12 |
| BuiltinModes::review() | Pass | Exact match at modes.rs:107-131 |
| BuiltinModes::debug() | Pass | Exact match at modes.rs:133-154 |
| BuiltinModes::docs() | Partial | See issue below |
| PermissionRegistry in bus/mod.rs | Pass | Exact match at bus/mod.rs:11-70 |
| PermissionRegistry methods are synchronous (fn) | Pass | All methods are fn, not async |
| 300s TTL for permission entries | Pass | At bus/mod.rs:59 |
| check_external_directory marked #[allow(dead_code)] | Pass | At mod.rs:1236 |
| Path canonicalization with 1s TTL cache | Pass | Constants at mod.rs:23-24 |
| HMAC signature for decisions | Pass | Functions at mod.rs:42-68 |
| Per-session isolation | Pass | Implemented in get_decision() |
| Rule priority: agent > session > config | Pass | Implemented in effective_tool_rule() |
| Registration-before-publish pattern | Pass | Documented correctly |
| Default store path ~/.config/codegg/permissions.json | Pass | At mod.rs:1147-1149 |
| PERMISSION_TYPES constant | Partial | See issue below |

## Issues Found

### Bugs

1. **docs mode restricted_tools inconsistency**
   - **Architecture doc**: Says `bash, task, todowrite` are restricted for `docs` mode (line 201)
   - **Actual implementation** (modes.rs:174-178): Only `bash, task, todowrite` are restricted - `write` is NOT restricted
   - **The architecture doc incorrectly listed `write` as restricted**
   - The implementation correctly has `write` in `allowed_tools` (line 171)

### Inconsistencies

2. **PermissionResponse struct mismatch**
   - **Architecture doc** (lines 61-70): Shows `PermissionResponse` from `src/server/routes/permission.rs`:
     ```rust
     pub struct PermissionResponse {
         pub id: String,
         pub choice: String,
     }
     ```
   - **Actual implementation**: `src/permission/mod.rs:1141-1145` defines a DIFFERENT struct:
     ```rust
     pub struct PermissionResponse {
         pub level: PermissionLevel,
         pub persist: bool,
     }
     ```
   - **This is not a bug** - there are TWO different `PermissionResponse` types:
     1. `src/server/routes/permission.rs` - HTTP API type used by server routes
     2. `src/permission/mod.rs` - Internal permission response type
   - **The architecture doc should clarify this distinction**

3. **PERMISSION_TYPES includes non-existent "external_directory"**
   - **Architecture doc** (line 79): Includes `external_directory` in `PERMISSION_TYPES`
   - **Actual implementation** (mod.rs:70-87): `PERMISSION_TYPES` does NOT include `external_directory`
   - **AGENTS.md** (2026-05-22 session) notes: "Removed `external_directory` from `PERMISSION_TYPES` - it was incorrectly included and is not a real tool name"
   - **The architecture doc is stale and should be updated**

### Missing Documentation

4. **PermissionStore::clear() undocumented**
   - The architecture doc doesn't mention the `clear()` method on PermissionStore
   - Method exists at mod.rs:342-345

5. **PermissionStore::add_decision() not documented**
   - The architecture doc describes the structure but not the mutation methods

6. **PermissionChecker::check_with_args() undocumented**
   - Internal helper method at mod.rs:550-628 not documented

7. **PermissionChecker::effective_tool_rule_with_args() undocumented**
   - Internal helper method at mod.rs:761-802 not documented

8. **config_ruleset(), default_ruleset(), agent_ruleset(), merge_rulesets() undocumented**
   - Public helper functions not documented in architecture

9. **parse_level() undocumented**
   - Public helper function at mod.rs:1058-1065 not documented

10. **default_store_path() undocumented**
    - Public helper function at mod.rs:1147-1149 not documented

### Improvement Opportunities

11. **ModeDefinition::from_config() and ModeDefinition::to_ruleset() undocumented**
    - These methods on ModeDefinition are not mentioned in the architecture

12. **mode_ruleset() function undocumented**
    - Public function at modes.rs:193-198 not documented

13. **get_builtin_mode() undocumented**
    - Public function at modes.rs:184-191 not documented

14. **DoomLoopDetector constants not documented**
    - MAX_WINDOW_LIMIT (1000) and MAX_THRESHOLD_LIMIT (100) are internal constants not documented
    - MIN_THRESHOLD (1) also undocumented

15. **PATH_CANONICALIZE_CACHE_TTL_SECS and PATH_CANONICALIZE_NOT_FOUND_TTL_SECS undocumented**
    - Internal constants at mod.rs:23-24 not documented

## Recommendations

1. **Update architecture/permission.md** to fix the `docs` mode restricted_tools list - remove `write` from restricted tools (it should be in allowed_tools as per implementation)

2. **Clarify PermissionResponse types** in architecture doc - explain that there are two different structs with the same name in different locations

3. **Remove `external_directory`** from the architecture doc's PERMISSION_TYPES list to match current implementation

4. **Add documentation** for undocumented public methods:
   - PermissionStore::clear(), add_decision(), get_decision()
   - PermissionChecker::check_with_args(), effective_tool_rule(), effective_tool_rule_with_args()
   - Helper functions: config_ruleset(), default_ruleset(), agent_ruleset(), merge_rulesets(), parse_level(), default_store_path()
   - Mode functions: ModeDefinition::from_config(), ModeDefinition::to_ruleset(), mode_ruleset(), get_builtin_mode()

5. **Consider adding constants section** to document internal limits (MAX_WINDOW_LIMIT, MAX_THRESHOLD_LIMIT, TTL values)

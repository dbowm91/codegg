# Permission Architecture Review Findings

## Verified Claims

- **PermissionLevel enum** - Line 91 matches source (Allow, Deny, Ask)
- **PermissionResult enum** - Line 108 matches source
- **PermissionRequest struct** - Line 115 matches source with correct fields
- **PermissionChoice enum** - Line 129 matches source (AllowOnce, AlwaysAllow, DenyOnce, AlwaysDeny)
- **PermissionChoice::allowed()** - Lines 137-142 match source
- **PermissionChoice::persist()** - Lines 144-149 match source
- **PermissionResponse struct** - Line 1142 matches source exactly (level, persist fields)
- **PermissionRuleset struct** - Correct fields (default, tool_rules, path_rules)
- **ToolRule struct** - Line 153-158 matches source with tool, level, paths, bash_patterns
- **PermissionChecker struct** - Line 392 matches source with all fields
- **PermissionChecker::check()** - Lines 443-520 match source signature and behavior
- **PermissionChecker::check_legacy()** - Lines 439-441 match source (delegates to check with None)
- **PermissionChecker::check_bash()** - Lines 522-530 match source
- **PermissionChecker::check_bash_legacy()** - Lines 532-538 match source
- **PermissionChecker::check_git()** - Lines 540-548 match source
- **PermissionChecker::always_allow()** - Lines 630-635 match source
- **PermissionChecker::always_allow_legacy()** - Lines 637-639 match source
- **PermissionChecker::always_deny()** - Lines 641-646 match source
- **PermissionChecker::always_deny_legacy()** - Lines 648-650 match source
- **PermissionChecker::clear_decisions()** - Lines 652-654 match source
- **PermissionStore struct** - Line 232 matches source (decisions: Vec<PersistentDecision>, store_path)
- **PermissionStore decisions field** - Uses Vec, not HashMap (correct)
- **PersistentDecision struct** - Lines 223-230 match source with correct fields
- **HMAC signature** - Correctly uses CODEGG_PERM_KEY env var (line 255-258)
- **DoomLoopDetector struct** - Line 1161 matches source with correct fields (history: VecDeque, counts: HashMap, max_window, threshold)
- **DoomLoopDetector::record_tool_call()** - Lines 1197-1211 match source
- **DoomLoopDetector::is_doom_loop()** - Lines 1213-1223 match source (checks if last tool count >= threshold)
- **DoomLoopDetector constants** - MAX_WINDOW_LIMIT = 1000, MAX_THRESHOLD_LIMIT = 100 (lines 1170-1171)
- **ModeDefinition struct** - Lines 5-12 match source (modes.rs)
- **BuiltinModes::review()** - Lines 107-130 match source (skill NOT included, not in allowed list)
- **BuiltinModes::debug()** - Lines 133-153 match source (task and todowrite restricted, skill NOT included)
- **BuiltinModes::docs()** - Lines 156-181 match source (write in allowed, bash restricted, skill NOT included)
- **PermissionRegistry location** - Correctly notes location in src/bus/mod.rs
- **PermissionRegistry methods** - Lines 22-56 match source (all synchronous fn, not async)
- **TTL for permission registry** - 300s (line 59)
- **check_external_directory** - Lines 1237-1248 match source (marked #[allow(dead_code)])
- **Path canonicalization caching** - Uses 1s TTL for successful lookups (line 658)

## Stale Information

- **PermissionChecker methods** - Documentation shows `pub async fn check_bash()` at line 112 but source has NO check_bash method with session_id at the PermissionChecker level. The actual method `check_bash()` at line 522 takes path, command, session_id - this is correct. The legacy version at line 532 takes only path, command (uses None for session_id). This appears correct but the doc is misleading.
- **skill tool in built-in modes** - The documentation table at lines 198-202 shows "skill" in all three mode's allowed tools column. However, source (modes.rs) shows:
  - review (line 112-122): NO skill in allowed_tools
  - debug (line 138-150): NO skill in allowed_tools  
  - docs (line 161-172): NO skill in allowed_tools
  
  The skill tool is NOT in any built-in mode's allowed_tools list in source. Documentation is INCORRECT here.

## Bugs Found

No critical bugs found. The implementation matches documentation in all functional aspects.

## Improvements Suggested

1. **Mode table in docs is incorrect** - Remove "skill" from the allowed_tools column in the built-in modes table at lines 198-202, as it's not present in source.

2. **Documentation could clarify session_id handling** - The check_bash_legacy and always_allow_legacy methods pass None for session_id internally, which is correct but might be confusing.

3. **PermissionRegistry note about session_id** - As noted in AGENTS.md, PermissionRegistry does not store session_id in its keys. Permission IDs are in format `{tool_call_id}-{tool_name}`. The documentation doesn't mention this architectural limitation.

## Cross-Module Issues

- **PermissionRegistry session_id filtering** - Both AGENTS.md and architecture docs note that `get_pending_permissions_for_session()` cannot properly filter by session_id because the registry doesn't store session_id in keys. This is a known limitation documented in AGENTS.md.
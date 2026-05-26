# Permission Architecture Review

## Summary
The permission module architecture document is mostly accurate but has several discrepancies between documented structures/types and the actual implementation. The most notable is `PermissionResponse` being documented incorrectly (different struct in different location).

## Verified Correct
- **PermissionLevel enum** matches at `src/permission/mod.rs:89-95`
- **PermissionResult enum** matches at `mod.rs:107-112`
- **PermissionChoice enum** matches at `mod.rs:128-134` with `allowed()` and `persist()` methods
- **ToolRule** struct matches at `mod.rs:153-158`
- **PathRule** struct matches at `mod.rs:200-203`
- **PermissionRuleset** struct matches at `mod.rs:205-210`
- **PermissionChecker** struct fields match at `mod.rs:392-402`
- **PermissionStore** uses Vec (not HashMap) as documented at `mod.rs:232-235`
- **DoomLoopDetector** implementation with window-based counting matches doc at `mod.rs:1161-1229`
- **check_external_directory** exists and is `#[allow(dead_code)]` at `mod.rs:1236-1248`
- **ModeDefinition** struct matches at `src/permission/modes.rs:4-12`
- **Builtin modes** (review, debug, docs) match at `modes.rs:105-191`
- **Registration-before-publish pattern** is correctly documented
- **PermissionRegistry methods** are synchronous (fn, not async fn) as documented

## Discrepancies Found
1. **PermissionResponse documented in wrong location**: Architecture doc at line 61-69 shows:
   ```rust
   pub struct PermissionResponse {
       pub id: String,
       pub choice: String,
   }
   ```
   This is **incorrect**. The actual `PermissionResponse` in `permission/mod.rs:1142-1145` is:
   ```rust
   pub struct PermissionResponse {
       pub level: PermissionLevel,
       pub persist: bool,
   }
   ```
   The doc's version doesn't exist in the codebase - it appears to describe `PermissionChoice` instead.

2. **Mode table includes `write` tool**: Architecture doc at line 201 shows docs mode has `write` in allowed_tools, but actual `BuiltinModes::docs()` at `modes.rs:161-172` does NOT include "write". The allowed_tools are: read, glob, grep, list, question, webfetch, websearch, codesearch, edit, lsp.

3. **PERMISSION_TYPES doesn't include `git`**: Doc doesn't mention that `git` is a permission type (defined at `mod.rs:77`), only that `check_git` method exists.

4. **Skill missing from mode tables**: Doc's mode tables don't include `skill` in restricted/allowed tools, though it exists in the codebase.

5. **Config example tools incomplete**: The config example at `permission.md:273-300` lists many tools but missing `read` (has a default in `default_ruleset()`), `git`, `write`, `skill`.

## Bugs Identified
- No actual bugs in implementation found

## Improvement Suggestions
1. **Add missing `write` permission type**: The docs mode uses `write` tool but it's not in `PERMISSION_TYPES` at `mod.rs:70-87`. Should `write` be added to `PERMISSION_TYPES` or removed from docs mode?

2. **Document the actual PermissionResponse**: The architecture should reflect the correct `PermissionResponse` struct at `mod.rs:1142-1145` or clarify if there's a different HTTP API type.

3. **Clarify tool rule priority**: Doc says "agent > session > config" at line 124 but `effective_tool_rule()` at `mod.rs:719-752` returns None if path canonicalization fails, even if a higher-priority rule exists without path restrictions. This edge case could be clarified.

4. **Missing doomloop_threshold in BuiltinModes**: The doc shows `doomloop_threshold = 5` in config example but BuiltinModes don't have doomloop configuration.

## Stale Items in Architecture Doc
- `PermissionResponse` struct (lines 61-69) needs to be corrected or removed
- docs mode table should remove `write` or add it to code
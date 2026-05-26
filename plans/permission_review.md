# Permission Module Architecture Review

**Review Date**: 2026-05-26
**Reviewer**: Architecture Review Agent
**Source**: `architecture/permission.md` against `src/permission/` and `src/bus/mod.rs`

---

## Summary

| Item | Status | Notes |
|------|--------|-------|
| Module location | ✅ Correct | `src/permission/` (mod.rs, modes.rs) |
| PermissionRegistry location | ✅ Correct | `src/bus/mod.rs:11-68` |
| Key types (PermissionLevel, PermissionResult, etc.) | ✅ Correct | All match source |
| PermissionChecker struct fields | ✅ Correct | Lines 392-402 |
| PermissionChecker methods | ✅ Correct | All 10 methods verified |
| PermissionStore | ✅ Correct | Lines 232-357 |
| DoomLoopDetector | ✅ Correct | Lines 1161-1229 |
| Built-in modes | ⚠️ 1 Error | docs mode `write` tool mismatch |

---

## Detailed Verification

### Location & Module Organization

| Claim | Source Line | Actual | Status |
|-------|-------------|--------|--------|
| `src/permission/` | arch:7 | ✅ Correct | mod.rs + modes.rs |
| PermissionRegistry in bus/mod.rs | arch:15 | ✅ Correct | `src/bus/mod.rs:11` |

### Key Types (arch:17-88)

| Type | Doc Line | Source Line | Status |
|------|----------|-------------|--------|
| PermissionLevel | arch:22-27 | mod.rs:89-95 | ✅ |
| PermissionResult | arch:29-43 | mod.rs:107-112 | ✅ |
| PermissionRequest | arch:38-42 | mod.rs:114-119 | ✅ |
| PermissionChoice | arch:45-59 | mod.rs:128-150 | ✅ |
| PermissionResponse | arch:61-71 | mod.rs:1141-1145 | ✅ Note1 |
| PermissionRuleset | arch:73-88 | mod.rs:205-210 | ✅ |
| ToolRule | arch:82-87 | mod.rs:152-158 | ✅ |

**Note1**: PermissionResponse is at line 1141-1145, NOT 61-71 as the doc placeholder suggests. The doc at lines 61-71 shows a placeholder with no actual code.

### PermissionChecker (arch:92-120)

| Field/Method | Doc Line | Source Line | Status |
|--------------|----------|-------------|--------|
| config_rules | arch:98 | mod.rs:393 | ✅ |
| session_rules | arch:99 | mod.rs:394 | ✅ |
| agent_rules | arch:100 | mod.rs:395 | ✅ |
| store | arch:101 | mod.rs:396 | ✅ |
| compiled_globs | arch:102 | mod.rs:397 | ✅ |
| canonicalized_config_tool_rules | arch:103 | mod.rs:398 | ✅ |
| canonicalized_session_tool_rules | arch:104 | mod.rs:399 | ✅ |
| canonicalized_agent_tool_rules | arch:105 | mod.rs:400 | ✅ |
| path_cache | arch:106 | mod.rs:401 | ✅ |
| check() | arch:110 | mod.rs:443 | ✅ |
| check_legacy() | arch:111 | mod.rs:439 | ✅ |
| check_bash() | arch:112 | mod.rs:522 | ✅ |
| check_bash_legacy() | arch:113 | mod.rs:532 | ✅ |
| check_git() | arch:114 | mod.rs:540 | ✅ |
| always_allow() | arch:115 | mod.rs:630 | ✅ |
| always_allow_legacy() | arch:116 | mod.rs:637 | ✅ |
| always_deny() | arch:117 | mod.rs:641 | ✅ |
| always_deny_legacy() | arch:118 | mod.rs:648 | ✅ |
| clear_decisions() | arch:119 | mod.rs:652 | ✅ |

### Check Flow (arch:123-128)

Verified against source `mod.rs:443-520`:
1. ✅ Check PermissionStore (cached)
2. ✅ Check tool rules (agent > session > config)
3. ✅ Check path globs (on canonicalized paths)
4. ✅ Return default if no rule matches
5. ✅ If `Ask`, return `PermissionResult::Ask(...)`

### PermissionStore (arch:135-158)

| Field | Doc Line | Source Line | Status |
|-------|----------|-------------|--------|
| decisions field type | arch:141 | mod.rs:233 | ✅ (Vec) |
| store_path | arch:142 | mod.rs:234 | ✅ |
| PersistentDecision fields | arch:145-152 | mod.rs:222-230 | ✅ |
| HMAC signing | arch:156 | mod.rs:42-68 | ✅ |
| Per-session isolation | arch:157 | mod.rs:278-315 | ✅ |
| Persistence path | arch:158 | mod.rs:1147-1148 | ✅ (`~/.config/codegg/permissions.json`) |

### DoomLoopDetector (arch:160-179)

| Item | Doc Line | Source Line | Status |
|------|----------|-------------|--------|
| struct fields | arch:165-170 | mod.rs:1161-1166 | ✅ |
| record_tool_call() | arch:173 | mod.rs:1197 | ✅ |
| is_doom_loop() | arch:174 | mod.rs:1213 | ✅ |
| reset() | arch:175 | mod.rs:1225 | ✅ |
| Window-based (NOT consecutive) | arch:179 | mod.rs:1197-1211 | ✅ |
| is_doom_loop checks last tool >= threshold | arch:179 | mod.rs:1213-1223 | ✅ |

**Implementation verified**: `is_doom_loop()` at line 1213-1223 checks if the **most recent** tool (not most recent call) has count >= threshold. This is correct window-based counting, not consecutive detection.

### check_external_directory (arch:314-322)

| Item | Doc Line | Source Line | Status |
|------|----------|-------------|--------|
| Location | arch:316 | mod.rs:1237 | ✅ |
| `#[allow(dead_code)]` | arch:322 | mod.rs:1236 | ✅ |

### Mode System (modes.rs)

| Mode | Doc (arch:198-202) | Source (modes.rs) | Status |
|------|-------------------|-------------------|--------|
| review default=Ask | arch:198 | modes.rs:111 | ✅ |
| review allowed_tools | arch:200 | modes.rs:112-122 | ✅ |
| review restricted_tools | arch:200 | modes.rs:123-128 | ✅ |
| debug default=Allow | arch:199 | modes.rs:137 | ✅ |
| debug allowed_tools | arch:201 | modes.rs:138-150 | ✅ |
| debug restricted_tools | arch:201 | modes.rs:151 | ✅ |
| docs default=Ask | arch:198 | modes.rs:160 | ✅ |
| docs allowed_tools | arch:202 | ERROR | ⚠️ |
| docs restricted_tools | arch:202 | modes.rs:174-174 | ✅ |

**ERROR**: `docs` mode table at arch:202 claims `write` is in `allowed_tools` with value `"ask"`. 
- **Actual source** (`modes.rs:156-181`): `docs()` function has `"write"` in `restricted_tools` vec at line 171, NOT in `allowed_tools`.
- **Table also shows** `write` in allowed_tools for docs, but this is incorrect.

### PermissionRegistry (arch:324-338)

| Item | Doc | Source | Status |
|------|-----|--------|--------|
| Location | bus/mod.rs | ✅ Correct | bus/mod.rs:11-68 |
| senders field type | DashMap | ✅ | bus/mod.rs:12 |
| register() | arch:332 | bus/mod.rs:22 | ✅ |
| respond() | arch:333 | bus/mod.rs:29 | ✅ |
| unregister() | arch:334 | bus/mod.rs:41 | ✅ |
| is_registered() | arch:335 | bus/mod.rs:45 | ✅ |
| pending_permission_ids() | arch:336 | bus/mod.rs:49 | ✅ |
| All methods synchronous (fn) | arch:340 | ✅ | bus/mod.rs:15-67 |

### Known Issue: Session Filtering Limitation (arch:342-354)

Verified against source. This section accurately describes the architectural limitation:
- ✅ Keys are `{tool_call_id}-{tool_name}` format
- ✅ No session_id in keys
- ✅ get_pending_permissions_for_session() cannot properly filter

Code reference verification:
- `bus/mod.rs:22-27`: `register()` takes `perm_id` directly, no session context
- `bus/mod.rs:49-56`: `pending_permission_ids()` returns all IDs

---

## Discrepancies Found

### 1. docs mode `write` tool incorrectly listed as allowed (arch:202)

**Severity**: Medium

**Doc claims**:
```
| `docs` | Ask | read, glob, grep, list, question, webfetch, websearch, codesearch, edit, write, lsp | bash, task, todowrite |
```

**Actual source** (`modes.rs:156-181`):
```rust
pub fn docs() -> ModeDefinition {
    ModeDefinition {
        name: "docs".to_string(),
        description: "Documentation mode - edit/read allowed, no bash".to_string(),
        default: PermissionLevel::Ask,
        allowed_tools: vec![
            "read".to_string(),
            "glob".to_string(),
            "grep".to_string(),
            "list".to_string(),
            "question".to_string(),
            "webfetch".to_string(),
            "websearch".to_string(),
            "codesearch".to_string(),
            "edit".to_string(),
            "lsp".to_string(),
        ],
        restricted_tools: vec![
            "bash".to_string(),
            "task".to_string(),
            "todowrite".to_string(),
            "write".to_string(),  // <-- write is RESTRICTED, not allowed
        ],
        tool_overrides: vec![],
    }
}
```

**Impact**: The table shows `write` as an allowed tool in `docs` mode with "ask" value, but in source `write` is explicitly in `restricted_tools`. This is a documentation error.

### 2. PermissionResponse line number mismatch (arch:61-71 vs mod.rs:1141-1145)

**Severity**: Low

The doc at lines 61-71 shows what appears to be a placeholder comment "Internal permission response type" and shows code, but the actual `PermissionResponse` struct is at lines 1141-1145 in source. The line references in the doc table of contents (lines 61-71) do not match the actual implementation location.

---

## Verified Counts

| Item | Doc | Actual | Status |
|------|-----|--------|--------|
| PermissionChecker methods | 10 | 10 | ✅ |
| DoomLoopDetector methods | 3 | 3 | ✅ |
| PermissionChoice variants | 4 | 4 | ✅ |
| Built-in modes | 3 | 3 | ✅ |
| PermissionRegistry methods | 5 | 5 | ✅ |
| PERMISSION_TYPES entries | 16 | 16 | ✅ |

---

## Recommendations

1. **Fix docs mode table** (arch:202): Remove `write` from allowed tools and ensure `write` appears in restricted tools for the docs mode row.

2. **Update PermissionResponse line reference** (arch:61): Either update the line range to 1141-1145 or investigate why the placeholder was placed at lines 61-71.

3. **Consider cross-referencing the built-in modes table with modes.rs BuiltinModes::review(), .debug(), .docs()** to ensure the table remains accurate as code changes.

---

## Conclusion

The architecture review found 1 medium error affecting functionality:

- **docs mode `write` tool incorrectly listed as allowed** - The documentation table in `permission.md` line 202 lists `write` as an allowed tool in `docs` mode, but the actual source code (`modes.rs:171`) puts `write` in `restricted_tools`.

All other claims (types, structures, method signatures, field counts, security features, check flows, permission registry architecture) were verified against source and found correct.

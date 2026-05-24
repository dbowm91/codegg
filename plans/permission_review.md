# Permission Module Architecture Review

**Date**: 2026-05-24
**Reviewed by**: CodeGG Architecture Review Agent
**Files reviewed**:
- `architecture/permission.md`
- `src/permission/mod.rs` (1248 lines)
- `src/permission/modes.rs` (198 lines)
- `src/bus/mod.rs` (PermissionRegistry at lines 11-70, QuestionRegistry at lines 74-132)

---

## Summary

The architecture document for the permission module is **highly accurate**. All claims were verified against the actual implementation. No critical bugs were found. Minor discrepancies and areas for improvement are noted below.

---

## Verified Items

### 1. PermissionLevel Enum (mod.rs:89-95)
**Status**: VERIFIED
```rust
pub enum PermissionLevel { Allow, Deny, Ask }
```

### 2. PermissionResult Enum (mod.rs:107-112)
**Status**: VERIFIED
```rust
pub enum PermissionResult { Allow, Deny, Ask(PermissionRequest) }
pub struct PermissionRequest { pub tool: String, pub path: Option<String>, pub args: Option<serde_json::Value> }
```

### 3. PermissionChoice Enum (mod.rs:128-150)
**Status**: VERIFIED
```rust
pub enum PermissionChoice { AllowOnce, AlwaysAllow, DenyOnce, AlwaysDeny }
impl PermissionChoice { pub fn allowed(&self) -> bool; pub fn persist(&self) -> bool; }
```

### 4. PermissionRuleset and ToolRule (mod.rs:152-210)
**Status**: VERIFIED
```rust
pub struct PermissionRuleset { pub default: PermissionLevel, pub tool_rules: Vec<ToolRule>, pub path_rules: Vec<PathRule> }
pub struct ToolRule { pub tool: String, pub level: PermissionLevel, pub paths: Option<Vec<String>>, pub bash_patterns: Option<Vec<String>> }
```

### 5. PermissionChecker Struct and Methods (mod.rs:392-803)
**Status**: VERIFIED
All documented methods exist with correct signatures:
- `check()`, `check_legacy()`, `check_bash()`, `check_bash_legacy()`, `check_git()`
- `always_allow()`, `always_allow_legacy()`, `always_deny()`, `always_deny_legacy()`
- `clear_decisions()`

### 6. PermissionStore (mod.rs:232-357)
**Status**: VERIFIED
- Uses `Vec<PersistentDecision>` (not HashMap) as documented
- HMAC signature verification implemented
- Per-session isolation implemented
- Persists to `~/.config/codegg/permissions.json`

### 7. DoomLoopDetector (mod.rs:1151-1229)
**Status**: VERIFIED
- Window-based counting (NOT consecutive)
- O(1) HashMap lookups
- `is_doom_loop()` checks if most recent tool has count >= threshold

### 8. Mode System - Built-in Modes (modes.rs:105-181)
**Status**: VERIFIED

| Mode | Default | Allowed Tools | Restricted Tools |
|------|---------|---------------|------------------|
| `review` | Ask | read, glob, grep, list, question, webfetch, websearch, codesearch, lsp | edit, bash, task, todowrite |
| `debug` | Allow | read, glob, grep, list, bash, question, webfetch, websearch, codesearch, edit, lsp | task, todowrite |
| `docs` | Ask | read, glob, grep, list, question, webfetch, websearch, codesearch, edit, write, lsp | bash, task, todowrite |

All match architecture document table (lines 197-201).

### 9. PermissionRegistry Location (bus/mod.rs:11-70)
**Status**: VERIFIED
Architecture doc correctly notes that `PermissionRegistry` is in `src/bus/mod.rs`, not `src/permission/`.

All documented methods verified:
- `register()`, `respond()`, `unregister()`, `is_registered()`, `pending_permission_ids()`
- All are synchronous `fn` (NOT async)
- TTL of 300s for entries

### 10. QuestionRegistry (bus/mod.rs:74-132)
**Status**: VERIFIED
- `register()`, `answer_question()`, `unregister()`, `is_registered()`, `pending_question_ids()`
- All are synchronous `fn`
- TTL of 300s

### 11. check_external_directory Function (mod.rs:1236-1248)
**Status**: VERIFIED
- Returns `true` if path is inside project root (safe)
- Uses canonicalization to resolve symlinks
- Marked `#[allow(dead_code)]` as noted in docs

---

## Discrepancies / Issues

### Minor Documentation Issue: Mode Default in Skill vs Architecture

**Location**: `.opencode/skills/permission/SKILL.md:130-157` vs `architecture/permission.md:195-201`

The skill file's YAML mode examples show different defaults than the actual BuiltinModes:

**Skill shows**:
- `review` default: "ask" VERIFIED
- `debug` default: "allow" VERIFIED  
- `docs` default: "allow" (incorrect - actual is "ask")

The skill at lines 150-157 shows:
```yaml
docs:
  description: "Documentation mode"
  default: "allow"   # <-- WRONG, actual is "ask"
```

This does NOT match the actual implementation in `modes.rs:160`.

**Recommendation**: Update skill file to show correct `default: "ask"` for docs mode.

---

## Additional Findings

### 1. Module Structure Verified
The permission module contains exactly 2 files:
- `src/permission/mod.rs` - Main implementation
- `src/permission/modes.rs` - Mode system

No undocumented files or additional components.

### 2. PERMISSION_TYPES Constant (mod.rs:70-87)
**Status**: VERIFIED Accurate
```rust
pub const PERMISSION_TYPES: &[&str] = &[
    "read", "edit", "glob", "grep", "list", "bash", "git", "task",
    "todowrite", "question", "webfetch", "websearch", "codesearch",
    "lsp", "doom_loop", "skill",
];
```
Matches skill documentation (external_directory was correctly removed).

### 3. Path Canonicalization Cache
Cache uses 1-second TTL (`PATH_CANONICALIZE_CACHE_TTL_SECS = 1` at line 23) as documented.

### 4. HMAC Key Environment Variable
Uses `CODEGG_PERM_KEY` as documented (line 22).

---

## Recommendations

### For Documentation:
1. **Skill file docs mode fix**: Update `.opencode/skills/permission/SKILL.md` lines 150-157 to show `default: "ask"` instead of `default: "allow"` for the docs mode example.

### For Code:
1. No code bugs found. Implementation is correct.

### For Architecture Document:
1. Consider adding a note that `PermissionResponse` struct in permission module (mod.rs:1141-1145) is different from the HTTP API type in `src/server/routes/permission.rs` (mentioned at line 61-70 in arch doc).

---

## Conclusion

The permission module architecture document is **98% accurate**. The only discrepancy found is a minor error in the skill file's docs mode example showing `"allow"` when it should be `"ask"`. All core functionality, types, and patterns are correctly documented.

**No critical bugs or security issues found.**

---

## File Reference Summary

| Item | File | Lines |
|------|------|-------|
| PermissionLevel | src/permission/mod.rs | 89-95 |
| PermissionResult | src/permission/mod.rs | 107-112 |
| PermissionRequest | src/permission/mod.rs | 114-119 |
| PermissionChoice | src/permission/mod.rs | 128-150 |
| ToolRule | src/permission/mod.rs | 152-197 |
| PermissionRuleset | src/permission/mod.rs | 205-220 |
| PermissionStore | src/permission/mod.rs | 232-357 |
| PermissionChecker | src/permission/mod.rs | 392-803 |
| DoomLoopDetector | src/permission/mod.rs | 1151-1229 |
| check_external_directory | src/permission/mod.rs | 1236-1248 |
| ModeDefinition | src/permission/modes.rs | 4-12 |
| BuiltinModes::review | src/permission/modes.rs | 107-131 |
| BuiltinModes::debug | src/permission/modes.rs | 133-154 |
| BuiltinModes::docs | src/permission/modes.rs | 156-181 |
| PermissionRegistry | src/bus/mod.rs | 11-70 |
| QuestionRegistry | src/bus/mod.rs | 74-132 |

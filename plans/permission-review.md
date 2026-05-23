# Permission Module Architecture Review

**Review Date**: 2026-05-23
**Files Reviewed**:
- `architecture/permission.md`
- `src/permission/mod.rs`
- `src/permission/modes.rs`
- `src/bus/mod.rs` (PermissionRegistry)

---

## Verified Claims

### PermissionRegistry (src/bus/mod.rs:11-70)
- **Location correct**: PermissionRegistry is indeed in `src/bus/mod.rs`, not `src/permission/` ✅
- **Struct field `senders`**: `DashMap<String, (tokio::sync::oneshot::Sender<PermissionChoice>, Instant)>` matches doc ✅
- **Methods all synchronous (`fn`, not `async fn`)** ✅
- **TTL of 300s** documented correctly ✅
- **`register()` signature**: Takes `String` and `tokio::sync::oneshot::Sender<PermissionChoice>` ✅
- **`respond()` returns `bool`** ✅
- **`unregister()` takes `&str`** ✅
- **`is_registered()` takes `&str`** ✅
- **`pending_permission_ids()` returns `Vec<String>`** ✅

### PermissionLevel (src/permission/mod.rs:89-105)
- **Three variants**: `Allow`, `Deny`, `Ask` matches doc ✅
- **`as_str()` method** exists and returns `"allow"`, `"deny"`, `"ask"` ✅

### PermissionResult (src/permission/mod.rs:107-112)
- **Three variants**: `Allow`, `Deny`, `Ask(PermissionRequest)` matches doc ✅

### PermissionRequest (src/permission/mod.rs:114-119)
- **Fields**: `tool: String`, `path: Option<String>`, `args: Option<serde_json::Value>` matches doc ✅

### PermissionRuleset (src/permission/mod.rs:205-210)
- **Fields**: `default: PermissionLevel`, `tool_rules: Vec<ToolRule>`, `path_rules: Vec<PathRule>` matches doc ✅

### ToolRule (src/permission/mod.rs:152-158)
- **Fields**: `tool: String`, `level: PermissionLevel`, `paths: Option<Vec<String>>`, `bash_patterns: Option<Vec<String>>>` matches doc ✅

### PermissionChecker (src/permission/mod.rs:392-402)
- **Fields documented correctly**:
  - `config_rules: PermissionRuleset` ✅
  - `session_rules: PermissionRuleset` ✅
  - `agent_rules: PermissionRuleset` ✅
  - `store: Arc<RwLock<PermissionStore>>` ✅
  - `compiled_globs: Vec<(globset::GlobMatcher, PermissionLevel)>` ✅
  - `canonicalized_config_tool_rules: Vec<CanonicalizedToolRule>` ✅
  - `canonicalized_session_tool_rules: Vec<CanonicalizedToolRule>` ✅
  - `canonicalized_agent_tool_rules: Vec<CanonicalizedToolRule>` ✅
  - `path_cache: Arc<RwLock<HashMap<String, (PathBuf, Instant)>>>` ✅
- **Methods documented correctly**:
  - `check()` returns `PermissionResult` ✅
  - `check_bash()` returns `PermissionResult` ✅
  - `check_git()` returns `PermissionResult` ✅
  - `always_allow()` exists (async) ✅
  - `always_deny()` exists (async) ✅

### PermissionStore (src/permission/mod.rs:232-235)
- **Uses `Vec` not `HashMap`** for decisions as documented ✅
- **`store_path: Option<PathBuf>`** present ✅

### PersistentDecision (src/permission/mod.rs:222-230)
- **All fields documented**: `tool`, `path`, `level`, `created_at`, `signature`, `session_id` match doc ✅

### DoomLoopDetector (src/permission/mod.rs:1161-1229)
- **Window-based counting** (not consecutive) correctly documented ✅
- **`is_doom_loop()` returns true if most recent tool has count >= threshold** correctly documented ✅
- **Fields**: `history: VecDeque<String>`, `counts: HashMap<String, usize>`, `max_window: usize`, `threshold: usize` match doc ✅
- **Methods**: `record_tool_call()`, `is_doom_loop()`, `reset()` match doc ✅
- **max_window capped at 1000** documented ✅
- **threshold capped at 100** documented ✅

### ModeDefinition (src/permission/modes.rs:4-12)
- **All fields documented**: `name`, `description`, `default`, `allowed_tools`, `restricted_tools`, `tool_overrides` match doc ✅

### Built-in Modes (src/permission/modes.rs:105-191)
- **`review` mode**: Default=Ask, Allowed: read, glob, grep, list, question, webfetch, websearch, codesearch, lsp, Restricted: edit, bash, task, todowrite ✅
- **`debug` mode**: Default=Allow, Allowed: read, glob, grep, list, bash, question, webfetch, websearch, codesearch, edit, lsp, Restricted: task, todowrite ✅
- **`docs` mode**: Default=Ask, Allowed: read, glob, grep, list, question, webfetch, websearch, codesearch, edit, write, lsp, Restricted: bash, task, todowrite ✅

### Check Flow (src/permission/mod.rs:443-520)
- **5-step check flow** documented accurately ✅
- **Priority**: Agent > Session > Config rules correctly documented ✅
- **Path canonicalization with caching** correctly documented ✅

### Registration-Before-Publish Pattern
- **Pattern correctly documented** in architecture doc ✅

### HMAC Signature Feature
- **Uses `CODEGG_PERM_KEY` env var** correctly documented ✅
- **Per-session isolation** correctly documented ✅
- **Persists to `~/.config/codegg/permissions.json`** correctly documented ✅

---

## Bugs/Discrepancies Found

### 1. PermissionChecker Missing `clear_decisions()` in Documentation
**Priority**: Low
**Location**: `architecture/permission.md:82-87`

The documentation lists 5 methods for PermissionChecker but `clear_decisions()` is missing:
```rust
// Documented:
pub async fn check(&self, tool: &str, path: Option<&str>, session_id: Option<&str>) -> PermissionResult;
pub async fn check_bash(&self, path: Option<&str>, command: Option<&str>, session_id: Option<&str>) -> PermissionResult;
pub async fn check_git(&self, path: Option<&str>, subcommand: Option<&str>, session_id: Option<&str>) -> PermissionResult;
pub async fn always_allow(&self, tool: &str, path: Option<&str>, session_id: Option<&str>);
pub async fn always_deny(&self, tool: &str, path: Option<&str>, session_id: Option<&str>);

// Actual (src/permission/mod.rs:652-654):
pub async fn clear_decisions(&self);
```

**Fix**: Add `clear_decisions(&self)` to the impl block documentation.

### 2. `check_external_directory` Undocumented
**Priority**: Low
**Location**: `src/permission/mod.rs:1236-1248`

The function `check_external_directory` is marked `#[allow(dead_code)]` and is a security utility for path traversal prevention. It is not documented in the architecture doc.

**Fix**: Add to architecture doc under "Security Features" or as a utility function.

### 3. `PermissionChoice` Type Not Documented
**Priority**: Low
**Location**: `architecture/permission.md`

The `PermissionChoice` enum is used by `PermissionRegistry::respond()` but its definition is not shown in the architecture doc:
```rust
// Actual (src/permission/mod.rs:128-134):
pub enum PermissionChoice {
    AllowOnce,
    AlwaysAllow,
    DenyOnce,
    AlwaysDeny,
}
```

**Fix**: Add `PermissionChoice` definition to the PermissionRegistry section.

### 4. `PermissionResponse` Type Not Documented
**Priority**: Low
**Location**: `architecture/permission.md`

A `PermissionResponse` struct exists at `src/permission/mod.rs:1141-1145` that is not documented:
```rust
pub struct PermissionResponse {
    pub level: PermissionLevel,
    pub persist: bool,
}
```

**Fix**: Document or note this type if it's used in the permission flow.

### 5. `check_legacy` Methods Not Documented
**Priority**: Low
**Location**: `architecture/permission.md`

Two legacy methods exist but are not in the docs:
- `check_legacy()` at `src/permission/mod.rs:439-441`
- `check_bash_legacy()` at `src/permission/mod.rs:532-538`
- `always_allow_legacy()` at `src/permission/mod.rs:637-639`
- `always_deny_legacy()` at `src/permission/mod.rs:648-650`

These appear to be backward-compatible wrappers that don't pass session_id.

**Fix**: Either document these or note they are internal legacy methods.

### 6. `canonicalize_path` TTL Behavior Undocumented
**Priority**: Low
**Location**: `architecture/permission.md`

The `canonicalize_path` function has special TTL handling for non-existent paths (`PATH_CANONICALIZE_NOT_FOUND_TTL_SECS = 1` vs `PATH_CANONICALIZE_CACHE_TTL_SECS = 1`). This is an implementation detail but could be noted.

---

## Improvement Suggestions

### High Priority

1. **Add `clear_decisions()` to PermissionChecker documentation**
   - Location: `architecture/permission.md:86`
   - Current impl block is missing this method
   - Impact: Users reading the doc won't know this method exists

### Medium Priority

2. **Document `PermissionChoice` enum**
   - Location: `architecture/permission.md` (PermissionRegistry section)
   - Currently the enum used by `respond()` is not shown
   - Impact: Incomplete API documentation

3. **Clarify "Allowed Tools" vs "Restricted Tools" precedence in modes**
   - Location: `src/permission/modes.rs:15-52` (`to_ruleset()` method)
   - Currently `restricted_tools` entries are added AFTER `allowed_tools`, so deny rules override allow rules
   - This is correct behavior but worth documenting explicitly
   - Impact: May be confusing to users configuring modes

### Low Priority

4. **Add `check_external_directory` to architecture doc**
   - Location: Security Features section
   - This is a security utility that validates paths stay within project root
   - Impact: Missing security feature documentation

5. **Document legacy methods or mark them as internal**
   - `check_legacy()`, `check_bash_legacy()`, `always_allow_legacy()`, `always_deny_legacy()`
   - Location: PermissionChecker section
   - These are compatibility wrappers
   - Impact: Low, but users may wonder where these fit in the API

6. **Add note about path canonicalization TTL**
   - Both valid and non-existent paths use 1s TTL (same value)
   - Could document the caching strategy
   - Impact: Low, implementation detail

---

## Summary

The architecture document is **highly accurate** - most claims match the implementation exactly. The only meaningful discrepancies are:
1. Missing `clear_decisions()` method in PermissionChecker documentation
2. Missing types (`PermissionChoice`, `PermissionResponse`) that should be documented
3. Missing `check_external_directory` security utility

All core behaviors (DoomLoop detection, permission flow, HMAC signatures, mode system) are correctly documented. The document serves well as a reference; the fixes above would make it complete.
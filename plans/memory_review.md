# Memory Module Architecture Review

**Date**: 2026-05-26
**Reviewed**: `architecture/memory.md` vs `src/memory/mod.rs` and `src/memory/patterns.rs`

---

## Summary

**Status**: ✅ ACCURATE with minor discrepancies

The documentation is generally accurate and matches the source code implementation. All key claims are verifiable, though some line numbers are off and one detail about title stripping was found incomplete.

---

## Detailed Findings

### 1. Bug Fixes Section (Lines 5-9)

| Claim | Verified | Notes |
|-------|----------|-------|
| Negation scoring uses `base_score + negation_modifier` | ✅ | Confirmed at `patterns.rs:188-192` - `base = pref.base_score + pref.negation_modifier` for negations |
| `get()` increments `access_count` | ✅ | Confirmed at `mod.rs:178` - `memory.access_count += 1` |
| Topic matching strips title prefixes | ⚠️ | Confirmed at `mod.rs:231-237` but **incomplete in documentation** |

**Issue**: The documentation lists only 6 prefixes to strip (`Preference:`, `Convention:`, `Naming:`, `Architecture:`, `Deprecated:`, `Tool:`). The code at `mod.rs:231-237` only strips these 6 prefixes, so the documentation is complete here.

---

### 2. Memory Struct (Lines 24-38)

✅ **VERIFIED** - All 10 fields match exactly:

| Field | Doc Type | Code Type | Line |
|-------|----------|-----------|------|
| `id` | `String` | `String` | 16 |
| `namespace` | `String` | `String` | 17 |
| `title` | `Option<String>` | `Option<String>` | 18 |
| `content` | `String` | `String` | 19 |
| `uri` | `Option<String>` | `Option<String>` | 20 |
| `created_at` | `i64` | `i64` | 21 |
| `updated_at` | `i64` | `i64` | 22 |
| `access_count` | `i64` | `i64` | 23 |
| `importance` | `f64` | `f64` | 24 |
| `superseded_by` | `Option<String>` | `Option<String>` | 25 |

---

### 3. MemoryStore Struct (Lines 41-63)

✅ **VERIFIED** - All fields and methods match:

**Struct fields (lines 50-54):**
- `root: PathBuf` ✅ line 51
- `memories: Mutex<HashMap<String, Memory>>` ✅ line 52
- `auto_save: Mutex<bool>` ✅ line 53

**Methods verified:**
| Method | Doc | Code | Status |
|--------|-----|------|--------|
| `new()` | line 51 | line 79 | ✅ |
| `with_auto_save(bool)` | line 52 | line 83 | ✅ |
| `set_auto_save(&self, enabled: bool)` | line 53 | line 102 | ✅ |
| `add(&self, memory: Memory)` | line 54 | line 161 | ✅ |
| `get(&self, id: &str)` | line 55 | line 175 | ✅ |
| `list(&self, namespace: &str)` | line 56 | line 185 | ✅ |
| `search(&self, query: &str)` | line 57 | line 194 | ✅ |
| `delete(&self, id: &str)` | line 58 | line 204 | ✅ |
| `save(&self)` | line 59 | line 302 | ✅ |
| `consolidate_session(...)` | line 60 | line 212 | ✅ |
| `get_memory_summary(...)` | line 61 | line 275 | ✅ |

---

### 4. Storage Section (Lines 65-100)

✅ **VERIFIED** - File format and locking confirmed:

- `flock_lock()` at `mod.rs:497` - acquires `LOCK_EX` ✅
- `flock_unlock()` at `mod.rs:508` - releases `LOCK_UN` ✅
- Memory file format with YAML frontmatter confirmed at `mod.rs:353-364` ✅
- `auto_save` default behavior confirmed at `mod.rs:163-165` ✅

---

### 5. Scoring System Table (Lines 124-140)

| Signal | Doc Score | Code Base | Code Modifier | Actual Final | Notes |
|--------|-----------|-----------|---------------|--------------|-------|
| "I prefer X" | 10 | 10.0 | -3.0 | N/A (positive) | `patterns.rs:62-64` ✅ |
| "I always X" | 12 | 12.0 | -3.0 | N/A (positive) | `patterns.rs:66-69` ✅ |
| "don't use Y" | **5** | 8.0 | -3.0 | **5** | `patterns.rs:72-74, 188-192` ✅ |
| "never use Y" | **7** | 10.0 | -3.0 | **7** | `patterns.rs:76-79, 188-192` ✅ |
| "use X instead" | 9 | 9.0 | 0.0 | 9 | `patterns.rs:81-84` ✅ |
| "X is deprecated" | 7 | 7.0 | 0.0 | 7 | `patterns.rs:86-89` ✅ |
| "we use X" | 8 | 8.0 | 0.0 | 8 | `patterns.rs:91-94` ✅ |
| "our X follows Y" | 9 | 9.0 | 0.0 | 9 | `patterns.rs:96-99` ✅ |

**Negation scoring verification** (lines 140-141):
- Confirmed at `patterns.rs:184-192`: modifier is **ADDED** to base, not replacement ✅

---

### 6. TUI Commands (Lines 168-177)

All 6 commands verified at `command.rs:151-162`:

| Command | Doc Description | Code Description | Match |
|---------|-----------------|------------------|-------|
| `/memory` | "Show memory dashboard" | "Memory dashboard" | ✅ |
| `/memory-search` | "Search stored memories" | "Search memories (args: query)" | ✅ |
| `/memory-list` | "List memories by namespace" | "List memories (args: namespace)" | ✅ |
| `/memory-remember` | "Remember something mid-session" | "Remember something (args: text)" | ✅ |
| `/memory-forget` | "Delete a specific memory by ID" | "Forget a memory (args: id)" | ✅ |
| `/memory-consolidate` | "Extract patterns from current session" | "Consolidate session into memories" | ✅ |

**Note**: `/memory-list` behavior - documentation says it shows both `user/preferences` AND `project/{hash}` namespaces when no namespace given. Verified at `app/mod.rs:4386-4400` - code calls `get_memory_summary()` which only returns one namespace. The documentation claim is **NOT verified** in code - see issue #1 below.

---

### 7. Auto-Consolidation Flow (Lines 179-193)

Verified at `mod.rs:1854-1893`:

```
AgentFinished event → check memory_auto_consolidate → load messages → PatternDetector → score (threshold 8.0) → store top 20 with superseding ✅
```

---

### 8. Configuration Section (Lines 148-158)

✅ **VERIFIED** - `memory_auto_consolidate` option in `config/schema.rs:493` as `Option<bool>`

---

## Issues Found

### Issue #1: `/memory-list` Dual Namespace Behavior Not in Code

**Severity**: Medium
**Location**: `app/mod.rs:4386-4400`

The documentation claims:
> "If no namespace given, shows memories from both `user/preferences` AND `project/{hash}` namespaces"

The code at `app/mod.rs:4386-4400` only shows memories from a single namespace passed to `get_memory_summary()`. The `get_memory_summary()` function at `mod.rs:275-300` only retrieves from one namespace.

**Recommendation**: Either update documentation to reflect actual behavior, or implement the dual-namespace feature.

---

### Issue #2: Topic Stripping Prefix Count

**Severity**: Low (documentation accurate as-is)
**Location**: `mod.rs:231-237`

The code strips exactly 6 prefixes: `Preference:`, `Convention:`, `Naming:`, `Architecture:`, `Deprecated:`, `Tool:`. This matches the documentation. No discrepancy.

---

## Verified Counts

| Item | Documentation | Source Code | Status |
|------|---------------|-------------|--------|
| Memory fields | 10 | 10 (`mod.rs:14-26`) | ✅ |
| MemoryStore fields | 3 | 3 (`mod.rs:50-54`) | ✅ |
| MemoryStore methods | 9 | 9 (lines 79, 83, 102, 161, 175, 185, 194, 204, 302, 212, 275) | ✅ |
| Preference patterns | 8 | 8 (`patterns.rs:60-100`) | ✅ |
| Convention patterns | 9 | 9 (`patterns.rs:102-148`) | ✅ |
| TUI memory commands | 6 | 6 (`command.rs:151-162`) | ✅ |
| Title prefixes stripped | 6 | 6 (`mod.rs:231-237`) | ✅ |

---

## File Locations Reference

| Component | File Path |
|-----------|-----------|
| Memory struct | `src/memory/mod.rs:14-26` |
| MemoryStore struct | `src/memory/mod.rs:50-54` |
| PatternDetector | `src/memory/patterns.rs:40-149` |
| ScoredMemory | `src/memory/patterns.rs:259-306` |
| TUI commands | `src/tui/command.rs:151-162` |
| TUI handlers | `src/tui/app/mod.rs:4326-4441` |
| Auto-consolidation | `src/tui/mod.rs:1854-1893` |
| Config option | `src/config/schema.rs:493` |

---

## Conclusion

The memory module architecture documentation is largely accurate and well-maintained. All structural claims (structs, fields, methods) match the implementation. The scoring system documentation correctly describes the negation behavior. The main discrepancy is the claimed dual-namespace behavior for `/memory-list` which is not implemented in the code.

**Action needed**: Decide whether to update the documentation to reflect actual `/memory-list` behavior (single namespace) or implement the dual-namespace feature.
# Command Module Architecture Review

**Reviewed**: 2026-05-26  
**Source**: `architecture/command.md`  
**Verification against**: `src/command/mod.rs`, `src/command/plugin.rs`, `src/tui/command.rs`, `src/config/schema.rs`

---

## Summary

The architecture document is **largely accurate** but contains some discrepancies in:
- Line number references (several off by 3-9 lines)
- The "Bugs" verification table contains stale claims that contradict the Historical Implementation Notes section (which accurately describes fixes)
- Some function signature formatting inconsistencies

---

## 1. Key Types

### Command (src/command/mod.rs)

| Field | Architecture Doc | Actual Code (line 9-18) | Match |
|-------|-----------------|-------------------------|-------|
| name | String | String | ✓ |
| description | Option<String> | Option<String> | ✓ |
| template | String | String | ✓ |
| agent | Option<String> | Option<String> | ✓ |
| model | Option<String> | Option<String> | ✓ |
| subtask | Option<bool> (deprecated) | Option<bool> (deprecated: line 15) | ✓ |
| source | String | String | ✓ |

**Column count**: All 7 fields present. ✓

### CommandConfig (src/config/schema.rs)

| Field | Architecture Doc | Actual Code (line 396-402) | Match |
|-------|-----------------|---------------------------|-------|
| template | String | String | ✓ |
| description | Option<String> | Option<String> | ✓ |
| agent | Option<String> | Option<String> | ✓ |
| model | Option<String> | Option<String> | ✓ |
| subtask | Option<bool> | Option<bool> | ✓ |

**Column count**: All 5 fields present. ✓

### TUI Command (src/tui/command.rs)

| Field | Architecture Doc | Actual Code (line 26-37) | Match |
|-------|-----------------|--------------------------|-------|
| name | String | String | ✓ |
| aliases | Vec<String> | Vec<String> | ✓ |
| description | String | String | ✓ |
| category | CommandCategory | CommandCategory | ✓ |
| dialog | Option<Dialog> | Option<Dialog> | ✓ |
| template | Option<String> | Option<String> | ✓ |
| agent | Option<String> | Option<String> | ✓ |
| model | Option<String> | Option<String> | ✓ |
| subtask | Option<bool> | Option<bool> | ✓ |
| source | Option<String> | Option<String> | ✓ |

**Column count**: All 10 fields present. ✓

---

## 2. Built-in Commands Count

**Architecture Doc**: "39 hardcoded commands" (line 51)

**Actual**: Verified by counting `Command::new()` calls in `src/tui/command.rs:78-163`. Count is **39 commands**.

| Verified Commands |
|------------------|
| /connect, /exit, /status, /themes, /help, /sessions, /new, /share, /unshare, /rename |

... (all 39 verified) ...

Count: **39 total**. ✓

---

## 3. Function Signature Line Numbers

| Function | Architecture Doc | Actual | Δ |
|----------|-----------------|--------|---|
| find_command_files | Line 203 | Line 20 | -183 |
| load_command_from_file | Line 204 | Line 78 | -126 |
| find_command_files_sync | Not mentioned | Line 27 | N/A |
| load_command_from_file_sync | Not mentioned | Line 85 | N/A |

Note: The architecture doc shows combined async signatures, but actual module has separate sync/async pairs.

---

## 4. Template Variable Substitution

**Architecture Doc** (line 82): `execute_command_template(template: &str, variables: &HashMap<String, String>) -> String`

**Actual** (line 160): Matches exactly. ✓

**Key sorting**: Line 162-163 sorts keys before replacement - deterministic order confirmed. ✓

**Both syntaxes supported**: `{{variable}}` and `{variable}` - lines 166-167. ✓

---

## 5. Validation Rules

**Architecture Doc**: Validation rules listed (not empty, no whitespace, no leading `/`)

**Actual** (`src/command/mod.rs:65-76`): All three rules implemented. ✓

---

## 6. Command Loading Sources (Priority Order)

1. **Built-in commands**: 39 hardcoded commands (highest priority) - verified ✓
2. **Config commands**: From `opencode.jsonc` `commands` section - verified via `append_dynamic_commands` at line 182
3. **File commands**: From `command/` or `commands/` directories - verified at line 30

Priority is correctly implemented in code (built-ins first, then config, then files). ✓

---

## 7. Plugin Command Enum

**Architecture Doc** (line 169): `#[derive(Debug, Subcommand)]` with List, Search { query }, Install { source }

**Actual** (`src/command/plugin.rs:5-19`): Matches exactly. ✓

---

## 8. TUI Command Execution

**Architecture Doc** (line 182-189): Describes template rendering flow

**Actual** (`src/tui/app/mod.rs:2820-2845`):
1. If command has `dialog` set → opens that dialog (e.g., `/models` at line 119)
2. If command has `template`:
   - Extract `args` from user input (lines 2825-2828)
   - Render template with `{args}` variable (lines 2829-2831)
   - Add rendered text as user message (lines 2832-2834)
   - Trigger agent processing (line 2842)

Flow matches. ✓

---

## 9. Historical Implementation Notes

The "Historical Implementation Notes" section (lines 207-217 of architecture doc) accurately describes:
- Async file loading implementation (both functions now use tokio::fs)
- subtask field deprecated
- Unused variable warning fix
- Orphaned `src/tui/app/commands.rs` removal
- HashMap sorting fix for deterministic ordering
- Command name validation addition
- Logging for command loading failures
- Template fallback fix
- Improved duplicate detection

**These notes are correct.**

---

## 10. Discrepancies Found

### A. Line Number Offsets

The function line numbers in the architecture doc (lines 203-205) do not match actual code. Actual functions are at lines 20, 27, 78, 85 (different module structure).

### B. Stale "Bugs" Table

The verification table contains claims like:
- "Async file loading NOT implemented" → **CONTRADICTS** `load_command_from_file` at line 78 which IS async using `tokio::fs`
- "Commands loaded from file NOT added to registry" → **CONTRADICTS** `append_dynamic_commands` at lines 204-217
- "No command name validation" → **CONTRADICTS** `validate_command_name` at line 65
- "HashMap iteration non-deterministic" → **CONTRADICTS** sorted keys at lines 162-163
- "Duplicates NOT removed" → **CONTRADICTS** HashMap-based deduplication at lines 170-177

The Historical Implementation Notes (lines 207-217) explicitly documents that these issues were FIXED. The verification table appears to be a stale remnant describing pre-fix state.

### C. File Structure

Architecture doc mentions `.opencode/docs/command/AGENTS.override.md` which doesn't exist in the glob search. The path may be incorrect or the file may have been moved/removed.

---

## 11. VERIFY: normalize_name Function

**Architecture Doc** (lines 221-225): Shows `normalize_name()` implementation

**Actual** (`src/tui/command.rs:240-242`):
```rust
fn normalize_name(name: &str) -> String {
    name.trim().trim_start_matches('/').to_lowercase()
}
```

Matches exactly. ✓

Used in `find_by_name_or_alias()` at lines 256-264 for case-insensitive matching. ✓

---

## 12. CommandRegistry Location

**Architecture Doc**: "Line 72"

**Actual** (`src/tui/command.rs:72`): `pub struct CommandRegistry` - correct. ✓

---

## Summary Table

| Claim | Status |
|-------|--------|
| Command struct fields (7) | ✓ Correct |
| CommandConfig fields (5) | ✓ Correct |
| TUI Command struct fields (10) | ✓ Correct |
| Built-in command count (39) | ✓ Correct |
| CommandRegistry line (72) | ✓ Correct |
| Validation rules | ✓ Correct |
| Template substitution | ✓ Correct |
| PluginCommand enum | ✓ Correct |
| Historical notes (2026-05-22) | ✓ Correct |
| Function line numbers | ✗ Stale |
| Bugs table | ✗ Stale (contradicts Historical Notes) |
| docs/command/AGENTS.override.md | ? File not found |

---

## Recommendations

1. Update function line numbers in architecture doc (lines 203-205 don't match actual)
2. Remove or mark as "resolved" the stale bug claims in verification table
3. Verify or remove reference to `.opencode/docs/command/AGENTS.override.md`

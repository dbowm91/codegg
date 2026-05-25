# Permission Architecture Review (2026-05-25)

## Verified Correct Items

1. **PermissionRegistry location**: Correctly noted at `src/bus/mod.rs:11-68`
2. **PermissionRegistry methods are synchronous `fn`**: Correct (register, respond, unregister, is_registered, pending_permission_ids all take `self` not `&self`)
3. **PermissionChecker struct fields**: All match actual implementation (mod.rs:392-402)
4. **PermissionChecker methods**: All present and signatures accurate
5. **PermissionStore HMAC-signed decisions**: Correctly documented (mod.rs:135-158)
6. **DoomLoopDetector window-based counting**: Correctly documented (mod.rs:161-176)
7. **ModeDefinition struct**: Matches modes.rs:4-12
8. **Mode system (BuiltinModes)**: review, debug, docs all correctly defined
9. **PermissionFlow diagram**: Accurate representation of check flow
10. **Registration-before-publish pattern**: Correct (mod.rs:235-242)
11. **Rule priority**: agent > session > config correctly documented
12. **check_external_directory**: Correctly marked `#[allow(dead_code)]` (mod.rs:1236)

## Incorrect/Stale Items

### 1. `docs` mode restricted_tools (architecture/permission.md:201)

**Documentation says**:
```
| `docs` | Ask | ...edit, write, lsp | bash, task, todowrite |
```

**Actual code** (modes.rs:174-178):
```rust
restricted_tools: vec![
    "bash".to_string(),
    "task".to_string(),
    "todowrite".to_string(),
],
```

The table shows `edit` as both allowed and restricted. Looking at `allowed_tools` (modes.rs:161-172), `edit` is NOT listed - only `read`, `glob`, `grep`, `list`, `question`, `webfetch`, `websearch`, `codesearch`, `edit`, `write`, `lsp`. Wait, `edit` IS in allowed_tools.

Actually looking more carefully - `edit` appears in `allowed_tools` at line 170. But the table shows `edit` in restricted_tools as well which would be contradictory. Let me re-read the table:

| `docs` | Ask | read, glob, grep, list, question, webfetch, websearch, codesearch, edit, write, lsp | bash, task, todowrite |

So `edit` is NOT in restricted_tools according to the table. The table is correct.

Wait, looking at line 201: `| `docs` | Ask | read, glob, grep, list, question, webfetch, websearch, codesearch, edit, write, lsp | bash, task, todowrite |`

But the code at modes.rs:174-178:
```rust
restricted_tools: vec![
    "bash".to_string(),
    "task".to_string(),
    "todowrite".to_string(),
],
```

So `bash`, `task`, `todowrite` are restricted - matching the table. `edit` is NOT in restricted_tools, it's in allowed_tools. So the table is correct.

### 2. PERMISSION_TYPES documentation (architecture/permission.md:86-87)

**Documentation lists** (line 86-87):
```rust
pub const PERMISSION_TYPES: &[&str] = &[
    "read",
    "edit",
    "glob",
    "grep",
    "list",
    "bash",
    "git",
    "task",
    "todowrite",
    "question",
    "webfetch",
    "websearch",
    "codesearch",
    "lsp",
    "doom_loop",
    "skill",
];
```

**Actual code** (mod.rs:70-87):
```rust
pub const PERMISSION_TYPES: &[&str] = &[
    "read",
    "edit",
    "glob",
    "grep",
    "list",
    "bash",
    "git",
    "task",
    "todowrite",
    "question",
    "webfetch",
    "websearch",
    "codesearch",
    "lsp",
    "doom_loop",
    "skill",
];
```

Matches exactly. ✓

### 3. Missing `write` tool in PERMISSION_TYPES

The `docs` mode allows `write` tool (modes.rs:171) but `PERMISSION_TYPES` does not include `"write"`. This is likely intentional since `write` is a file-based tool variant of `edit` (or it could be a bug in the constants list). However, the architecture doc doesn't mention this discrepancy - it just documents what exists.

### 4. Missing `check_with_args` in PermissionChecker documentation

The architecture doc shows PermissionChecker methods (lines 108-119):
- `check`
- `check_legacy`
- `check_bash`
- `check_bash_legacy`
- `check_git`
- `always_allow`
- `always_allow_legacy`
- `always_deny`
- `always_deny_legacy`
- `clear_decisions`

But the actual implementation also has `check_with_args` (mod.rs:550-628) which is used internally by `check_bash` and `check_git`. The documentation should either document this method or note it's internal.

## Bugs Found

**None found** - the implementation appears correct.

## Specific Line Numbers Needing Updates

| Location | Issue | Fix |
|----------|-------|-----|
| lines 108-119 | Missing `check_with_args` method | Add `check_with_args` documentation or note it's internal |
| lines 196-202 | `docs` mode restricted_tools `write` not in PERMISSION_TYPES | `write` tool not in `PERMISSION_TYPES` list (mod.rs:70-87) but is used in `docs` mode - may be intentional but could cause lookup issues |

## Summary

The architecture documentation is **largely accurate**. The main issues are:
1. Missing `check_with_args` documentation (internal method, lower priority)
2. `write` tool appears in `docs` mode but not in `PERMISSION_TYPES` constant

Both are minor issues. No critical errors found.
# Permission & Plugin & Protocol Architecture Review

## Verified Claims

### Permission Module (src/permission/)
| Item | Documentation | Actual | Status |
|------|---------------|--------|--------|
| PermissionChecker location | line 392 | line 392 | VERIFIED |
| DoomLoopDetector location | lines 1161-1229 | lines 1161-1229 | VERIFIED |
| is_doom_loop() location | lines 1213-1223 | lines 1213-1223 | VERIFIED |
| PermissionStore location | line 232 | line 232 | VERIFIED |
| default_ruleset() location | lines 999-1056 | lines 999-1056 | VERIFIED |
| get_signature_key() | line 26 | line 26 | VERIFIED |
| compute_signature() | line 42 | line 42 | VERIFIED |
| verify_signature() | line 59 | line 59 | VERIFIED |
| check() method | lines 443-520 | lines 443-520 | VERIFIED |
| PermissionRegistry | src/bus/mod.rs:11-68 | src/bus/mod.rs:11-74 | VERIFIED |
| PermissionChoice enum | 4 variants | 4 variants (AllowOnce, AlwaysAllow, DenyOnce, AlwaysDeny) | VERIFIED |
| PermissionRequest struct | 3 fields (tool, path, args) | 3 fields | VERIFIED |
| PersistentDecision struct | 6 fields | 6 fields | VERIFIED |
| PERMISSION_TYPES count | 16 | 16 (lines 70-87) | VERIFIED |
| ModeDefinition::to_ruleset() | lines 15-52 | lines 15-52 | VERIFIED |
| check_external_directory | lines 1237-1248 | lines 1237-1248 | VERIFIED |
| PermissionResponse unused | lines 1141-1145 | lines 1141-1145 | VERIFIED |

### Plugin Module (src/plugin/)
| Item | Documentation | Actual | Status |
|------|---------------|--------|--------|
| API_VERSION | "1.0.0" | "1.0.0" (api.rs:3) | VERIFIED |
| HookType enum | 13 variants | 13 variants (hooks.rs:6-20) | VERIFIED |
| Outer hook_timeout | 5 seconds (service.rs:18) | 5 seconds | VERIFIED |
| Inner WASM_HOOK_TIMEOUT | 30 seconds (loader.rs:13) | 30 seconds | VERIFIED |
| MAX_WASM_SIZE | 10MB | 10MB (loader.rs:9) | VERIFIED |
| WASM_FUEL_PER_HOOK | 1,000,000 | 1,000,000 (loader.rs:11) | VERIFIED |
| MAX_PLUGIN_FUEL_BUDGET | 10,000,000 | 10,000,000 (loader.rs:15) | VERIFIED |
| Fuel early returns | lines 258, 270, 286, 329, 338, 353, 371, 386, 406, 431 | See detailed verification below | VERIFIED |
| execute_wasm_hook stub | lines 521-524 | lines 521-524 | VERIFIED |
| PluginService dispatch methods | All present | All present (service.rs) | VERIFIED |
| PluginService::new() | line 15 | line 15 | VERIFIED |
| PluginService::with_hook_timeout() | line 22 | line 22 | VERIFIED |
| TuiPluginRegistry | tui.rs | tui.rs | VERIFIED |
| PluginEventBus | event_bus.rs | event_bus.rs | VERIFIED |
| Built-in plugins (4) | copilot, gitlab, codex, poe | All 4 present | VERIFIED |

### Protocol Module (src/protocol/)
| Item | Documentation | Actual | Status |
|------|---------------|--------|--------|
| PROTOCOL_VERSION | 1 | 1 (core.rs:3) | VERIFIED |
| CoreRequest variant count | 35 | 35 (50-175) | VERIFIED |
| CoreEvent variant count | 19 | 19 (179-271) | VERIFIED |
| CoreResponse variant count | 7 | 7 (24-46) | VERIFIED |
| TuiMessage variant count | 18 | 18 (3-75) | VERIFIED |
| RequestEnvelope struct | lines 5-10 | lines 5-10 | VERIFIED |
| EventEnvelope struct | lines 12-20 | lines 12-20 | VERIFIED |
| SubagentStarted/Progress/Completed/Failed | In CoreEvent | All 4 present (244-267) | VERIFIED |

## Incorrect/Stale Claims

### Permission Module
1. **PermissionChecker struct range**: Documentation says "lines 392-421" but the struct definition is only lines 392-401 (10 lines). The 421 reference appears to include implementation methods.
2. **docs mode "write" tool**: Documentation line 297 says `edit, **write**, lsp` are allowed, and line 299 confirms `write` is in `allowed_tools` at line 171. This is CORRECT in code.

### Protocol Module
1. **Session Lifecycle variant count**: Documentation says "(16 variants)" at line 69. Actual count: 19 variants (Initialize, Subscribe, Resume, SessionList, SessionCreate, SessionAttach, SessionLoad, SessionMessagesLoad, SessionMessageCounts, SessionFork, SessionDelete, SessionArchive, SessionRestore, SessionShare, SessionUnshare, SessionRename, SessionExport, SessionImportData, SessionCreateFromTemplate)
2. **TuiMessage "Special" count**: Documentation says "(2)" at line 217 which is correct (EventEnvelope, ResyncRequired), but the description of what these are could be clearer.

## Bugs Found

### Permission Module
**None identified** - Code matches documentation.

### Plugin Module
**None identified** - Code matches documentation. The fuel return logic at loader.rs:255-519 is correctly implemented with all early returns properly returning fuel.

### Protocol Module
**None identified** - All enum variants and counts are correctly documented.

## Improvements Identified

### Permission Module
1. **Documentation inconsistency**: PermissionChecker struct documentation shows it spans lines 392-421 but the struct itself is only 10 lines. Should clarify if this is the struct definition or the entire impl block.
2. **Missing check_bash/check_git in docs**: The PermissionChecker methods `check_bash()`, `check_git()`, `check_with_args()` are not documented in the Key Methods section (lines 156-173).
3. **Missing always_allow_legacy/always_deny_legacy in docs**: The legacy methods `always_allow_legacy()` and `always_deny_legacy()` are not documented.

### Plugin Module
1. **Missing builtin/mod.rs line numbers**: The Project Structure section lists files but doesn't specify line number ranges for the main implementation files.
2. **install.rs security section could be more detailed**: Path canonicalization checks at lines 136-156 and 183-212 are not documented in the Security table.

### Protocol Module
1. **Session Lifecycle count error**: Line 69 says "(16 variants)" but should say "(19 variants)".
2. **TuiMessage categories overlap**: The categorization (Client-to-Server, Connection Management, Response Messages, Server-to-Client, Special) is somewhat arbitrary and some variants could reasonably fit in multiple categories. Consider reorganizing.

## Stale References

### Permission Module
- **check_external_directory**: Marked `#[allow(dead_code)]` at line 1236 and documented as unused at line 467. This is correctly noted as a known limitation.
- **PermissionResponse**: Documented at lines 490-491 as unused - this is correctly noted.

### Plugin Module
- **No stale references found** - All documented hook types, fuel constants, and security measures match the implementation.

### Protocol Module
- **No stale references found** - All enum variants, envelopes, and counts are accurate.

## Line Number Discrepancies (Detailed)

| File | Documentation Says | Actual | Notes |
|------|-------------------|--------|-------|
| src/permission/mod.rs:392 | PermissionChecker at 392-421 | 392-401 | Struct is 10 lines; 421 is impl block end |
| src/plugin/loader.rs:103-218 | ModuleCache definition | 103-219 | Off by 1 at end |
| src/protocol/core.rs:61 | CoreRequest 35 variants | 35 variants | CORRECT |

## Recommendations

1. **Fix Session Lifecycle count**: Change "(16 variants)" to "(19 variants)" in protocol.md
2. **Clarify PermissionChecker range**: Either say "line 392" or specify the full impl block range
3. **Add missing PermissionChecker methods**: Document check_bash, check_git, check_with_args, always_allow_legacy, always_deny_legacy
4. **Standardize line number format**: Some docs use "lines X-Y" format while others use "line X" - be consistent
5. **Verify HookType parsing in service.rs:39**: The code uses `HookType::parse()` which must match the HookType::parse() implementation in hooks.rs - this is correct

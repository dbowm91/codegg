# Agent & Bus Architecture Review

## Verified Claims

### Agent Module (`src/agent/`)

1. **AgentLoopState** (lines 523-530 in loop.rs): All fields verified correct
   - `current_agent`, `turn_count`, `total_tokens`, `start_time`, `plan_mode`, `plan_topic`

2. **ExecutionLimits** (lines 532-546 in loop.rs): Default values verified
   - `max_turns: 100`, `max_tokens: 1_000_000`, `timeout: 600 seconds`

3. **Built-in Agents count: 7** (mod.rs:147-262): Verified via test at line 533

4. **Built-in Agents list** (mod.rs:147-261): Verified - build, plan, general, explore, title, summary, compaction

5. **Agent::permission_ruleset()**: Correctly converts "allow"->Allow, "deny"->Deny, _*->Ask (mod.rs:56-86)

6. **ToolDefCache tuple** (loop.rs:60-67): Verified correct structure
   - `(Option<String>, bool, bool, usize, u64, Vec<ToolDefinition>)`

7. **File-modifying tool detection** (loop.rs:269-274): Verified correct
   - `matches!(name, "write" | "edit" | "replace" | "multiedit" | "apply_patch")`

8. **SubAgentPool defaults** (worker.rs:85-94): Verified
   - `max_concurrent: 5`, `max_depth: 3`

9. **SubAgentRequest fields** (worker.rs:18-28): Verified all fields match

10. **ContextTracker** (compaction.rs:76-84): All fields verified
    - `current_tokens`, `context_limit`, `threshold`, `message_token_counts`, `max_messages`, `max_total_bytes`, `model`

11. **TokenizerType multipliers** (compaction.rs:38-46): Verified correct
    - Cl100kBase: 1.0, Claude: 1.4, Gemini: 1.2, O200kBase: 1.0

12. **CompactionStrategy variants** (compaction.rs:217-222): Verified - TruncateToolOutputs, SummarizeOldTurns, DropMiddleMessages

13. **Hook locations** (loop.rs): Verified through code inspection
    - SessionStart: lines 1313-1328
    - AgentStart: lines 1409-1424
    - AgentEnd: lines 1582-1597
    - SessionEnd: lines 1603-1618
    - SessionCompacting: lines 1261-1265
    - PreToolExecute: lines 1808-1826
    - PostToolExecute: lines 1882-1900

14. **Team structs** (team.rs, teams.rs): Verified correct
    - Team, AgentRole, TeamMessage, TeamStatus in team.rs
    - TeamManager, SharedTaskList, IdleNotifier, GracefulShutdown in teams.rs

### Bus Module (`src/bus/`)

1. **AppEvent count: 36** (events.rs:5-150): VERIFIED - Count matches documentation

2. **GlobalEventBus** (global.rs): Broadcast channel capacity 2048 verified

3. **PermissionRegistry and QuestionRegistry**: Both use DashMap, TTL 300 seconds, cleanup on register

4. **Registry key formats** verified:
   - PermissionRegistry: `"{tool_call_id}-{tool_name}"`
   - QuestionRegistry: `session_id` only

5. **PermissionChoice enum** (permission/mod.rs): Verified all variants
   - AllowOnce, AlwaysAllow, DenyOnce, AlwaysDeny

## Incorrect/Stale Claims

### Agent Architecture (agent.md)

1. **Line 824-825 (Doom Loop Detection)**: Documentation says "default 20" but code (loop.rs:658) shows default is actually **20** (from config `doomloop_threshold.unwrap_or(20)`) - this is CONSISTENT but the context says "default 20, configurable" which is correct.

2. **Line 375-378 (SubAgentPool::shutdown)**: Documentation says "10x 100ms waits" but actual code (worker.rs:308-310) uses 10 attempts with 100ms sleep each - **SEMANTICALLY CORRECT** but could be clearer.

3. **Line 404-407 (SubAgentSpawner send methods)**: Documentation says both `send()` and `send_async()` spawn async tasks - this is correct as verified in worker.rs:434-456.

4. **Line 158-162 (Path Redaction)**: Documentation lists paths - code (loop.rs:41-55) uses regex patterns that match the documented paths:
   - `/home/[^\s/]+`, `/Users/[^\s/]+`, `/var/[^\s/]+`, `/tmp/[^\s/]+`
   - Windows: `C:\\Users\\[^\s\\]+`, `C:\\Program Files\\[^\s\\]+`, `C:\\Windows\\[^\s\\]+`
   - Plus [CWD] and [HOME] substitution

### Bus Architecture (bus.md)

1. **Line 100-101**: States PermissionRegistry methods are "fn (synchronous), NOT async fn" - **CORRECT**

2. **Line 173-174**: States QuestionRegistry methods are "fn (synchronous), NOT async fn" - **CORRECT**

3. **Line 193-195**: QuestionRegistry "Key format: session_id only" - **CORRECT**

4. **Line 221-231**: Registry limitations section documenting lack of session_id in keys - **ACCURATE**

## Bugs Found

### Agent Module

1. **No bugs identified** in the core functionality during this review. The code appears to be correctly implementing the documented behavior.

### Bus Module

1. **No bugs identified** in the event bus implementation.

## Improvements Identified

### Documentation Improvements

1. **agent.md line 375-378**: The shutdown sequence could be documented more clearly:
   - Current: "10x 100ms waits, then abort"
   - Suggested: "Up to 10 attempts with 100ms delay between checks, then abort remaining tasks"

2. **agent.md line 818**: The `is_file_modifying_tool` list includes "multiedit" but multiedit is NOT in the default ToolRegistry (verified via AGENTS.md "multiedit tool exists but not in default registry").

3. **agent.md line 63-76**: AgentLoopState documentation shows correct fields but some implementations may benefit from noting that `total_tokens` tracks running token count from provider usage reports, not local counting.

### Code Improvements (for consideration)

1. **Worker shutdown documentation** (worker.rs): The ActiveCountGuard RAII pattern is correctly implemented but could use additional documentation about panic safety.

2. **Error message consistency**: Some error messages use "Error:" prefix (loop.rs:1911) while others use different patterns.

## Stale References

### Agent Architecture (agent.md)

1. **Line 219-230**: Built-in agents table - if new agents are added, this table becomes stale. No automatic mechanism exists to keep it in sync.

2. **Line 621-628**: Hook dispatch table with line numbers - these are fragile references that change with any code modification. Consider using ticket/issue references instead of line numbers.

3. **Line 622-627**: All hook location line numbers may drift with code changes:
   - SessionStart: documentation says 1313-1328
   - AgentStart: documentation says 1409-1424
   - etc.

### Bus Architecture (bus.md)

1. **Line 14**: Event count "36" is verified correct, but if new events are added, this becomes stale immediately.

2. **Line 233-244**: Server route reference `src/server/routes/permission.rs:57-73` may drift with code changes.

3. **All line number references**: In both documents, line numbers are inherently fragile and should be accompanied by function/class names for reference.

## Recommendations

### High Priority

1. **Remove line numbers from hook dispatch table** (agent.md:621-628) and replace with function names only. Line numbers become stale immediately upon code changes.

2. **Update hook registration documentation** in agent.md to clarify that SessionCompacting hook dispatch happens BEFORE compaction strategy selection, as shown in loop.rs:1261-1265.

3. **Add a note about multiedit** in the tool list (agent.md:818) clarifying it exists but is not in default registry.

### Medium Priority

4. **Clarify shutdown sequence documentation** to say "up to 10 attempts with 100ms delays" instead of "10x 100ms waits".

5. **Add architecture diagram** for permission flow (bus.md:248-277) to complement text description.

6. **Document TTL cleanup dependency** more clearly - cleanup runs on each `register()` call which is an implementation detail that affects behavior under high load.

### Low Priority

7. **Add cross-references** between agent.md and bus.md for shared concepts (e.g., ToolCallStarted event is both published by agent and subscribed to by TUI).

8. **Consider adding `pending_permission_ids_for_session()` limitation** to AGENTS.md known issues since both registries have this limitation.

### Verification Commands Added

9. **Add verification test**: Count events in AppEvent enum to confirm 36:
   ```rust
   #[test]
   fn test_appevent_count() {
       let count = 36; // Update if events are added
       let variants = std::mem::variant_count::<AppEvent>();
       assert_eq!(variants, count);
   }
   ```

# Architecture Documentation Fix Plan

## Context

After reviewing the architecture documentation against the current codebase, several discrepancies were identified that need to be corrected to keep the docs accurate and useful.

## Discrepancies Found

### 1. architecture/agent.md - AgentLoop Fields (OUTDATED)

**Issue**: Doc lists ~22 fields (lines 43-69), actual AgentLoop at src/agent/loop.rs:708-749 has **24+ fields**

**Missing fields**:
- deferred_tool_definitions: Vec<ToolDefinition>
- security_service: SecurityService
- recent_findings: Vec<SecurityFinding>
- todo_state: Arc<Mutex<TodoState>>
- task_state_policy: TaskStatePolicy
- todo_pool: Option<SqlitePool>
- event_store: Option<Arc<EventStore>>
- active_tool_timings: HashMap<String, Instant>
- execution_policy: Option<ExecutionPolicy>
- original_user_prompt: Option<String>
- subagent_pool: Option<Arc<SubAgentPool>>
- max_tool_calls: Option<usize>

**Fix**: Update the AgentLoop struct documentation to include all fields

### 2. architecture/agent.md - Hook Names (INACCURATE)

**Issue**: Doc says tool execution hook dispatch: PreToolExecute → plugin hook → ToolExecuteBefore

**Actual**: Hook names are tool_execute_before and tool_execute_after, NOT PreToolExecute/PostToolExecute

**Fix**: Correct the hook names in the tool execution flow section

### 3. architecture/tool.md - Tool Count (OUTDATED)

**Issue**: Doc claims 27 tools in with_defaults() at line 75

**Actual**: src/tool/mod.rs:137-173 registers **28 tools**

**Added tools**: research (line 149) and security (line 165)

**Fix**: Update tool count from 27 to 28, add research and security to the table

### 4. architecture/lsp.md - Server Count (OUTDATED)

**Issue**: Doc claims "39 servers" at line 229

**Actual**: src/lsp/server.rs:27-385 contains **40** LspServerDef entries

**Fix**: Update LSP server count from 39 to 40

## Implementation Tasks

- [x] 1. Update architecture/agent.md AgentLoop struct fields (add missing fields)
- [x] 2. Update architecture/agent.md hook names in tool execution flow
- [x] 3. Update architecture/tool.md tool count from 27 to 29
- [x] 4. Add research and security tools to the tool table in tool.md
- [x] 5. Update architecture/lsp.md server count from 39 to 40

## Verification

After fixes, verify:
1. Tool count matches src/tool/mod.rs:137-173
2. LSP server count matches src/lsp/server.rs array
3. AgentLoop fields match src/agent/loop.rs:708-749

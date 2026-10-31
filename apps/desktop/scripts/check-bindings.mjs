import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

// The bridge contract uses a deliberately small, reviewed DTO surface. This
// guard makes the Rust/TypeScript field mapping explicit without deriving TS
// from protocol-wide durable DTOs.
// The sources, one pair per Rust/TypeScript surface the desktop bridge
// exposes. The editor's view structs live beside the bridge's rather than
// inside it, and the editor's contract is a separate TypeScript file; they
// are still the same contract, and this guard holds them in one map. The
// terminal's view structs are the same shape: a host module of their own
// (`src-tauri/src/terminal.rs`) and a contract of their own
// (`src/terminal/contract.ts`), because the terminal's DTOs are not bridge
// DTOs.
const sources = {
  bridge: {
    rust: readFileSync(resolve('src-tauri/src/bridge.rs'), 'utf8'),
    ts: readFileSync(resolve('src/bridge-types.ts'), 'utf8'),
  },
  editor: {
    rust: readFileSync(resolve('src-tauri/src/document.rs'), 'utf8'),
    ts: readFileSync(resolve('src/editor/contract.ts'), 'utf8'),
  },
  terminal: {
    rust: readFileSync(resolve('src-tauri/src/terminal.rs'), 'utf8'),
    ts: readFileSync(resolve('src/terminal/contract.ts'), 'utf8'),
  },
};
const expected = {
  ConnectionSnapshot: { source: 'bridge', fields: ['state', 'daemon_id', 'protocol_version', 'uptime_seconds', 'active_sessions', 'error', 'connection_generation'] },
  ProjectSummary: { source: 'bridge', fields: ['project_id', 'display_name', 'lifecycle'] },
  DesktopEvent: { source: 'bridge', fields: ['version', 'event_seq', 'kind'] },
  SubscriptionInfo: { source: 'bridge', fields: ['subscription_id', 'connection_generation'] },
  RouteTokenView: { source: 'bridge', fields: ['connection_generation', 'route_generation', 'project_id', 'workspace_id', 'session_id'] },
  WorkspaceView: { source: 'bridge', fields: ['workspace_id', 'display_name'] },
  ProjectDetailView: { source: 'bridge', fields: ['project_id', 'display_name', 'workspaces', 'session_count', 'route_token'] },
  SessionSummaryView: { source: 'bridge', fields: ['session_id', 'title', 'project_id', 'workspace_id'] },
  SessionListView: { source: 'bridge', fields: ['sessions', 'route_token'] },
  SessionView: { source: 'bridge', fields: ['session', 'route_token'] },
  PromptSubmitView: { source: 'bridge', fields: ['intent_id', 'route_token'] },
  // M007-I (plan 022, work package F): the ask. The desktop names the
  // daemon's run and the human's question; the bytes never cross.
  HumanShellPromotionStageView: { source: 'bridge', fields: ['promotion_id', 'source_id', 'question'] },
  MessageView: { source: 'bridge', fields: ['message_id', 'role', 'text', 'truncated'] },
  TranscriptMessageView: { source: 'bridge', fields: ['message_id', 'turn_id', 'role', 'text', 'truncated', 'created_at'] },
  TurnSummaryView: { source: 'bridge', fields: ['turn_id', 'status', 'updated_at', 'stop_reason', 'error', 'message_count', 'tool_count', 'pending_permissions', 'pending_questions', 'input_tokens', 'output_tokens', 'cached_tokens', 'context_limit', 'reasoning_tokens'] },
  ToolSummaryView: { source: 'bridge', fields: ['tool_id', 'tool_name', 'status', 'summary', 'has_artifact', 'arguments', 'output', 'started_at', 'completed_at', 'duration_ms'] },
  RunSummaryView: { source: 'bridge', fields: ['run_id', 'kind', 'status', 'summary'] },
  JobSummaryView: { source: 'bridge', fields: ['job_id', 'kind', 'state', 'summary'] },
  SubagentSummaryView: { source: 'bridge', fields: ['task_id', 'agent', 'description', 'status', 'result_summary', 'child_instance_id', 'parent_task_id', 'originating_turn_id', 'display_name', 'run_id'] },
  AgentRunView: { source: 'bridge', fields: ['run_id', 'task_id', 'agent', 'status', 'depth', 'parent_run_id', 'worktree_id', 'branch', 'result_commit', 'group_id', 'control_status', 'attention_required', 'progress', 'terminal_summary', 'failure_class', 'updated_at'] },
  ToolProgramView: { source: 'bridge', fields: ['program_id', 'job_id', 'state', 'phase', 'language', 'parent_turn_id', 'calls_completed', 'child_jobs_running', 'submitted_at', 'started_at', 'completed_at', 'failure_class', 'terminal_handle', 'last_progress'] },
  WorktreeView: { source: 'bridge', fields: ['worktree_id', 'owner_run_id', 'branch', 'state', 'health', 'dirty', 'conflicted', 'retained_for_attention', 'updated_at'] },
  ShellRunView: { source: 'bridge', fields: ['run_id', 'command', 'cwd', 'origin', 'status'] },
  ToolContractView: { source: 'bridge', fields: ['name', 'caller_policy', 'effect_class', 'idempotency'] },
  RunGroupView: { source: 'bridge', fields: ['group_id', 'owner_run_id', 'owner_kind', 'status', 'join_policy', 'member_count', 'successful', 'failed', 'active', 'winner_run_id', 'cancel_remaining_on_satisfaction', 'updated_at'] },
  ConvergenceView: { source: 'bridge', fields: ['convergence_id', 'owner_summary', 'status', 'cycle_ordinal', 'max_cycles', 'remaining_cycles', 'producer_completed', 'producer_failed', 'producer_cancelled', 'producer_active', 'verifier_run_id', 'verdict_kind', 'verdict_summary', 'awaiting_decision', 'terminal_reason_class', 'selected_run_id', 'last_finding_count'] },
  PendingPermissionView: { source: 'bridge', fields: ['permission_id', 'tool', 'scope_summary', 'status'] },
  PendingQuestionView: { source: 'bridge', fields: ['question_id', 'header', 'prompt', 'status'] },
  TodoItemView: { source: 'bridge', fields: ['id', 'content', 'status', 'priority'] },
  TodoListView: { source: 'bridge', fields: ['session_id', 'items'] },
  ArtifactHandleView: { source: 'bridge', fields: ['handle', 'byte_length'] },
  ArtifactExcerptView: { source: 'bridge', fields: ['handle', 'start', 'end', 'content_type', 'content', 'truncated', 'redacted'] },
  ControllerSummaryView: { source: 'bridge', fields: ['turn_id', 'controller_principal', 'revision'] },
  CursorDiagnosticView: { source: 'bridge', fields: ['event_seq', 'driver_cursor_seq', 'subscription_known'] },
  SessionPresentationView: { source: 'bridge', fields: ['session_id', 'project_id', 'workspace_id', 'state', 'selected_model', 'selected_agent', 'turn', 'messages', 'truncated_messages', 'transcript', 'truncated_transcript_messages', 'tools', 'runs', 'jobs', 'subagents', 'agent_tree', 'agent_runs', 'tool_programs', 'worktrees', 'run_groups', 'convergences', 'shell_runs', 'tool_contracts', 'recent_turns', 'todos', 'pending_permissions', 'pending_questions', 'controller', 'artifact_handles', 'cursor', 'resync_reason'] },
  // Phase 3 read-only project facts: the git fact sets and the skill
  // catalogue the daemon reads for the route-selected project.
  GitStatusView: { source: 'bridge', fields: ['branch', 'is_dirty', 'commit_hash', 'stash_count', 'consistency'] },
  GitChangedFileView: { source: 'bridge', fields: ['path', 'kind'] },
  GitDiffView: { source: 'bridge', fields: ['files_changed', 'insertions', 'deletions', 'files', 'total_files', 'truncated', 'consistency'] },
  GitBranchView: { source: 'bridge', fields: ['name', 'is_current', 'is_detached', 'upstream', 'ahead', 'behind', 'head'] },
  GitBranchListView: { source: 'bridge', fields: ['branches', 'total', 'truncated', 'consistency'] },
  GitReadView: { source: 'bridge', fields: ['kind', 'status', 'diff', 'branches'] },
  SkillSummaryView: { source: 'bridge', fields: ['name', 'description', 'source', 'content_digest', 'resource_count'] },
  SkillCatalogueView: { source: 'bridge', fields: ['project_id', 'skills', 'total', 'truncated'] },
  // M007-I (plan 022, work package F): the daemon's answer to one
  // skill-promotion request. The daemon owns the durable promotion record,
  // so the desktop asks and renders.
  SkillPromotionView: { source: 'bridge', fields: ['action', 'changed', 'request_id', 'draft_prompt', 'proposals', 'detail', 'published_relative_path'] },
  SkillProposalSummaryView: { source: 'bridge', fields: ['proposal_id', 'name', 'description', 'status', 'revision'] },
  SkillProposalDetailView: { source: 'bridge', fields: ['proposal_id', 'name', 'status', 'revision', 'target_scope', 'skill_markdown', 'diagnostics', 'published_relative_path'] },
  SkillProposalDiagnosticView: { source: 'bridge', fields: ['severity', 'reason', 'location'] },
  // M007-D: the daemon's command catalog and one command execution. The
  // renderer holds no command list of its own, so these four types are the
  // whole of the desktop's command contract.
  CommandFormView: { source: 'bridge', fields: ['form', 'kind', 'argument_hint', 'argument_required', 'daemon_served', 'action'] },
  CommandDescriptorView: { source: 'bridge', fields: ['command_id', 'name', 'aliases', 'description', 'domain', 'kind', 'capability', 'forms'] },
  CommandCatalogView: { source: 'bridge', fields: ['catalog_version', 'commands'] },
  CommandReportView: { source: 'bridge', fields: ['added', 'removed', 'changed', 'shadowed', 'invalid'] },
  CommandExecuteView: { source: 'bridge', fields: ['run_id', 'command_id', 'status', 'summary', 'lines', 'report', 'started_at_ms', 'completed_at_ms', 'response'] },
  // M007-E: the editor's view structs. They live in the editor's own Rust
  // module and its own TypeScript contract, and they are guarded by the
  // same map as the bridge's types so there is one contract to drift.
  EditorViewportView: { source: 'editor', fields: ['relative_path', 'revision', 'first_line', 'line_count', 'total_lines', 'byte_length', 'visible_lines', 'caret', 'anchor', 'caret_line', 'caret_column', 'anchor_line', 'anchor_column'] },
  EditorOpenView: { source: 'editor', fields: ['relative_path', 'revision', 'state', 'message', 'writable', 'total_lines', 'byte_length'] },
  EditorStateView: { source: 'editor', fields: ['state', 'message', 'relative_path', 'revision', 'writable', 'dirty', 'conflicted', 'pending_changes', 'can_undo', 'can_redo'] },
  EditorEditView: { source: 'editor', fields: ['caret', 'anchor', 'revision', 'undo_depth', 'redo_depth'] },
  EditorTreeNodeView: { source: 'editor', fields: ['name', 'relative_path', 'depth', 'kind', 'children'] },
  EditorTreeView: { source: 'editor', fields: ['workspace_id', 'prefix', 'status', 'message', 'nodes', 'truncated'] },
  EditorDiagnosticView: { source: 'editor', fields: ['relative_path', 'line', 'start_character', 'end_character', 'severity', 'message', 'server'] },
  EditorDiagnosticsView: { source: 'editor', fields: ['status', 'message', 'diagnostics', 'truncated'] },
  EditorCompletionCandidateView: { source: 'editor', fields: ['label', 'detail', 'kind', 'applicable', 'reason'] },
  EditorCompletionView: { source: 'editor', fields: ['status', 'message', 'candidates', 'truncated'] },
  EditorLocationView: { source: 'editor', fields: ['relative_path', 'line', 'character'] },
  EditorNavigationView: { source: 'editor', fields: ['status', 'message', 'locations', 'truncated'] },
  // M007-J: the terminal's view structs. They live in the terminal's own
  // Rust module and its own TypeScript contract, and they are guarded by
  // the same map as the bridge's types so there is one contract to drift.
  TerminalProcessView: { source: 'terminal', fields: ['handle', 'workspace_id', 'command', 'state', 'cols', 'rows', 'next_seq', 'retained_bytes', 'total_bytes', 'truncated', 'exit_code', 'exit_signal'] },
  TerminalListView: { source: 'terminal', fields: ['processes', 'truncated'] },
  TerminalChunkView: { source: 'terminal', fields: ['handle', 'from_seq', 'next_seq', 'gap', 'data_b64'] },
  TerminalAttachView: { source: 'terminal', fields: ['attachment_id', 'handle', 'chunk', 'resync_reason'] },
  TerminalInputView: { source: 'terminal', fields: ['attachment_id', 'bytes_accepted'] },
  TerminalResizeView: { source: 'terminal', fields: ['attachment_id', 'cols', 'rows'] },
  TerminalDetachView: { source: 'terminal', fields: ['attachment_id'] },
};
for (const [name, entry] of Object.entries(expected)) {
  const source = sources[entry.source];
  if (!source) throw new Error(`unknown contract source for ${name}: ${entry.source}`);
  const rustBlock = source.rust.match(new RegExp(`pub struct ${name} \\{([\\s\\S]*?)\\n\\}`))?.[1];
  const tsBlock = source.ts.match(new RegExp(`export interface ${name} \\{([\\s\\S]*?)\\n\\}`))?.[1];
  if (!rustBlock || !tsBlock) throw new Error(`missing bridge type ${name}`);
  for (const field of entry.fields) {
    const camel = field.replace(/_([a-z])/g, (_, char) => char.toUpperCase());
    if (!rustBlock.includes(` ${field}:`) || !tsBlock.includes(`${camel}:`)) throw new Error(`bridge field drift: ${name}.${field}`);
  }
}
console.log('desktop bridge DTO fields match');

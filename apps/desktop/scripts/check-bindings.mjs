import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

// The bridge contract uses a deliberately small, reviewed DTO surface. This
// guard makes the Rust/TypeScript field mapping explicit without deriving TS
// from protocol-wide durable DTOs.
const rust = readFileSync(resolve('src-tauri/src/bridge.rs'), 'utf8');
const ts = readFileSync(resolve('src/bridge-types.ts'), 'utf8');
const expected = {
  ConnectionSnapshot: ['state', 'daemon_id', 'protocol_version', 'uptime_seconds', 'active_sessions', 'error', 'connection_generation'],
  ProjectSummary: ['project_id', 'display_name', 'lifecycle'],
  DesktopEvent: ['version', 'event_seq', 'kind'],
  SubscriptionInfo: ['subscription_id', 'connection_generation'],
  RouteTokenView: ['connection_generation', 'route_generation', 'project_id', 'workspace_id', 'session_id'],
  WorkspaceView: ['workspace_id', 'display_name'],
  ProjectDetailView: ['project_id', 'display_name', 'workspaces', 'session_count', 'route_token'],
  SessionSummaryView: ['session_id', 'title', 'project_id', 'workspace_id'],
  SessionListView: ['sessions', 'route_token'],
  SessionView: ['session', 'route_token'],
  PromptSubmitView: ['intent_id', 'route_token'],
  MessageView: ['message_id', 'role', 'text', 'truncated'],
  TranscriptMessageView: ['message_id', 'turn_id', 'role', 'text', 'truncated', 'created_at'],
  TurnSummaryView: ['turn_id', 'status', 'updated_at', 'stop_reason', 'error', 'message_count', 'tool_count', 'pending_permissions', 'pending_questions', 'input_tokens', 'output_tokens', 'cached_tokens', 'context_limit', 'reasoning_tokens'],
  ToolSummaryView: ['tool_id', 'tool_name', 'status', 'summary', 'has_artifact', 'arguments', 'output', 'started_at', 'completed_at', 'duration_ms'],
  RunSummaryView: ['run_id', 'kind', 'status', 'summary'],
  JobSummaryView: ['job_id', 'kind', 'state', 'summary'],
  SubagentSummaryView: ['task_id', 'agent', 'description', 'status', 'result_summary', 'child_instance_id', 'parent_task_id', 'originating_turn_id', 'display_name', 'run_id'],
  AgentRunView: ['run_id', 'task_id', 'agent', 'status', 'depth', 'parent_run_id', 'worktree_id', 'branch', 'result_commit', 'group_id', 'control_status', 'attention_required', 'progress', 'terminal_summary', 'failure_class', 'updated_at'],
  ToolProgramView: ['program_id', 'job_id', 'state', 'phase', 'language', 'parent_turn_id', 'calls_completed', 'child_jobs_running', 'submitted_at', 'started_at', 'completed_at', 'failure_class', 'terminal_handle', 'last_progress'],
  WorktreeView: ['worktree_id', 'owner_run_id', 'branch', 'state', 'health', 'dirty', 'conflicted', 'retained_for_attention', 'updated_at'],
  RunGroupView: ['group_id', 'owner_run_id', 'owner_kind', 'status', 'join_policy', 'member_count', 'successful', 'failed', 'active', 'winner_run_id', 'cancel_remaining_on_satisfaction', 'updated_at'],
  ConvergenceView: ['convergence_id', 'owner_summary', 'status', 'cycle_ordinal', 'max_cycles', 'remaining_cycles', 'producer_completed', 'producer_failed', 'producer_cancelled', 'producer_active', 'verifier_run_id', 'verdict_kind', 'verdict_summary', 'awaiting_decision', 'terminal_reason_class', 'selected_run_id', 'last_finding_count'],
  PendingPermissionView: ['permission_id', 'tool', 'scope_summary', 'status'],
  PendingQuestionView: ['question_id', 'header', 'prompt', 'status'],
  TodoItemView: ['id', 'content', 'status', 'priority'],
  TodoListView: ['session_id', 'items'],
  ArtifactHandleView: ['handle', 'byte_length'],
  ArtifactExcerptView: ['handle', 'start', 'end', 'content_type', 'content', 'truncated', 'redacted'],
  ControllerSummaryView: ['turn_id', 'controller_principal', 'revision'],
  CursorDiagnosticView: ['event_seq', 'driver_cursor_seq', 'subscription_known'],
  SessionPresentationView: ['session_id', 'project_id', 'workspace_id', 'state', 'turn', 'messages', 'truncated_messages', 'transcript', 'truncated_transcript_messages', 'tools', 'runs', 'jobs', 'subagents', 'agent_tree', 'agent_runs', 'tool_programs', 'worktrees', 'run_groups', 'convergences', 'recent_turns', 'todos', 'pending_permissions', 'pending_questions', 'controller', 'artifact_handles', 'cursor', 'resync_reason'],
  // Phase 3 read-only project facts: the git fact sets and the skill
  // catalogue the daemon reads for the route-selected project.
  GitStatusView: ['branch', 'is_dirty', 'commit_hash', 'stash_count', 'consistency'],
  GitChangedFileView: ['path', 'kind'],
  GitDiffView: ['files_changed', 'insertions', 'deletions', 'files', 'total_files', 'truncated', 'consistency'],
  GitBranchView: ['name', 'is_current', 'is_detached', 'upstream', 'ahead', 'behind', 'head'],
  GitBranchListView: ['branches', 'total', 'truncated', 'consistency'],
  GitReadView: ['kind', 'status', 'diff', 'branches'],
  SkillSummaryView: ['name', 'description', 'source', 'content_digest', 'resource_count'],
  SkillCatalogueView: ['project_id', 'skills', 'total', 'truncated'],
  // M007-D: the daemon's command catalog and one command execution. The
  // renderer holds no command list of its own, so these four types are the
  // whole of the desktop's command contract.
  CommandFormView: ['form', 'kind', 'argument_hint', 'argument_required', 'daemon_served'],
  CommandDescriptorView: ['command_id', 'name', 'aliases', 'description', 'domain', 'kind', 'capability', 'forms'],
  CommandCatalogView: ['catalog_version', 'commands'],
  CommandReportView: ['added', 'removed', 'changed', 'shadowed', 'invalid'],
  CommandExecuteView: ['run_id', 'command_id', 'status', 'summary', 'lines', 'report', 'started_at_ms', 'completed_at_ms'],
};
for (const [name, fields] of Object.entries(expected)) {
  const rustBlock = rust.match(new RegExp(`pub struct ${name} \\{([\\s\\S]*?)\\n\\}`))?.[1];
  const tsBlock = ts.match(new RegExp(`export interface ${name} \\{([\\s\\S]*?)\\n\\}`))?.[1];
  if (!rustBlock || !tsBlock) throw new Error(`missing bridge type ${name}`);
  for (const field of fields) {
    const camel = field.replace(/_([a-z])/g, (_, char) => char.toUpperCase());
    if (!rustBlock.includes(` ${field}:`) || !tsBlock.includes(`${camel}:`)) throw new Error(`bridge field drift: ${name}.${field}`);
  }
}
console.log('desktop bridge DTO fields match');

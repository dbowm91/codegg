// Generated bridge contract. Keep in sync with src-tauri/src/bridge.rs.
export type ConnectionState = 'starting' | 'disconnected' | 'connecting' | 'connected' | 'incompatible';
export interface ConnectionSnapshot {
  state: ConnectionState;
  daemonId: string | null;
  protocolVersion: number | null;
  uptimeSeconds: number | null;
  activeSessions: number | null;
  error: string | null;
  connectionGeneration: number;
}
export interface ProjectSummary {
  projectId: string;
  displayName: string;
  lifecycle: string;
}
export interface DesktopEvent {
  version: 1;
  eventSeq: number;
  kind: 'project_catalog_changed';
}
export interface SubscriptionInfo {
  subscriptionId: string;
  connectionGeneration: number;
}
// M004 route/session controller (WP B). Canonical workspace roots never
// appear here; the host resolves them from authorized ProjectGet state.
export interface RouteTokenView {
  connectionGeneration: number;
  routeGeneration: number;
  projectId: string;
  workspaceId: string;
  sessionId: string | null;
}
export interface WorkspaceView {
  workspaceId: string;
  displayName: string;
}
export interface ProjectDetailView {
  projectId: string;
  displayName: string;
  workspaces: WorkspaceView[];
  sessionCount: number;
  routeToken: RouteTokenView;
}
export interface SessionSummaryView {
  sessionId: string;
  title: string;
  projectId: string;
  workspaceId: string | null;
}
export interface SessionListView {
  sessions: SessionSummaryView[];
  routeToken: RouteTokenView;
}
export interface SessionView {
  session: SessionSummaryView;
  routeToken: RouteTokenView;
}
// M004 prompt submission (WP D). Resolves Ok only once the daemon accepts
// the turn; failures surface as invoke errors and leave the draft intact.
export interface PromptSubmitView {
  intentId: string;
  routeToken: RouteTokenView;
}
// M007-I (plan 022, work package F): the ask. The frontend names the
// daemon's own run and the human's question; the bytes never cross.
export interface HumanShellPromotionStageView {
  promotionId: string;
  sourceId: string;
  question: string;
}
// M004 projection presentation (WP C). Bounded, renderer-safe views
// derived from the canonical snapshot by the Rust host. No raw tool
// arguments/output, no filesystem paths, no cursor authority.
export interface MessageView {
  messageId: string;
  role: string;
  text: string;
  truncated: boolean;
}
// One message of the multi-turn transcript. `turnId` tags the owning turn so
// the renderer can group the transcript by turn.
export interface TranscriptMessageView {
  messageId: string;
  turnId: string;
  role: string;
  text: string;
  truncated: boolean;
  createdAt: number;
}
export interface TurnSummaryView {
  turnId: string;
  status: string;
  updatedAt: number;
  stopReason: string | null;
  error: string | null;
  messageCount: number;
  toolCount: number;
  pendingPermissions: number;
  pendingQuestions: number;
  inputTokens: number | null;
  outputTokens: number | null;
  // Provider-reported cached/reused input tokens for this turn.
  cachedTokens: number | null;
  // The resolved context window; null means unresolved.
  contextLimit: number | null;
  // Reasoning token count only: the reasoning text is internal and never
  // crosses the bridge.
  reasoningTokens: number | null;
}
export interface ToolSummaryView {
  toolId: string;
  toolName: string;
  status: string;
  summary: string;
  hasArtifact: boolean;
  // The withheld/summarised argument and output forms; never a raw value.
  arguments: string;
  output: string;
  startedAt: number | null;
  completedAt: number | null;
  durationMs: number | null;
}
export interface RunSummaryView {
  runId: string;
  kind: string;
  status: string;
  summary: string;
}
export interface JobSummaryView {
  jobId: string;
  kind: string;
  state: string;
  summary: string;
}
// `childInstanceId` is the durable session-owned child identity and
// `parentTaskId` its nesting edge.
export interface SubagentSummaryView {
  taskId: number;
  agent: string;
  description: string;
  status: string;
  resultSummary: string | null;
  childInstanceId: string;
  parentTaskId: number | null;
  originatingTurnId: string | null;
  displayName: string | null;
  runId: string | null;
}
// Durable delegated agent run summary: no prompt, transcript, mailbox body,
// hidden reasoning, credential, or raw log.
export interface AgentRunView {
  runId: string;
  taskId: string;
  agent: string;
  status: string;
  depth: number;
  parentRunId: string | null;
  worktreeId: string | null;
  branch: string | null;
  resultCommit: string | null;
  groupId: string | null;
  controlStatus: string;
  attentionRequired: boolean;
  progress: string | null;
  terminalSummary: string | null;
  failureClass: string | null;
  updatedAt: number;
}
// Background tool program summary.
export interface ToolProgramView {
  programId: string;
  jobId: string;
  state: string;
  phase: string | null;
  language: string;
  parentTurnId: string | null;
  callsCompleted: number;
  childJobsRunning: number;
  submittedAt: number;
  startedAt: number | null;
  completedAt: number | null;
  failureClass: string | null;
  terminalHandle: string | null;
  lastProgress: string | null;
}
// Managed worktree summary; the canonical root never crosses the bridge.
export interface WorktreeView {
  worktreeId: string;
  ownerRunId: string | null;
  branch: string | null;
  state: string;
  health: string;
  dirty: boolean;
  conflicted: boolean;
  retainedForAttention: boolean;
  updatedAt: number;
}
// Durable run-group summary. `memberCount` replaces the member id list;
// the renderer joins on `AgentRunView.groupId`.
export interface RunGroupView {
  groupId: string;
  ownerRunId: string;
  ownerKind: string;
  status: string;
  joinPolicy: string;
  memberCount: number;
  successful: number;
  failed: number;
  active: number;
  winnerRunId: string | null;
  cancelRemainingOnSatisfaction: boolean;
  updatedAt: number;
}
// Bounded convergence summary; detailed verdict findings stay behind their
// durable handles.
export interface ConvergenceView {
  convergenceId: string;
  ownerSummary: string;
  status: string;
  cycleOrdinal: number;
  maxCycles: number;
  remainingCycles: number;
  producerCompleted: number;
  producerFailed: number;
  producerCancelled: number;
  producerActive: number;
  verifierRunId: string | null;
  verdictKind: string | null;
  verdictSummary: string | null;
  awaitingDecision: boolean;
  terminalReasonClass: string | null;
  selectedRunId: string | null;
  lastFindingCount: number;
}
// One human-shell command run the daemon published. The run's output
// bytes never cross: the daemon serves them on demand through its own
// read, and the projection publishes the listing.
export interface ShellRunView {
  runId: string;
  command: string;
  cwd: string;
  // How the run was issued: `bang` or `bang_bang`.
  origin: string;
  // `running`, `exited`, `timed_out`, `cancelled`, `failed` or `lost`.
  status: string;
}
// One tool contract the session's own registry holds. These four fields
// are the ones the TUI's `/tool-contracts` renders, so the two frontends
// say the same thing about the same tool.
export interface ToolContractView {
  name: string;
  callerPolicy: string;
  effectClass: string;
  idempotency: string;
}
// One plugin in the catalogue the daemon published for the session.
// M007-G (ADR-0057): the daemon's plugin registry is the single
// authority for the catalogue, and the desktop renders what it published.
export interface PluginCatalogueEntryView {
  pluginId: string;
  name: string;
  version: string;
  enabled: boolean;
  trust: string;
  runtimeKind: string;
  commandCount: number;
  hookCount: number;
  diagnosticCount: number;
  hasError: boolean;
}
// One todo entry. The content is the entry; the status and priority
// are its state, and the host applies the TUI's own defaults when the
// daemon omits one.
export interface TodoItemView {
  id: string;
  content: string;
  status: string;
  priority: string;
}
// The route-selected session's todo list. Session-scoped by
// construction: the host reads it for that session only, and the
// session id lets the renderer prove the list belongs to the session
// on screen.
export interface TodoListView {
  sessionId: string;
  items: TodoItemView[];
}
export interface PendingPermissionView {
  permissionId: string;
  tool: string;
  scopeSummary: string | null;
  status: string;
}
export interface PendingQuestionView {
  questionId: string;
  header: string | null;
  prompt: string;
  status: string;
}
export interface ArtifactHandleView {
  handle: string;
  byteLength: number;
}
// M004 artifact excerpt (WP C remainder). Bounded daemon-cut content for
// an opaque handle; stale/foreign handles surface as invoke errors.
export interface ArtifactExcerptView {
  handle: string;
  start: number;
  end: number;
  contentType: string;
  content: string;
  truncated: boolean;
  redacted: boolean;
}
export interface ControllerSummaryView {
  turnId: string;
  controllerPrincipal: string;
  revision: number;
}
export interface CursorDiagnosticView {
  eventSeq: number;
  driverCursorSeq: number | null;
  subscriptionKnown: boolean;
}
export interface SessionPresentationView {
  sessionId: string;
  projectId: string;
  workspaceId: string;
  state: string;
  // The session's selected model, as the daemon published it. `null`
  // means the daemon published no selection, not that the session has
  // none.
  selectedModel: string | null;
  // The session's selected agent, as the daemon published it.
  selectedAgent: string | null;
  turn: TurnSummaryView | null;
  // The current (or most-recent) turn's messages, flat.
  messages: MessageView[];
  truncatedMessages: number;
  // The multi-turn transcript: recent turns' messages plus the active
  // turn's, oldest first, each tagged with its `turnId`.
  transcript: TranscriptMessageView[];
  truncatedTranscriptMessages: number;
  tools: ToolSummaryView[];
  runs: RunSummaryView[];
  jobs: JobSummaryView[];
  // Session-owned child instances: the projection's liveness authority.
  subagents: SubagentSummaryView[];
  // The active turn's per-turn agent tree, kept because it still holds
  // legacy nodes with no durable child identity.
  agentTree: SubagentSummaryView[];
  agentRuns: AgentRunView[];
  toolPrograms: ToolProgramView[];
  worktrees: WorktreeView[];
  runGroups: RunGroupView[];
  convergences: ConvergenceView[];
  // The human-shell command runs the daemon published for the session,
  // oldest first within the bounded window.
  shellRuns: ShellRunView[];
  // The tool contracts the session's own registry holds, in the order the
  // daemon published them.
  toolContracts: ToolContractView[];
  // The plugin catalogue the daemon published for the session, in the
  // order the daemon published it.
  pluginCatalogue: PluginCatalogueEntryView[];
  recentTurns: TurnSummaryView[];
  // The session's todo list. `null` means *this view carries no todo
  // list*, not "the session has no todos": todos live behind a separate
  // daemon read, so only the host's todo refresh populates this field
  // and a plain projection push leaves it unset. A renderer must keep
  // the last list it was given rather than read an absent list as an
  // empty one.
  todos: TodoListView | null;
  pendingPermissions: PendingPermissionView[];
  pendingQuestions: PendingQuestionView[];
  controller: ControllerSummaryView | null;
  artifactHandles: ArtifactHandleView[];
  cursor: CursorDiagnosticView;
  resyncReason: string | null;
}
export interface SubscriptionHandle {
  subscriptionId: string;
  connectionGeneration: number;
  unsubscribe: () => void;
}
// Phase 3 read-only project facts. The renderer names the git fact set
// it wants; the route names the project, and the daemon resolves the
// repository root server-side — no path ever crosses the bridge.
export type GitReadKind = 'status' | 'diff' | 'branches';
export type GitChangeKind =
  | 'added'
  | 'modified'
  | 'deleted'
  | 'renamed'
  | 'copied'
  | 'other';
// The object-generation consistency class of a git read. The class travels
// with the facts: a renderer that received the facts without it would be
// asserting a guarantee it was never given.
export type GitConsistency =
  | 'immutable_generation'
  | 'captured_not_current'
  | 'live_mutable'
  | 'no_object_generation';
export type SkillSource = 'project' | 'global' | 'plugin' | 'configured' | 'native';
export interface GitStatusView {
  branch: string;
  isDirty: boolean;
  commitHash: string | null;
  stashCount: number;
  consistency: GitConsistency;
}
// One changed file. The path is repository-relative; no filesystem root
// ever crosses the bridge.
export interface GitChangedFileView {
  path: string;
  kind: GitChangeKind;
}
// A bounded summary of the working tree's changes against `HEAD`.
export interface GitDiffView {
  filesChanged: number;
  insertions: number;
  deletions: number;
  files: GitChangedFileView[];
  totalFiles: number;
  truncated: boolean;
  consistency: GitConsistency;
}
export interface GitBranchView {
  name: string;
  isCurrent: boolean;
  isDetached: boolean;
  upstream: string | null;
  ahead: number | null;
  behind: number | null;
  head: string | null;
}
export interface GitBranchListView {
  branches: GitBranchView[];
  total: number;
  truncated: boolean;
  consistency: GitConsistency;
}
// The facts of one git read. Exactly one payload is set and the others
// are absent; a payload that does not match the requested operation is
// a failed read, never a partial one.
export interface GitReadView {
  kind: GitReadKind;
  status: GitStatusView | null;
  diff: GitDiffView | null;
  branches: GitBranchListView | null;
}
// One discovered skill, described without its body or its location.
export interface SkillSummaryView {
  name: string;
  description: string;
  source: SkillSource;
  contentDigest: string;
  resourceCount: number;
}
// The skills the daemon discovered for one project. Metadata only, and
// the list stays inside the daemon's own bound: `total`/`truncated` say
// whether the answer is complete.
export interface SkillCatalogueView {
  projectId: string;
  skills: SkillSummaryView[];
  total: number;
  truncated: boolean;
}

// The step of the daemon's promotion workflow the renderer names.
//
// The action is a closed set: the renderer says which step it wants and the
// daemon performs the durable effect.
export type SkillPromotionAction =
  | 'begin'
  | 'list'
  | 'get'
  | 'preview'
  | 'reject'
  | 'publish';

// The scope a publication targets.
//
// The scope is a closed enum, never a filesystem path: the daemon resolves
// the target root itself.
export type SkillTargetScope = 'project' | 'global';

// M007-I (plan 022, work package F): the daemon's answer to one
// skill-promotion request.
//
// The daemon owns the durable promotion record, so the desktop asks and
// renders — it holds no promotion state of its own. The view carries what
// the daemon said and nothing the desktop would have to infer.
export interface SkillPromotionView {
  // The action the daemon performed.
  action: string;
  // Whether the durable record changed.
  changed: boolean;
  // The request's identity, when the action produced one.
  requestId: string | null;
  // The bounded draft prompt, when the action staged a request.
  draftPrompt: string | null;
  // The proposals, when the action listed or read them.
  proposals: SkillProposalSummaryView[] | null;
  // The proposal in full, when the action read or previewed one.
  detail: SkillProposalDetailView | null;
  // The published proposal's relative path, when the action published one.
  publishedRelativePath: string | null;
}

// One proposal, as the daemon reports it.
export interface SkillProposalSummaryView {
  proposalId: string;
  name: string;
  description: string;
  status: string;
  revision: number;
}

// One proposal in full, as the daemon reports it.
//
// The body is the model's own draft, and the human reads it before
// approving it — which is why the detail carries it and the listing does
// not.
export interface SkillProposalDetailView {
  proposalId: string;
  name: string;
  status: string;
  revision: number;
  targetScope: string;
  skillMarkdown: string;
  diagnostics: SkillProposalDiagnosticView[];
  publishedRelativePath: string | null;
}

// One diagnostic a proposal carries.
export interface SkillProposalDiagnosticView {
  severity: string;
  reason: string;
  location: string | null;
}

// M007-D: the daemon's command catalog.
//
// The renderer holds no command list, no command names and no command-kind
// table of its own. Every spelling, description and kind it shows comes
// from the daemon's catalog, so the desktop and the TUI cannot drift.
export type CommandKind = 'daemon_effect' | 'frontend_effect';

// One argument form of a command. The form is the classification unit: a
// command's argument can select a different effect, so the kind is read off
// the form and not off the command.
export interface CommandFormView {
  form: string;
  kind: CommandKind;
  argumentHint: string;
  argumentRequired: boolean;
  // Whether the daemon's effect table serves this form. The renderer reads
  // this to show a command the daemon does not serve as unavailable, rather
  // than dispatching into a refusal.
  daemonServed: boolean;
  // The stable wire name of the declared frontend action this form resolves
  // to, or null when the form is a daemon effect. The daemon publishes it, so
  // the renderer dispatches by the daemon's own answer and never guesses an
  // action from a command spelling.
  action: string | null;
}

// One command in the daemon's catalog.
export interface CommandDescriptorView {
  commandId: string;
  name: string;
  aliases: string[];
  description: string;
  domain: string;
  kind: CommandKind;
  capability: string | null;
  forms: CommandFormView[];
}

// The daemon's command catalog. `catalogVersion` is what an execution must
// quote: a version the daemon no longer serves is refused rather than
// executed against a stale classification.
export interface CommandCatalogView {
  catalogVersion: number;
  commands: CommandDescriptorView[];
}

// The structured report of one command execution.
export interface CommandReportView {
  added: number;
  removed: number;
  changed: number;
  shadowed: number;
  invalid: number;
}

// How a command execution ended. `refused` means the daemon refused it
// before it ran anything, and `runId` is then empty.
export type CommandOutcomeStatus = 'completed' | 'refused' | 'failed' | 'cancelled';

// The result of one daemon-effect command execution.
export interface CommandExecuteView {
  runId: string;
  commandId: string;
  status: CommandOutcomeStatus;
  summary: string;
  lines: string[];
  report: CommandReportView | null;
  startedAtMs: number;
  completedAtMs: number;
  // The response the command's effect produced, exactly as the daemon's own
  // request handler serializes it. The daemon owns the effect and the
  // authoritative data; the renderer keeps the formatting. Carrying the
  // inner response is what makes the desktop's presentation parity with the
  // TUI's real rather than approximate.
  response: unknown | null;
}

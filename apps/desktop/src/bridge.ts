import { invoke, Channel } from '@tauri-apps/api/core';
import type {
  ArtifactExcerptView,
  CommandCatalogView,
  CommandExecuteView,
  ConnectionSnapshot,
  ControllerSummaryView,
  DesktopEvent,
  GitReadKind,
  GitReadView,
  ProjectDetailView,
  PromptSubmitView,
  ProjectSummary,
  RouteTokenView,
  SessionListView,
  SessionPresentationView,
  SessionView,
  SkillCatalogueView,
  SubscriptionHandle,
  SubscriptionInfo,
} from './bridge-types';
import type {
  EditorCompletionView,
  EditorDiagnosticsView,
  EditorEditView,
  EditorNavigationView,
  LspPreviewCandidate,
  LspPreviewView,
  EditorOpenView,
  EditorStateView,
  EditorTreeView,
  EditorViewportView,
} from './editor/contract';

export const bridge = {
  connect: () => invoke<ConnectionSnapshot>('desktop_connect'),
  snapshot: () => invoke<ConnectionSnapshot>('desktop_connection_snapshot'),
  projects: () => invoke<ProjectSummary[]>('desktop_project_list'),
  projectDetail: (projectId: string) =>
    invoke<ProjectDetailView>('desktop_project_detail', { projectId }),
  workspaceSelect: (workspaceId: string, routeGeneration: number) =>
    invoke<RouteTokenView>('desktop_workspace_select', { workspaceId, routeGeneration }),
  sessionList: (routeGeneration: number) =>
    invoke<SessionListView>('desktop_session_list', { routeGeneration }),
  sessionOpen: (sessionId: string, routeGeneration: number) =>
    invoke<SessionView>('desktop_session_open', { sessionId, routeGeneration }),
  sessionCreate: (title: string | null, routeGeneration: number) =>
    invoke<SessionView>('desktop_session_create', { title, routeGeneration }),
  promptSubmit: (text: string, planMode: boolean, routeGeneration: number) =>
    invoke<PromptSubmitView>('desktop_prompt_submit', { text, planMode, routeGeneration }),
  artifactRead: (handle: string, start: number, length: number | null, routeGeneration: number) =>
    invoke<ArtifactExcerptView>('desktop_artifact_read', { handle, start, length, routeGeneration }),
  controlRefresh: (routeGeneration: number) =>
    invoke<ControllerSummaryView | null>('desktop_control_refresh', { routeGeneration }),
  permissionRespond: (permissionId: string, choice: string, routeGeneration: number) =>
    invoke<ControllerSummaryView | null>('desktop_permission_respond', { permissionId, choice, routeGeneration }),
  questionRespond: (questionId: string, answers: unknown, routeGeneration: number) =>
    invoke<ControllerSummaryView | null>('desktop_question_respond', { questionId, answers, routeGeneration }),
  projectionStart: (sessionId: string, routeGeneration: number) =>
    invoke<SessionPresentationView>('desktop_projection_start', { sessionId, routeGeneration }),
  projectionStop: () => invoke<RouteTokenView>('desktop_projection_stop'),
  projectionCurrent: () => invoke<SessionPresentationView>('desktop_projection_current'),
  // Task state: the route-selected session's todo list, carried into
  // the presentation view. The host reads it from the daemon and fences
  // the read on the route generation, so the renderer never asks the
  // daemon for todos itself and never fabricates one.
  todoRefresh: (routeGeneration: number) =>
    invoke<SessionPresentationView>('desktop_todo_refresh', { routeGeneration }),
  // Phase 3 read-only project facts. The renderer names the git fact
  // set it wants and nothing else: the host reads it for the
  // route-selected project and the daemon resolves the repository root
  // server-side, so no path ever crosses the bridge. Each read is
  // fenced on the route generation.
  gitRead: (operation: GitReadKind, routeGeneration: number) =>
    invoke<GitReadView>('desktop_git_read', { operation, routeGeneration }),
  skillCatalogue: (routeGeneration: number) =>
    invoke<SkillCatalogueView>('desktop_skill_catalogue', { routeGeneration }),
  // M007-D: the daemon's command catalog, and the execution of one
  // daemon-effect command form. The renderer holds no command list of its
  // own: it renders what the daemon's catalog says. The catalog version is
  // quoted back on an execution so a stale palette is refused rather than
  // executed against a classification the daemon no longer serves.
  commandCatalog: (routeGeneration: number) =>
    invoke<CommandCatalogView>('desktop_command_catalog', { routeGeneration }),
  commandExecute: (
    commandId: string,
    form: string,
    argument: string,
    catalogVersion: number,
    routeGeneration: number,
  ) =>
    invoke<CommandExecuteView>('desktop_command_execute', {
      commandId,
      form,
      argument,
      catalogVersion,
      routeGeneration,
    }),
  // The editor (M007-E). Every call is a bounded operation with a bounded
  // DTO: the host never serves the document, only the visible window it
  // cut from its own snapshot. The renderer names a movement or an edit
  // range; the host owns the byte arithmetic.
  documentOpen: (path: string, routeGeneration: number) =>
    invoke<EditorOpenView>('desktop_document_open', { path, routeGeneration }),
  documentViewport: (firstLine: number | null, lineCount: number | null) =>
    invoke<EditorViewportView>('desktop_document_viewport', { firstLine, lineCount }),
  documentCursor: (movement: string, extend: boolean) =>
    invoke<EditorViewportView>('desktop_document_cursor', { movement, extend }),
  documentEdit: (start: number, end: number, insert: string) =>
    invoke<EditorEditView>('desktop_document_edit', { start, end, insert }),
  documentUndo: () => invoke<EditorEditView>('desktop_document_undo'),
  documentRedo: () => invoke<EditorEditView>('desktop_document_redo'),
  documentSave: () => invoke<EditorStateView>('desktop_document_save'),
  documentReload: () => invoke<EditorStateView>('desktop_document_reload'),
  documentResync: () => invoke<EditorStateView>('desktop_document_resync'),
  documentClose: () => invoke<EditorStateView>('desktop_document_close'),
  documentState: () => invoke<EditorStateView>('desktop_document_state'),
  documentTree: (prefix: string, routeGeneration: number) =>
    invoke<EditorTreeView>('desktop_document_tree', { prefix, routeGeneration }),
  documentDiagnostics: (routeGeneration: number) =>
    invoke<EditorDiagnosticsView>('desktop_document_diagnostics', { routeGeneration }),
  documentDiagnosticsSubscribe: (
    routeGeneration: number,
    onMessage: (view: EditorDiagnosticsView) => void,
  ) => {
    const channel = new Channel<EditorDiagnosticsView>();
    channel.onmessage = onMessage;
    return invoke<string>('desktop_document_diagnostics_subscribe', { routeGeneration, channel });
  },
  documentDiagnosticsUnsubscribe: (subscriptionId: string) =>
    invoke<void>('desktop_document_diagnostics_unsubscribe', { subscriptionId }),
  documentCompletion: (trigger: string | null) =>
    invoke<EditorCompletionView>('desktop_document_completion', { trigger }),
  documentNavigate: (operation: string) =>
    invoke<EditorNavigationView>('desktop_document_navigate', { operation }),
  documentPreview: (operation: 'format' | 'rename', newName: string | null, routeGeneration: number) =>
    invoke<LspPreviewView>('desktop_document_preview', {
      operation,
      newName,
      routeGeneration,
    }),
  documentPreviewApply: (candidate: LspPreviewCandidate, routeGeneration: number) =>
    invoke<string>('desktop_document_preview_apply', { candidate, routeGeneration }),
  disconnect: () => invoke<void>('desktop_disconnect'),
  unsubscribe: (subscriptionId: string) =>
    invoke<void>('desktop_unsubscribe_events', { subscriptionId }),
  subscribe: async (
    onEvent: (event: DesktopEvent) => void,
  ): Promise<SubscriptionHandle> => {    const channel = new Channel<DesktopEvent>();
    channel.onmessage = onEvent;
    const info = await invoke<SubscriptionInfo>('desktop_subscribe_events', {
      channel,
    });
    let cleaned = false;
    const unsubscribe = () => {
      if (cleaned) return;
      cleaned = true;
      channel.onmessage = () => undefined;
      void invoke<void>('desktop_unsubscribe_events', {
        subscriptionId: info.subscriptionId,
      }).catch(() => undefined);
    };
    return {
      subscriptionId: info.subscriptionId,
      connectionGeneration: info.connectionGeneration,
      unsubscribe,
    };
  },
  subscribeProjection: async (
    onView: (view: SessionPresentationView) => void,
  ): Promise<{ unsubscribe: () => void }> => {
    const channel = new Channel<SessionPresentationView>();
    channel.onmessage = onView;
    await invoke<void>('desktop_projection_subscribe', { channel });
    let cleaned = false;
    return {
      unsubscribe: () => {
        if (cleaned) return;
        cleaned = true;
        channel.onmessage = () => undefined;
        // The host watcher exits on channel close; stop is explicit
        // via projectionStop so reload/close teardown stays deliberate.
      },
    };
  },
};

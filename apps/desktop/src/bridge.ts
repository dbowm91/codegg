import { invoke, Channel } from '@tauri-apps/api/core';
import type {
  ConnectionSnapshot,
  DesktopEvent,
  ProjectDetailView,
  PromptSubmitView,
  ProjectSummary,
  RouteTokenView,
  SessionListView,
  SessionPresentationView,
  SessionView,
  SubscriptionHandle,
  SubscriptionInfo,
} from './bridge-types';

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
  projectionStart: (sessionId: string, routeGeneration: number) =>
    invoke<SessionPresentationView>('desktop_projection_start', { sessionId, routeGeneration }),
  projectionStop: () => invoke<RouteTokenView>('desktop_projection_stop'),
  projectionCurrent: () => invoke<SessionPresentationView>('desktop_projection_current'),
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

import { invoke, Channel } from '@tauri-apps/api/core';
import type {
  ConnectionSnapshot,
  DesktopEvent,
  ProjectSummary,
  SubscriptionHandle,
  SubscriptionInfo,
} from './bridge-types';

export const bridge = {
  connect: () => invoke<ConnectionSnapshot>('desktop_connect'),
  snapshot: () => invoke<ConnectionSnapshot>('desktop_connection_snapshot'),
  projects: () => invoke<ProjectSummary[]>('desktop_project_list'),
  disconnect: () => invoke<void>('desktop_disconnect'),
  unsubscribe: (subscriptionId: string) =>
    invoke<void>('desktop_unsubscribe_events', { subscriptionId }),
  subscribe: async (
    onEvent: (event: DesktopEvent) => void,
  ): Promise<SubscriptionHandle> => {
    const channel = new Channel<DesktopEvent>();
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
};

import { invoke, Channel } from '@tauri-apps/api/core';
import type { ConnectionSnapshot, DesktopEvent, ProjectSummary } from './bridge-types';

export const bridge = {
  connect: () => invoke<ConnectionSnapshot>('desktop_connect'),
  snapshot: () => invoke<ConnectionSnapshot>('desktop_connection_snapshot'),
  projects: () => invoke<ProjectSummary[]>('desktop_project_list'),
  disconnect: () => invoke<void>('desktop_disconnect'),
  subscribe: (onEvent: (event: DesktopEvent) => void) => {
    const channel = new Channel<DesktopEvent>();
    channel.onmessage = onEvent;
    void invoke('desktop_subscribe_events', { channel });
    return () => { channel.onmessage = () => undefined; };
  },
};

// Whether the projection the renderer holds carries an installed snapshot.
//
// The presenter publishes a *skeleton* — the session's identity, the driver
// state and empty lists — when no snapshot is installed yet
// (`apps/desktop/src-tauri/src/present.rs:742-774`), and the driver states
// in which that happens are `connecting`, `subscribing`, `resyncing`,
// `disconnected` and `unavailable` (`present.rs:110-119`). An empty list
// under one of those states means "not known yet", not "there is nothing":
// a panel that draws the empty state for both makes a claim the daemon
// never made.
//
// "No session attached" is a third, separate fact: the renderer holds no
// projection at all because no session was opened.

export type SnapshotAvailability = 'present' | 'no-snapshot' | 'no-session';

const NO_SNAPSHOT_STATES = new Set([
  'connecting',
  'subscribing',
  'resyncing',
  'disconnected',
  'unavailable',
]);

export const NO_SNAPSHOT_MESSAGE = 'No snapshot installed yet.';

export const NO_SESSION_MESSAGE = 'No session attached.';

/**
 * What the projection's driver state says about the lists it carries.
 *
 * An unstated state (`undefined`) is not a claim about the snapshot: the
 * caller has not said anything, so the reading stays the one it had.
 */
export function snapshotAvailability(state: string | null | undefined): SnapshotAvailability {
  if (state === undefined) return 'present';
  if (state === null) return 'no-session';
  return NO_SNAPSHOT_STATES.has(state.trim().toLowerCase()) ? 'no-snapshot' : 'present';
}

/**
 * The sentence a panel draws when the lists it is about to render are not
 * known yet. `null` means the lists are the daemon's to be trusted.
 */
export function snapshotMessage(availability: SnapshotAvailability): string | null {
  if (availability === 'no-snapshot') return NO_SNAPSHOT_MESSAGE;
  if (availability === 'no-session') return NO_SESSION_MESSAGE;
  return null;
}

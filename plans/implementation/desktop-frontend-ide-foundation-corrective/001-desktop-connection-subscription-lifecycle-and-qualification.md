# Desktop Frontend and IDE Foundation Corrective C001 — Connection/Subscription Lifecycle and Display Qualification

Status: ready for handoff

Repository baseline: `82e289db90c4c7981524b4291986af39a7f881ea`

Source corrective addendum:

- `plans/subsystems/desktop-frontend-ide-foundation-m003-lifecycle-corrective-addendum.md`

Predecessor implementation and closure:

- `plans/implementation/desktop-frontend-ide-foundation/003-tauri-desktop-shell-and-bridge.md`
- `plans/closure/desktop-frontend-ide-foundation/003-status.md`

Applicable ADR:

- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md`

Primary class: invariant / frontend lifecycle corrective

Hard dependencies:

- M001 shared frontend client runtime: closed.
- M003 Tauri shell implementation: landed and conditionally closed.

Operational dependencies:

- a macOS or Linux display-backed environment is required for final qualification;
- M002 Windows qualification is not required to close this corrective for Linux/macOS, and remains separate.

## 1. Objective

Repair desktop connection/subscription ownership so renderer unsubscribe, reconnect, disconnect, reload, and close deterministically stop and join stale project-event forwarders and release their `LocalSocketClient` clones. Then run the real display-backed desktop lifecycle trajectory required to promote M003 from conditional to strict closure.

Do not implement M004 session behavior in this corrective.

## 2. Confirmed defect

Current code can retain a stale daemon connection after reconnect or renderer teardown:

```text
desktop_subscribe_events
    -> clone LocalSocketClient
    -> client.subscribe()
    -> detached tauri::async_runtime::spawn
    -> no HostState task/cancel owner
```

The renderer cleanup currently does only:

```ts
channel.onmessage = () => undefined;
```

That does not signal Rust-side cancellation.

`desktop_disconnect` drops only `HostState.client`. The event-forwarder task still owns another clone of the old client.

A successful reconnect can therefore produce:

```text
HostState.client --------------> new daemon connection

old event forwarder -----------> old daemon connection
       |
       `-- old Tauri Channel
```

The React subscription effect depends on `connection.state`. Because a reconnect can remain `connected -> connected`, no effect dependency necessarily changes and the renderer may continue using the old subscription while commands use the new client.

This is a correctness/resource-lifecycle defect, not only a missing-E2E-evidence issue.

## 3. Invariants

- One active project-event subscription per active desktop renderer/connection generation.
- Every forwarder has explicit cancellation and terminal/join ownership.
- No stale task may keep a superseded `LocalSocketClient` alive.
- Renderer cleanup causes Rust cleanup.
- Subscription ids and connection generations are distinct and both are checked where needed.
- Stale unsubscribe cannot cancel a current subscription.
- Stale event delivery cannot mutate current renderer state.
- Reconnect installs a new visible connection generation even when the coarse state remains `connected`.
- Desktop shutdown never stops daemon-owned work.
- Existing least-privilege Tauri security posture remains unchanged.

## 4. Required production changes

### WP1 — Extract an explicit desktop host lifecycle controller

Introduce a testable Rust-side owner for active desktop connection/subscription state rather than leaving ownership spread across Tauri command functions.

A target shape may be:

```rust
struct DesktopConnection {
    generation: u64,
    client: LocalSocketClient,
    daemon_id: String,
}

struct DesktopSubscription {
    subscription_id: String,
    connection_generation: u64,
    cancel: CancellationToken,
    task: JoinHandle<()>,
}

struct HostState {
    connection: Mutex<Option<DesktopConnection>>,
    subscription: Mutex<Option<DesktopSubscription>>,
    next_generation: AtomicU64,
}
```

Exact types may differ. The important contract is explicit ownership.

Do not expose `JoinHandle` or cancellation internals to TypeScript.

If `tokio_util::sync::CancellationToken` is used, add only the narrow desktop dependency needed. An owned abort handle plus awaited join is also acceptable if semantics are equivalent.

Acceptance evidence:

- controller has deterministic unit tests independent of a visible WebView;
- no detached subscription task exists without a stored owner;
- task cancellation/drop paths do not hold a mutex guard across await in a way that can deadlock.

### WP2 — Make subscription creation/replacement identity-scoped

Change `desktop_subscribe_events` so subscription creation returns an explicit bounded subscription identity and is tied to the current connection generation.

Recommended bridge result:

```text
{ subscriptionId, connectionGeneration }
```

On subscribe:

1. snapshot the current connection and generation;
2. create a new subscription id;
3. cancel/join any previous active project-event subscription, or explicitly reject duplicate subscription creation if that produces cleaner renderer semantics;
4. install the new owner atomically;
5. spawn the forwarder;
6. before every send or on terminal cleanup, confirm the owner/generation is still current where necessary;
7. clear the stored owner only if the terminating task still matches the stored subscription id.

Do not let a terminated old task clear a newer owner's state.

Acceptance evidence:

- replace/resubscribe cannot leave two live forwarders;
- an old terminal task cannot clear the new owner;
- repeated cycles return ownership counters to baseline.

### WP3 — Add explicit unsubscribe and lifecycle teardown

Add a narrow command such as `desktop_unsubscribe_events(subscription_id)`.

It must:

- locate only the matching active subscription;
- cancel it;
- await/join its forwarder or otherwise obtain deterministic terminal observation;
- be idempotent for an already-ended matching subscription;
- ignore/reject a stale id without affecting a newer subscription.

Update `desktop_disconnect` to:

1. increment/invalidate the connection generation;
2. cancel/join the active subscription;
3. drop the active client/daemon identity;
4. leave daemon process/work untouched.

Update reconnect/install logic so a successful new connection cannot coexist with a stale subscription bound to the previous connection. Prefer cancelling the stale subscription before committing the new connection as current, while avoiding loss of a still-good connection on a failed reconnect attempt.

Acceptance evidence:

- disconnect returns only after the host no longer owns the subscription task/client clone;
- failed reconnect does not destroy a valid current connection unless product semantics explicitly choose that behavior and tests document it;
- successful reconnect has exactly one current client and zero stale project-event forwarders before renderer re-subscribe.

### WP4 — Expose a renderer-visible connection generation

Extend `ConnectionSnapshot` with a generation or equivalent opaque connection identity.

Generation changes on every successfully installed connection generation and on transitions that invalidate the prior connection. Do not use daemon id alone because the same daemon survives reconnect.

Update generated/manual bridge typings and `bindings:check`.

Renderer logic must key subscription setup on the current connection generation rather than only `connection.state`.

Acceptance evidence:

- connected generation N -> connected generation N+1 reruns subscription setup;
- stale async project refresh/event work from generation N is ignored after N+1 becomes current.

### WP5 — Repair React/bridge subscription cleanup

Make `bridge.subscribe` asynchronous or otherwise return enough identity for an explicit Rust unsubscribe.

The React effect must handle all races:

- normal mount -> subscribe -> cleanup;
- cleanup before subscribe command resolves;
- reconnect while subscribe is in flight;
- subscription channel terminal failure;
- component unmount;
- repeated reconnects.

A safe shape is:

```ts
const subscription = await bridge.subscribe(onEvent)
return () => void subscription.unsubscribe()
```

with an effect-local cancelled/stale flag that immediately unsubscribes a late result if cleanup already occurred.

Do not treat `channel.onmessage = noop` as teardown.

Acceptance evidence:

- renderer unit tests prove unsubscribe invocation;
- unmount-before-resolution produces no leaked subscription;
- reconnect with state remaining `connected` replaces the subscription.

### WP6 — Add lifecycle instrumentation/test seams

Add bounded test-only or architecture-approved observations sufficient to prove task/connection return to baseline.

Useful evidence may include:

- active desktop subscription count;
- current subscription id/generation;
- fake/mocked event receiver drop observation;
- daemon `SnapshotDaemon.connected_clients` before/after display-backed reconnect and close.

Do not add production diagnostics that expose secrets or turn test instrumentation into a new public protocol requirement.

### WP7 — Preserve Tauri security and build boundaries

Rerun and, if useful, strengthen `scripts/check-desktop-boundary.sh`.

The corrective must not add:

- renderer plugin permissions;
- shell/process/fs/http/updater capabilities;
- remote IPC origins;
- generic `core_request` invoke;
- Tauri dependencies to root workspace;
- a root MSRV change.

### WP8 — Display-backed strict M003 qualification

After deterministic tests are green, run the actual app on a display-backed macOS or Linux host.

Record exact OS, architecture, desktop/Tauri build mode, daemon executable path, and commands.

Required trajectory:

1. start a canonical daemon or prepare explicit-path autostart;
2. record daemon connected-client count;
3. launch desktop and confirm connected daemon identity + project catalog;
4. keep a TUI client attached simultaneously;
5. record expected connected-client count;
6. trigger desktop reconnect while coarse state remains connected;
7. prove old client/subscription disappears and exactly one desktop client remains;
8. mutate/register/archive/restore a project using an existing deterministic fixture or safe operator action and prove only the current desktop subscription reacts;
9. reload/recreate renderer and repeat subscription-baseline assertion;
10. close desktop and prove desktop client count returns to baseline while daemon and TUI remain responsive;
11. separately exercise desktop autostart using explicit `codegg` executable and prove desktop exit leaves daemon running.

Do not claim a UI path was exercised from a headless shell.

## 5. Protocol/storage impact

Expected:

- no CoreFrame version change;
- no daemon storage migration;
- no project/session/projection DTO change;
- desktop bridge DTO change only for connection/subscription identity.

If a daemon protocol change becomes necessary, stop and re-plan.

## 6. Runtime and concurrency semantics

- Connection generation changes are monotonic within one desktop process.
- Subscription identity is unique within that process.
- Subscription cancellation is idempotent.
- A subscription task may clear HostState ownership only with compare-by-id/generation semantics.
- Connection replacement must not deadlock with event forwarding.
- Event forwarding remains bounded by `LocalSocketClient::subscribe()` and Tauri Channel behavior.
- Project invalidation events retain daemon `event_seq`.
- No task-per-event spawning.
- Desktop close cancels desktop tasks only.

## 7. Required tests

### Rust tests

At minimum:

- subscribe then unsubscribe releases task/owner;
- subscribe twice cannot accumulate two owners;
- stale unsubscribe leaves newer subscription alive;
- disconnect tears down subscription before/with client release;
- successful reconnect invalidates old subscription;
- failed reconnect preserves or predictably transitions current connection according to documented semantics;
- old task terminal cleanup cannot clear newer owner;
- channel failure clears matching owner;
- 25–100 repeated lifecycle cycles show zero owner/task accumulation.

### Renderer tests

At minimum:

- generation N -> N+1 causes re-subscribe with connected state unchanged;
- cleanup invokes Rust unsubscribe;
- unmount-before-subscribe-resolve immediately cleans the late subscription;
- stale generation project event is ignored;
- repeated reconnect does not accumulate callbacks/subscriptions.

### Boundary tests

- `scripts/check-desktop-boundary.sh`;
- bridge type/binding drift check;
- no forbidden Tauri permissions;
- root Cargo metadata excludes desktop package;
- root Rust MSRV remains 1.89.

### Regression tests

- `cargo test -p codegg-client`;
- desktop Rust host tests;
- desktop renderer/typecheck/build;
- root quick verification;
- existing daemon lifecycle tests if touched indirectly.

## 8. Suggested verification commands

Record exact commands actually run.

```bash
cargo test -p codegg-client --locked
cargo test -p codegg --test single_daemon_lifecycle --locked
scripts/check-desktop-boundary.sh
scripts/verify.sh quick
cargo fmt --all -- --check
git diff --check

cd apps/desktop
npm ci
npm run typecheck
npm test
npm run bindings:check
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
npm run tauri build -- --debug --no-bundle
```

Then execute the display-backed trajectory separately and record it as runtime evidence, not CI unless actually run in CI.

## 9. Documentation updates

Update:

- `architecture/desktop.md` with connection/subscription ownership and generation model;
- predecessor roadmap M003 status only when C001 closes;
- `plans/registry.md`;
- C001 closure record;
- M003 status reconciliation additively, preserving the original conditional closure record.

Do not rewrite `plans/closure/desktop-frontend-ide-foundation/003-status.md` to erase the original evidence gap/defect discovery.

## 10. Acceptance criteria

C001 is complete when:

1. every desktop project-event forwarder has explicit host ownership.
2. renderer unsubscribe reaches Rust and terminates the matching forwarder.
3. disconnect and reconnect cannot leave a stale client clone held by an event task.
4. successful reconnect changes renderer-visible connection identity and causes re-subscription.
5. stale subscription cleanup cannot affect a newer subscription.
6. repeated lifecycle tests show no subscription/task accumulation.
7. root/Tauri capability and MSRV boundaries remain unchanged.
8. the real display-backed existing-daemon, coexistence, reconnect, renderer-reload, close, and explicit-autostart trajectories pass.
9. daemon connected-client counts return to expected baseline after reconnect/reload/close.
10. no unresolved high/medium lifecycle/security finding remains.

When all criteria pass, strict-close M003 and register M004 as ready.

## 11. Stop conditions

Stop and report if:

- a daemon/CoreFrame protocol change is required;
- the fix requires M004 session-projection implementation;
- a broad Tauri plugin permission is proposed;
- the only cleanup mechanism is abandoning tasks without terminal observation;
- fixing reconnect requires killing/restarting the daemon;
- root MSRV/toolchain isolation must change;
- display-backed evidence exposes an independent daemon client-registry leak;
- Windows-specific runtime behavior becomes the only blocker: keep that under M002 rather than expanding C001.

## 12. Closure evidence required

Create:

- `plans/closure/desktop-frontend-ide-foundation-corrective/001-status.md`

Record:

- implementation commit(s);
- exact lifecycle ownership model;
- task cancellation/join mechanism;
- bridge command/DTO changes;
- deterministic Rust + renderer lifecycle test counts/results;
- repeated-cycle evidence;
- desktop boundary/security guard results;
- display-backed platform/commands;
- connected-client counts through existing-daemon/reconnect/reload/close/autostart trajectories;
- TUI coexistence result;
- root quick/build/MSRV non-regression;
- unresolved findings and severity;
- final disposition for M003 and M004.

## 13. Handoff notes

Prefer a small lifecycle controller over adding more ad hoc fields directly to Tauri commands.

Do not use a global event bus as a shortcut; M004 will need stronger generation/subscription ownership, so this corrective should establish the pattern now.

The desktop shell remains intentionally minimal. Once this corrective closes, M004 can build session projections on a trustworthy connection/subscription lifecycle rather than carrying this debt forward.

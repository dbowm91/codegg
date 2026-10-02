# Desktop Frontend and IDE Foundation Corrective C002 — Atomic Lifecycle Transitions, Subscription Arming, and Window Teardown

Status: ready for handoff

Repository baseline: `5005389b176545ae9df86d8d74441434aeddaa8b`

Source corrective addendum:

- `plans/subsystems/desktop-frontend-ide-foundation-m003-lifecycle-corrective-addendum.md`

Predecessor implementation and evidence:

- `plans/implementation/desktop-frontend-ide-foundation-corrective/001-desktop-connection-subscription-lifecycle-and-qualification.md`
- `plans/closure/desktop-frontend-ide-foundation-corrective/001-status.md` — C001 conditionally closed
- C001 implementation `7b17808f`

Applicable architecture:

- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md`
- `architecture/desktop.md`

Primary class: invariant / concurrency / frontend lifecycle corrective

Hard dependencies:

- C001 production ownership repair is implemented.
- M003 Tauri shell exists and remains conditionally closed.

Operational closure dependency:

- after C002 correctness closes, the previously registered visible-window interaction run is still required before strict M003 closure and M004 readiness.

## 1. Objective

Make desktop connection, reconnect, disconnect, subscription installation, and window/app teardown linearizable at the Rust host boundary.

C001 established explicit subscription identity, task ownership, cancel/join semantics, renderer-visible connection generations, and renderer-side unsubscribe. Follow-up source review found races between those individually correct operations because their final state transitions are not governed by one lifecycle serialization boundary.

C002 must ensure that:

- no subscription can be installed against a connection generation after that generation has been invalidated;
- no event forwarder can observe itself as non-current before its owner is installed;
- disconnect supersedes every connect attempt that began before the disconnect;
- concurrent connect attempts cannot commit after a newer lifecycle decision has superseded them;
- window/app teardown reaches Rust-side disconnect rather than relying on renderer cleanup or process termination;
- every completed lifecycle command leaves a self-consistent tuple of connection, generation, subscription owner, and connect-attempt epoch.

Do not implement M004 session/projection features in this corrective.

## 2. Confirmed concurrency findings

### Finding A — subscription task can run before ownership is installed

Current `subscribe_with_sink` performs, in order:

1. snapshot client + generation;
2. create the client event receiver;
3. `tokio::spawn` the forwarder;
4. call `SubscriptionRegistry::install_with_id`.

The forwarder checks `active_snapshot()` before every send. A project event can arrive after step 3 but before step 4. In that interleaving the new subscription id is not yet the active owner, so the forwarder exits and `clear_if_matching` clears nothing. Step 4 can then install the already-finished `JoinHandle` as the nominal active owner.

Observable failure:

- subscribe returns success;
- registry reports an active subscription id;
- no task remains capable of forwarding subsequent project invalidations.

Generation fencing does not prevent this race because the failure is owner-publication ordering inside one generation.

### Finding B — subscribe is not serialized with disconnect/reconnect

`subscribe_with_sink` snapshots `HostState.client` and `connection_generation` without a lifecycle transition lock. `disconnect_host` and `install_connection` can run between that snapshot and `install_with_id`.

Representative disconnect interleaving:

```text
subscribe                       disconnect
---------                       ----------
clone old client
read generation G
                                generation := G + 1
                                subscriptions.shutdown()
spawn forwarder
install owner for old client/G
                                clear/drop active connection
```

Representative reconnect interleaving:

```text
subscribe                       reconnect commit
---------                       ----------------
clone old client / G
                                subscriptions.shutdown()
spawn forwarder
                                install new client
                                generation := G + 1
install owner tagged G
```

The forwarder eventually notices a generation mismatch once it receives an event, and renderer cleanup may also remove it, but host correctness must not depend on a later event or JavaScript cleanup. A completed transition must not leave stale host ownership installed.

### Finding C — disconnect does not supersede in-flight connect attempts

`desktop_connect` allocates an attempt id from `connect_serial` and checks whether that attempt has been superseded before installation.

`disconnect_host` increments `connection_generation` but does not advance `connect_serial`.

Therefore:

1. connect attempt A begins;
2. user disconnects while A is doing local connect/autostart/snapshot work;
3. disconnect completes;
4. A still sees itself as the latest connect attempt;
5. A installs a connection after the explicit disconnect.

The disconnect has been undone by an operation that began before it.

### Finding D — connect supersession check and final commit are not atomic

A connect attempt checks `is_superseded()`, then separately calls `install_connection`. A newer connect attempt can begin between those operations.

This permits an older attempt to commit after it has become stale. Even if a later successful attempt eventually replaces it, an intervening failure or subscription request can observe the wrong committed generation.

### Finding E — native window/app close has no explicit host teardown hook

The desktop Rust host currently exposes `desktop_disconnect`, but `run()` does not register a Tauri window/app lifecycle hook that invokes the host teardown when the main window is closed/destroyed or the app is exiting.

Renderer effect cleanup is insufficient as the sole native teardown guarantee because the WebView may be destroyed before an async invoke completes. Tauri 2 exposes native `WindowEvent::CloseRequested`/`Destroyed` and application `RunEvent` lifecycle events; the Rust host must own a deterministic teardown path appropriate to the selected close behavior.

This matters independently of process exit, especially on platforms where closing the main window does not necessarily imply immediate process termination.

## 3. Required invariants

### Linearizable host state

After any public desktop lifecycle operation returns, host state must describe one coherent lifecycle epoch:

```text
(connection_generation,
 current connection?,
 current daemon id?,
 active subscription?,
 latest connect-attempt serial)
```

No active subscription may name a generation different from the current installed connection generation.

### Disconnect authority

A completed explicit disconnect or native main-window teardown supersedes every connect attempt that started before it.

An old connect attempt must never resurrect the connection after disconnect.

### Connect commit authority

Only the latest still-valid connect attempt may commit a newly established client.

The supersession check and state commit must occur under the same lifecycle serialization boundary.

Network connection, daemon startup, and snapshot requests must remain outside that boundary so a slow daemon cannot block disconnect/subscription state indefinitely.

### Subscription publication

A newly spawned forwarder must not process events until the registry/state has published it as the active owner.

The task is armed only after ownership installation succeeds for the same current connection generation.

### Subscription/transition exclusion

Subscribe installation and connection invalidation/replacement are mutually ordered.

If subscribe linearizes first, a following disconnect/reconnect tears it down before completing.

If disconnect/reconnect linearizes first, subscribe either binds the new current generation or fails because no connection exists.

There is no third state where a stale subscription becomes active after a transition completed.

### Terminal cleanup

Forwarder terminal cleanup remains compare-by-subscription-id and generation-safe. It must never clear a newer owner.

### Window/app teardown

Closing/destroying the main window or exiting the desktop app reaches the same Rust-host teardown semantics as explicit `desktop_disconnect`, without stopping the daemon.

Repeated/overlapping close/disconnect events are idempotent.

### Security/toolchain

No new renderer permissions, generic machine authority, protocol version, storage migration, root-workspace dependency, or root MSRV change.

## 4. Target ownership model

Prefer one explicit lifecycle controller or serialization gate rather than additional independent atomics.

A minimal acceptable shape is:

```rust
struct HostState {
    lifecycle_gate: Mutex<()>,
    client: Mutex<Option<LocalSocketClient>>,
    daemon_id: Mutex<Option<String>>,
    connection_generation: AtomicU64,
    connect_serial: AtomicU64,
    subscriptions: SubscriptionRegistry,
}
```

A stronger refactor may place the coherent state tuple behind one controller mutex:

```rust
struct LifecycleState {
    connection: Option<DesktopConnection>,
    connection_generation: u64,
    connect_serial: u64,
    subscription: Option<OwnedSubscription>,
}

struct DesktopLifecycle {
    state: Mutex<LifecycleState>,
    next_subscription_id: AtomicU64,
}
```

Either is acceptable if the implementation proves the invariants and lock ordering. Do not refactor unrelated desktop bridge code merely to reach the stronger shape.

Critical rule: slow external work does not run while holding the lifecycle transition lock.

## 5. Work packages

### WP1 — Introduce a lifecycle transition serialization boundary

Add a single explicit boundary governing final state-changing phases of:

- connect commit;
- reconnect commit;
- disconnect;
- subscription install/replace;
- native window/app teardown.

Document the linearization point for each operation.

If multiple internal mutexes remain, document and mechanically preserve one lock order. Prefer taking ownership out of locked state and performing abort/join after state has been made inaccessible to new users.

Acceptance:

- no network I/O or daemon startup runs under the transition gate;
- no transition waits indefinitely on a forwarder that itself needs the same gate;
- state inspection used for correctness is performed under the same serialized epoch as mutation, not by unrelated atomics before/after it.

### WP2 — Make connect finalization atomic and disconnect-superseding

Split `desktop_connect` into:

1. slow preparation outside the lifecycle gate:
   - resolve endpoint;
   - connect/reuse/autostart;
   - negotiate daemon identity;
   - request daemon snapshot;
2. serialized commit:
   - acquire lifecycle boundary;
   - verify `attempt == current connect_serial`;
   - if stale, drop the prepared client and return superseded;
   - tear down/take the prior subscription owner;
   - install client + daemon id;
   - advance connection generation exactly once;
   - release lifecycle boundary;
   - cancel/join any removed old subscription if that work was intentionally moved outside the gate;
   - return a snapshot carrying the committed generation.

`disconnect_host` must, under the same boundary:

- advance/invalidate the connect-attempt serial so every earlier in-flight connect is stale;
- invalidate/advance connection generation;
- remove active subscription ownership;
- clear active client/daemon id;
- release the boundary;
- cancel/join removed forwarder ownership;
- return only after desktop-owned resources are terminally observed.

A newer connect begun after the disconnect remains allowed.

Acceptance:

- connect A -> disconnect -> A completion cannot reconnect;
- connect A -> connect B -> A finalization cannot commit after B became current;
- connect B failure does not permit a stale A commit after B's attempt epoch was allocated;
- failed latest reconnect preserves the prior valid connection unless an explicit disconnect occurred.

### WP3 — Arm forwarders only after owner publication

Add a start/arming barrier between task creation and event processing.

Acceptable mechanisms include:

- `tokio::sync::oneshot`;
- `Notify` with state;
- a small owned start gate.

Required ordering:

```text
allocate subscription id
create event receiver
spawn task -> WAITING/UNARMED
serialize against lifecycle transitions
verify connection + generation still current
publish subscription owner
release/commit serialized state
ARM task
task may now read/process events
```

If installation fails or the connection was invalidated:

- drop/cancel the unarmed task;
- do not return a successful `SubscriptionInfo`;
- do not leave a registry owner.

The arming signal must not be sent before ownership is visible.

Acceptance:

- an event queued immediately after spawn cannot cause the task to self-terminate before installation;
- no completed task can be installed as the active owner due to pre-install execution;
- arm failure/caller cancellation cleans up without a detached waiter.

### WP4 — Serialize subscription installation with connection transitions

Under the lifecycle boundary:

- read the current connection and generation;
- reject when disconnected;
- establish the subscription against exactly that connection generation;
- publish the owner atomically with respect to disconnect/reconnect.

Do not rely on the renderer to repair a stale host subscription.

Define and test legal orderings:

**Subscribe wins first**
- subscribe returns generation G;
- a following reconnect/disconnect tears it down before transition completion.

**Transition wins first**
- reconnect: subscribe binds generation G+1;
- disconnect: subscribe observes no connection and fails.

Acceptance:

- after `disconnect_host().await`, active subscription count is zero;
- after successful reconnect commit, no active owner references the prior generation;
- no stale owner waits for a future event to discover it is stale.

### WP5 — Add deterministic native window/app teardown

Add Rust-side Tauri lifecycle handling for the main window/app.

Required semantics:

- a main-window close/destroy/app-exit path initiates the same host cleanup as `desktop_disconnect`;
- cleanup is idempotent with explicit renderer disconnect/unsubscribe;
- daemon process is not stopped;
- no reliance on a final JavaScript invoke for correctness;
- if the selected Tauri callback is synchronous and teardown must complete before destruction, use a bounded/reentrant-safe close protocol rather than fire-and-forget cleanup whose completion cannot be observed.

The implementation may use Tauri's native `WindowEvent`/`RunEvent` surfaces appropriate to current 2.12 APIs. If `CloseRequested` is intercepted to await cleanup, guard against recursively re-triggering the interception when the window is subsequently closed/destroyed.

On macOS, explicitly verify the behavior when the main window closes but the application process remains alive.

Acceptance:

- window close leaves zero desktop subscriptions and no desktop daemon client;
- daemon remains live;
- repeated close/exit events do not panic or double-own cleanup;
- app exit during an in-flight connect prevents that connect from committing.

### WP6 — Add deterministic race tests with explicit barriers

Do not rely on sleep-based probabilistic tests for the core races.

Add test seams/barriers around:

- subscribe snapshot before owner installation;
- subscription task arming;
- connect after network/snapshot preparation but before final commit;
- transition ownership removal before task join if needed.

Required interleavings:

1. event available before subscription owner publication;
2. subscribe paused before install + disconnect completes;
3. subscribe paused before install + reconnect commits;
4. reconnect paused before commit + disconnect completes;
5. connect A paused before commit + connect B starts;
6. connect A paused before commit + disconnect + A resumes;
7. old forwarder exits while newer subscription is installing;
8. native close races with subscribe;
9. native close races with connect finalization;
10. 50–100 repeated mixed lifecycle transitions return owner/task/client observations to baseline.

Tests must assert final state, not only lack of panic.

### WP7 — Re-run live daemon trajectory with adversarial transitions

Extend the ignored real-daemon lifecycle test or add a sibling focused C002 trajectory using an isolated `CODEGG_DAEMON_HOME`.

At minimum prove:

- TUI observer baseline;
- desktop current client count exactly one after connect;
- subscribe/reconnect/disconnect counts converge without waiting for a project event;
- disconnect while a prepared connect is paused cannot resurrect desktop;
- reconnect leaves exactly one desktop client and zero old-generation subscription owners;
- project invalidation reaches only the armed current subscription;
- native-host cleanup seam returns daemon client count to observer-only baseline;
- daemon remains responsive throughout.

Do not require a visible WebView for the C002 concurrency proof. The separate visible-window run remains the final M003 operational gate and should be executed after C002 closes.

### WP8 — Preserve renderer and security boundaries

The C001 renderer generation behavior remains valid and should not be broadened.

Rerun:

- renderer reconnect/resubscribe tests;
- late-subscribe cleanup tests;
- stale-generation event ignore;
- `bindings:check`;
- desktop boundary guard;
- root quick verification.

No new Tauri plugin permission is expected for native window event handling.

## 6. Concurrency model and lock rules

The implementation must document these rules in `architecture/desktop.md`:

1. one lifecycle transition serialization boundary defines connection/subscription linearization;
2. network connect/autostart/request work happens outside it;
3. connect attempt validity is checked inside it immediately before commit;
4. disconnect advances the connect-attempt epoch inside it;
5. subscription ownership is published inside it before arming the task;
6. forwarders never acquire the lifecycle transition boundary while being abort/joined under that same boundary;
7. if join happens after releasing the boundary, generation/ownership is invalidated before release so the old task cannot publish current events;
8. renderer generation checks are defense in depth, not host-state correctness authority.

No correctness requirement may depend on scheduler timing.

## 7. Required Rust tests

At minimum add focused tests named or equivalent to:

- `subscription_is_armed_only_after_owner_publication`;
- `event_before_install_does_not_leave_finished_owner`;
- `subscribe_racing_disconnect_is_linearizable`;
- `subscribe_racing_reconnect_binds_one_generation_only`;
- `disconnect_supersedes_inflight_connect`;
- `newer_connect_attempt_prevents_older_commit`;
- `failed_newer_connect_does_not_allow_stale_commit`;
- `native_close_tears_down_subscription_and_connection`;
- `native_close_supersedes_inflight_connect`;
- `mixed_lifecycle_stress_returns_to_baseline`.

Use barriers/oneshots to force the vulnerable interleavings.

If production Tauri event types are awkward in unit tests, extract a host-level close handler whose behavior is invoked from the actual Tauri event callback and test the host handler directly. Keep at least one integration/build check proving the Tauri callback is wired.

## 8. Renderer regression tests

No new renderer architecture is expected, but rerun and preserve:

- connected generation N -> N+1 re-subscribe;
- unmount unsubscribe;
- unmount-before-subscribe-resolution cleanup;
- stale generation invalidation ignore;
- repeated reconnect no accumulation.

If C002 changes subscription failure timing, add a test that a host rejection caused by concurrent disconnect does not leave a phantom renderer subscription handle.

## 9. Verification commands

Record exact commands actually executed. Expected minimum:

```bash
cargo test -p codegg-client --locked
cargo test -p codegg --test single_daemon_lifecycle --locked
scripts/verify.sh quick
scripts/check-desktop-boundary.sh
cargo fmt --all -- --check
git diff --check

cd apps/desktop
npm ci
npm run typecheck
npm test
npm run bindings:check
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

Run the isolated real-daemon C002 race trajectory explicitly with a fresh native-target build and record daemon client-count observations.

Hosted CI must be green after integration. Do not claim an absent workflow run as evidence.

## 10. Visible-window closure ordering

Do not spend the final visible-window qualification evidence against code that still contains the C002 races.

Required order:

```text
C002 implementation
    -> deterministic race tests
    -> real-daemon concurrency trajectory
    -> C002 closure
    -> visible-window M003 trajectory
    -> strict M003 closure
    -> M004 ready
```

If the visible-window run itself exposes a new correctness defect, do not strict-close M003; register a new bounded corrective only if the defect is outside C002's concurrency scope.

## 11. Protocol/storage/security impact

Expected:

- no CoreFrame change;
- no protocol version change;
- no storage migration;
- no daemon authorization change;
- no renderer permission addition;
- no root Rust MSRV change;
- no Tauri/root workspace merge.

Bridge DTOs need not change unless an explicit lifecycle result/error shape is required. Prefer keeping current `ConnectionSnapshot`/`SubscriptionInfo` contracts.

## 12. Documentation updates

Update:

- `architecture/desktop.md` with the lifecycle linearization boundary, connect-attempt epoch, subscription arm-before-run ordering, and native window teardown;
- this implementation plan status;
- corrective addendum C002 status;
- desktop foundation roadmap M003 status;
- `plans/registry.md`;
- new closure record.

Do not rewrite C001's closure record. It remains historical evidence of the first ownership repair and the later-discovered C002 races.

## 13. Completion criteria

C002 closes only when:

1. connect commit, disconnect, subscription install, and native teardown are mutually ordered by one lifecycle boundary.
2. disconnect supersedes every earlier in-flight connect.
3. an older connect cannot commit after a newer attempt became authoritative.
4. a forwarder cannot process events before its owner is published.
5. subscribe cannot install a stale owner after reconnect/disconnect completes.
6. every completed disconnect/native close has zero active desktop subscription owners.
7. every successful reconnect has exactly one current desktop connection and no active old-generation subscription owner.
8. deterministic barrier tests exercise each named race.
9. real-daemon adversarial transition evidence returns client/subscription counts to baseline.
10. renderer tests and Tauri security/MSRV boundary guards remain green.
11. hosted CI is green.
12. no unresolved high/medium concurrency/lifecycle finding remains within this scope.

C002 closure does **not** itself strict-close M003. The visible-window interaction run remains the final operational evidence item. Once that run passes on the C002-corrected code, promote M003 to strict `closed` and register M004 as `ready`.

## 14. Stop conditions

Stop and re-plan if:

- lifecycle atomicity requires a CoreFrame/daemon protocol change;
- M004 session projection work is required to solve these races;
- correctness requires holding a lifecycle mutex across network I/O;
- task cancellation can only be achieved by leaking/detaching tasks;
- native window cleanup requires broad renderer permissions;
- the daemon itself is shown to retain disconnected clients after the host has dropped all current ownership;
- Windows-only behavior becomes the blocker; keep it under M002.

## 15. Closure evidence required

Create:

- `plans/closure/desktop-frontend-ide-foundation-corrective/002-status.md`

Record:

- implementation commit(s);
- final lifecycle state model and linearization points;
- connect-attempt invalidation semantics;
- forwarder arming mechanism;
- native Tauri close/app-exit hook selected and why;
- deterministic interleaving test matrix/results;
- repeated mixed-lifecycle stress result;
- isolated real-daemon client-count and invalidation-delivery evidence;
- renderer regression results;
- desktop boundary/MSRV/security checks;
- hosted CI run;
- unresolved findings;
- disposition of the visible-window gate, M003, and M004.

## 16. Handoff notes

Keep this corrective narrow. The core issue is not the existence of atomics or mutexes individually; it is that lifecycle decisions currently have no single linearization boundary.

The implementation should make the legal state transitions obvious enough that M004 projection subscriptions can reuse the same ownership discipline rather than inventing another concurrency model.

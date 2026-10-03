# Desktop Frontend and IDE Foundation — M003 Lifecycle Corrective Addendum

Status: closed (C001/C002 conditionally closed per their records; C003 closed — hosted-CI reconciliation plus built-app visible-window qualification both green; M003 strict-closed, M004 unblocked to ready)

Repository baseline reviewed: `82e289db90c4c7981524b4291986af39a7f881ea`

Predecessor work:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`
- `plans/implementation/desktop-frontend-ide-foundation/003-tauri-desktop-shell-and-bridge.md`
- `plans/closure/desktop-frontend-ide-foundation/003-status.md`

Accepted architecture:

- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md`

Related closed dependencies:

- `plans/closure/desktop-frontend-ide-foundation/001-status.md`
- `plans/closure/desktop-frontend-ide-foundation/002-status.md` remains conditional only for its Windows qualification findings.

## 1. Purpose and corrective trigger

M003 conditionally closed after landing the optional Tauri 2 / React desktop shell, the narrow Rust bridge, root/desktop toolchain isolation, project-catalog rendering, and a least-privilege WebView capability/CSP boundary.

A follow-up source review found that M003's remaining work is not evidence-only. The desktop event-subscription lifecycle currently has a real ownership defect:

1. `desktop_subscribe_events` clones the active `LocalSocketClient` into a detached Tauri async task.
2. `HostState` stores no cancellation token, task handle, or subscription identity for that forwarder.
3. the TypeScript cleanup returned by `bridge.subscribe()` only replaces `channel.onmessage` with a no-op; it does not signal the Rust task to stop.
4. `desktop_disconnect` clears `HostState.client` but cannot cancel or join the existing forwarder, so the task's client clone may keep the old daemon connection alive.
5. a successful reconnect can replace `HostState.client` with a new client while an old forwarder still owns the previous client.
6. the React subscription effect depends only on `connection.state`; a successful connected -> connected reconnect need not recreate the subscription, so request/response traffic may use the new connection while project invalidations remain attached to the old one.

This violates ADR-0010's frontend-owned lifecycle rule and weakens the exact reconnect/subscription-generation guarantees needed by M004 session projections.

This corrective does not reopen the Tauri/toolchain/security architecture. It repairs client/subscription ownership and then collects the display-backed evidence already required for strict M003 closure.

## 2. Work classification

### Invariants

- At most one active desktop project-event subscription exists for the active renderer/connection generation unless a later feature explicitly requires multiple subscriptions.
- Every Rust event-forwarder task has an explicit owner, cancellation path, and join/terminal observation.
- Replacing or disconnecting a desktop daemon connection cannot leave a client clone retained solely by a stale event-forwarding task.
- Renderer subscription teardown must cause Rust-side subscription teardown; JavaScript handler replacement alone is not sufficient.
- A stale connection/subscription generation cannot publish events into the active renderer.
- A successful reconnect must bind subsequent requests and subscriptions to the same current connection generation.
- Desktop cleanup never stops the daemon or daemon-owned work.
- Existing Tauri least-privilege capabilities, local-only CSP, and absence of generic CoreRequest/filesystem/shell/process authority remain unchanged.
- M004 stays blocked until C003 reconciles hosted CI, the built-app visible-window qualification passes, and M003 is promoted to strict closed.

### Capabilities

No new end-user capability is introduced. Project catalog display and reconnect behavior remain the only product surface under repair.

### Infrastructure

Desktop Rust-host connection/subscription ownership, renderer bridge cleanup, lifecycle tests, and display-backed desktop qualification.

### Polish

Closure/documentation reconciliation after runtime evidence is collected.

## 3. Non-goals

- Implementing M004 session/project interaction.
- Session projection subscribe/replay/resume.
- Monaco, editor buffers, LSP document overlays, or terminal UI.
- Remote `/core` desktop access.
- Windows named-pipe runtime qualification owned by M002.
- Desktop signing/notarization/updater/distribution automation.
- Broad React state-management redesign.
- Changing Tauri framework choice, root MSRV, or desktop workspace isolation.
- Adding generic Tauri shell/filesystem/network permissions.

## 4. Current implementation evidence

At the reviewed baseline:

- `HostState` stores `client`, `daemon_id`, and an atomic `generation`; it does not store an event-forwarder owner.
- `desktop_subscribe_events` clones the current `LocalSocketClient`, calls `client.subscribe()`, and detaches a task with `tauri::async_runtime::spawn`.
- the forwarder exits only when the client event receiver closes or `Channel::send` returns an error.
- `bridge.subscribe()` invokes `desktop_subscribe_events` asynchronously and returns a cleanup function that only sets `channel.onmessage` to a no-op.
- `desktop_disconnect` increments generation and drops the client/daemon-id slots but has no forwarder handle to cancel/join.
- `desktop_connect` can replace `HostState.client` after a successful reconnect; an old forwarder can retain the old client independently.
- the React subscription effect is keyed by `connection.state`, not a connection-generation identity. A connected -> connected reconnect can therefore leave the existing JS subscription effect intact.
- `LocalSocketClient` itself already has explicit reader-task ownership and generation fencing; the defect is in the desktop host layer above it, not in the shared client crate.

## 5. Target lifecycle

The desktop host owns one explicit connection generation and one explicit project-event subscription associated with that generation:

```text
HostState
  |
  +-- active connection
  |     |-- generation
  |     |-- LocalSocketClient
  |     `-- daemon_id
  |
  `-- active renderer subscription
        |-- subscription_id
        |-- connection_generation
        |-- cancellation token
        `-- owned/joinable forwarder task
```

Reconnect or disconnect follows an ordered transition:

```text
invalidate old generation
        |
cancel active renderer subscription
        |
join/observe forwarder termination
        |
drop/replace old LocalSocketClient
        |
install new connection generation
        |
renderer establishes subscription for that generation
```

A renderer unsubscribe similarly cancels and joins only the matching subscription id/generation. A stale unsubscribe from an older renderer generation cannot tear down a newer subscription.

The exact Rust types may differ, but task ownership and generation identity must be explicit.

## 6. Corrective milestone

### C001 — Desktop connection/subscription ownership and display-backed qualification

Status: conditionally closed (`plans/closure/desktop-frontend-ide-foundation-corrective/001-status.md`; implementation `7b17808f`).

Plan:

- `plans/implementation/desktop-frontend-ide-foundation-corrective/001-desktop-connection-subscription-lifecycle-and-qualification.md`

C001 owns the original detached-forwarder/renderer-cleanup defect and recorded live command-level qualification. It remains conditionally closed.

A post-C001 source review found narrower concurrency defects in the final lifecycle transition boundary: subscribe can spawn before ownership publication, subscribe can race connection invalidation/replacement, disconnect does not supersede an already-running connect attempt, and connect supersession is checked separately from final commit. The Rust host also lacks an explicit native window/app teardown hook.

### C002 — Atomic lifecycle transitions, subscription arming, and window teardown

Status: conditionally closed (`plans/closure/desktop-frontend-ide-foundation-corrective/002-status.md`; implementation `dc32a6ad`).

Plan:

- `plans/implementation/desktop-frontend-ide-foundation-corrective/002-atomic-lifecycle-transitions-subscription-arming-window-teardown.md`

C002 owns only those post-C001 concurrency/lifecycle findings. Production implementation and local/adversarial-live evidence landed. Hosted run `37062733033` later failed in the unrelated `interactive_process_sessions::environment_overrides_apply_while_denied_vars_stay_stripped` test after desktop-relevant guards/Clippy had passed; the same PTY test passed on prior green hosted run `37051423825`, and no interactive-process implementation/test file changed across the desktop C002 delta. C002 therefore remains conditionally closed pending a fresh green hosted disposition rather than being reclassified as a desktop regression.

### C003 — Hosted-CI reconciliation and built-app visible-window strict closure

Status: closed (`plans/closure/desktop-frontend-ide-foundation-corrective/003-status.md`; implementations `eac14e01` through `cc9d9c19`; hosted `CI / verify` runs `37073506756` + `37092317678` SUCCESS; hosted `Desktop E2E` run `37092317709` SUCCESS with lifecycle 5/5 + autostart 2/2).

Plan:

- `plans/implementation/desktop-frontend-ide-foundation-corrective/003-hosted-ci-visible-window-strict-closure.md`

C003 owns the final strict-closure evidence:

- obtain/record a green hosted root run containing C002, or separately own any reproducible non-desktop CI defect without weakening the test;
- add a checked-in built-app WebdriverIO/Tauri harness using a test-only embedded WebDriver provider;
- prove rendered daemon/project state, real Tauri IPC Channel invalidation, visible reconnect, renderer reload, native close, TUI coexistence, explicit autostart, and daemon survival;
- mechanically prove the WebDriver test plugin/capability cannot ship in the production desktop build.

C003 contains no session/projection/prompt feature work. M004 remains blocked until C003 strict-closes M003.

If C003 exposes a new product defect rather than an evidence/test-harness issue, stop and register that defect separately instead of declaring M003 closed.

## 7. Cross-cutting requirements

### Connection identity

The Rust host must expose or internally maintain a monotonically changing connection generation. Renderer-side state must be able to distinguish connected generation N from connected generation N+1 even when both have `state = "connected"`.

### Subscription identity

Subscription teardown must be idempotent and identity-scoped. Recommended shape:

- Rust command starts a subscription and returns a generated `subscription_id`;
- Rust stores the active subscription owner;
- renderer cleanup invokes a narrow unsubscribe command carrying only that id;
- host verifies the id/generation before cancellation;
- a late cleanup from an old render cannot cancel the current subscription.

An equivalent design is acceptable if it provides the same ownership and stale-cleanup guarantees.

### Task cancellation and join

Cancellation without ownership/join evidence is insufficient. The host must be able to establish that the event-forwarding task stopped and released its `LocalSocketClient` clone.

Do not hold a Tokio mutex guard across a task join or bridge call in a way that can deadlock the forwarder.

### Renderer cleanup

React unmount/re-subscribe/reconnect cleanup must request Rust-side unsubscribe. Because effect cleanup cannot directly await, the bridge must safely handle late subscription setup and cleanup races: if the component becomes stale before subscribe returns, the resulting subscription must be immediately cancelled rather than leaked.

### Reconnect

Successful reconnect must update a renderer-visible generation/token so the subscription effect is re-established against the current connection. Reconnect failure must not fabricate a newer connected generation.

### Shutdown semantics

Desktop disconnect/app close releases only desktop-owned connection/subscription tasks. The daemon remains alive exactly as specified by ADR-0010.

### Security

No new renderer Tauri permissions are necessary. The corrective must preserve:

- empty plugin permission list;
- local bundled assets;
- restrictive CSP;
- no generic CoreRequest bridge;
- no direct filesystem/shell/process/network authority.

## 8. Verification strategy

C001 must add deterministic host/renderer tests before collecting manual/display-backed evidence.

### Rust host lifecycle tests

Prove:

- starting a second subscription cancels/joins or explicitly rejects/replaces the first according to the chosen one-active-subscription contract;
- unsubscribe releases the forwarder and its client clone;
- disconnect releases the active forwarder before/with client teardown;
- reconnect invalidates the previous subscription generation;
- stale unsubscribe cannot cancel a newer subscription;
- channel send failure terminates and clears the matching subscription owner;
- repeated subscribe/unsubscribe cycles return active task/subscription count to baseline.

If Tauri command types make direct unit tests awkward, extract a small host lifecycle controller independent of Tauri command macros and test that controller directly.

### Renderer tests

Prove:

- connected generation change resubscribes even when coarse connection state remains `connected`;
- effect unmount requests unsubscribe;
- unmount-before-subscribe-completion cleans up the late subscription id;
- stale project events from an old generation are ignored;
- reconnect does not leave two renderer subscriptions.

### Integration/display-backed evidence

On a host with a real display/session:

1. start/reuse an existing daemon;
2. launch the actual desktop app;
3. confirm daemon identity/project list rendering;
4. attach or keep a TUI client concurrently;
5. record daemon connected-client baseline;
6. trigger desktop reconnect;
7. prove the old desktop connection/subscription is released and the client count returns to the expected one-desktop + TUI state;
8. cause a project-catalog invalidation and prove only the current subscription refreshes;
9. reload/recreate the renderer and prove no accumulating connections/forwarders;
10. close the desktop and prove desktop client count returns to baseline while the daemon and TUI remain responsive;
11. exercise explicit-path autostart in a separate run and prove desktop exit does not stop the daemon.

A macOS or Linux display-backed run is sufficient for strict M003 closure. Windows remains separately conditional under M002.

## 9. Completion definition

C001 closes only when:

- the desktop Rust host explicitly owns every project-event forwarder;
- unsubscribe/disconnect/reconnect tear down stale forwarders and release stale client clones;
- renderer cleanup invokes Rust-side teardown and handles subscribe/setup races;
- connected -> connected reconnect causes a new subscription bound to the current generation;
- stale subscription ids/generations cannot affect the active connection;
- deterministic lifecycle tests cover repeated cycles with no accumulation;
- display-backed existing-daemon, autostart, reconnect, coexistence, reload, and close trajectories pass;
- the Tauri capability/CSP/boundary guard remains green;
- no unresolved high/medium desktop lifecycle or security finding remains.

On C001 closure:

1. record `plans/closure/desktop-frontend-ide-foundation-corrective/001-status.md`;
2. promote predecessor M003 from `conditionally closed` to `closed` by additive roadmap/registry reconciliation without rewriting the historical M003 closure record;
3. mark M004 `ready` and register it in the dependency-ready implementation table;
4. keep M002 conditional for Windows-specific evidence.

## 10. Stop conditions

Stop and register a new plan/ADR if:

- fixing ownership requires changing daemon session/projection semantics;
- M004 session projections are required to implement the project-event lifecycle fix;
- a generic Tauri event/global bus replaces the bounded channel solely to simplify cleanup;
- the renderer requires shell/filesystem/process privileges;
- the corrective requires raising root MSRV or folding Tauri into the root workspace;
- live display evidence reveals a daemon-side client-leak defect independent of the desktop host;
- a Windows-only blocker is encountered that belongs to M002.

## 11. Status

| Corrective | Status | Implementation plan | Blocker |
|---|---|---|---|
| C001 desktop connection/subscription lifecycle + qualification | conditionally closed | `plans/implementation/desktop-frontend-ide-foundation-corrective/001-desktop-connection-subscription-lifecycle-and-qualification.md` | Closure: `plans/closure/desktop-frontend-ide-foundation-corrective/001-status.md` (implementation `7b17808f`). Original ownership repair + live command-level qualification landed. Post-C001 source review found narrower transition races now owned by C002. |
| C002 atomic lifecycle transitions + subscription arming + window teardown | conditionally closed | `plans/implementation/desktop-frontend-ide-foundation-corrective/002-atomic-lifecycle-transitions-subscription-arming-window-teardown.md` | Closure: `plans/closure/desktop-frontend-ide-foundation-corrective/002-status.md` (implementation `dc32a6ad`). Production/race/live evidence landed. Hosted run `37062733033` failed in an unrelated PTY environment test; C003 owns green-run reconciliation without weakening that test. |
| C003 hosted-CI reconciliation + built-app visible-window strict closure | ready | `plans/implementation/desktop-frontend-ide-foundation-corrective/003-hosted-ci-visible-window-strict-closure.md` | No desktop code dependency blocker. Reconcile hosted root CI, add test-only WebdriverIO/Tauri built-app harness, run visible reconnect/Channel/reload/close/autostart/TUI-coexistence trajectory, then strict-close M003 and unblock M004. |

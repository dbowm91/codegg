# Desktop Frontend and IDE Foundation Milestone 004 — Desktop Session Control-Plane Vertical Slice

Status: ready for handoff (unblocked by strict M003 closure through closed C003; see `plans/closure/desktop-frontend-ide-foundation-corrective/003-status.md` §11)

Repository baseline reviewed: `3f222c2e5f957efdbb6a530276ff5b63a095e435`

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-004--desktop-sessioncontrol-plane-vertical-slice`

Hard dependency:

- M003 Tauri desktop shell + secure bridge must strict-close first.
- Current strict-closure handoff:
  `plans/implementation/desktop-frontend-ide-foundation-corrective/003-hosted-ci-visible-window-strict-closure.md`.

Operational dependency:

- M002 portable local transport must strict-close before Windows-specific M004 qualification can be claimed. Linux/macOS M004 implementation is not blocked by the Windows evidence item.

Applicable architecture/ADRs:

- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md`
- `plans/adrs/ADR-0006-project-channel-chat-access-policy.md`
- `plans/adrs/ADR-0007-shared-session-control-ownership.md`
- `architecture/desktop.md`
- `architecture/projection.md`
- `architecture/protocol.md`
- `architecture/client.md`
- `architecture/authorization.md`

Primary class: capability / frontend-controller integration

## 1. Objective

Prove the desktop architecture with one bounded, useful session/control-plane vertical slice:

- select a daemon-authorized project and one of its registered workspaces;
- list/open/create a session;
- consume the canonical session projection through a reusable non-TUI client driver;
- submit one prompt at most once;
- render bounded public assistant/tool/job/run/subagent activity;
- surface controller state and pending permission/question interactions;
- answer allowed permission/question requests through existing daemon authority;
- reconnect/resume/resync without stale delivery;
- coexist with a TUI client against the same daemon;
- close/reload the desktop without cancelling daemon-owned work.

This milestone closes the **desktop foundation**. It deliberately stops before editor buffers, repository browsing, graphical PTYs, file mutation UI, Git mutation UI, or general IDE functionality.

## 2. Research update since the original M004 plan

The original M004 direction remains valid, but several dependencies matured after that plan was first written.

### 2.1 Reuse `HeadlessProjectionConsumer`, not a desktop reducer

`codegg-protocol::projection::consumer::HeadlessProjectionConsumer` is now a production, transport-neutral, non-TUI consumer of the canonical projection protocol.

It already owns:

- capability response negotiation;
- session subscription request construction;
- canonical snapshot installation;
- `ProjectionReducer` application;
- duplicate/gap handling;
- cursor retention;
- reconnect resume request construction;
- replay/resync response handling;
- acknowledgement request construction;
- unsubscribe request construction;
- bounded artifact handle/read validation;
- visibility sanitization.

M004 SHOULD use this consumer as the projection-state core.

Do not create:

- a React/TypeScript projection reducer;
- a second Rust reducer in the desktop crate;
- a copy of TUI `ProjectionClientState`;
- a desktop-only snapshot/replay protocol.

`ProjectionClientController` remains the lower-level canonical controller primitive, but the headless consumer is now the more appropriate reusable composition point for a non-TUI desktop client.

### 2.2 `codegg-client` event lag is not currently explicit enough

Current `LocalSocketClient::subscribe()` adapts a bounded Tokio broadcast to an mpsc receiver. A broadcast lag currently only logs:

```text
native client event subscriber lagged
```

and continues.

That behavior is acceptable for advisory global catalog invalidations, but it is insufficient for a projection-primary session UI: dropping a private `ProjectionStreamEvent` without surfacing loss can leave a consumer waiting on a sequence that will never arrive.

M004 must make transport loss explicit.

Preferred additive client surface:

```rust
enum ClientEvent {
    Event(EventEnvelope<CoreEvent>),
    Lagged { dropped: u64 },
    Closed,
}
```

or an equivalent typed projection-driver event source.

Compatibility:

- existing `LocalSocketClient::subscribe()` may remain for current callers;
- the projection driver must consume a loss-aware surface;
- a lag signal forces authoritative resume/resync rather than continuing as if delivery were lossless.

### 2.3 Native projection transport already has response-before-live ordering

The daemon's local `/core` transport already owns projection subscriptions per connection and gates live projection delivery until the critical `ProjectionSubscribed`/resume response has been committed.

M004 should preserve that contract by installing its client-side event receiver **before** sending the subscribe request, then activating the headless consumer from the committed response.

Do not add another daemon-side ordering mechanism unless current tests demonstrate a defect.

### 2.4 Project-scoped workspace discovery is already available

Do not use global `WorkspaceList` for desktop project navigation.

Global `WorkspaceList` is intentionally LocalOwner/proven-local only and includes daemon-local workspace metadata. The desktop already has `ProjectList`; on selection it should call `ProjectGet`.

`ProjectDetailsDto` returns:

- the selected `ProjectSummaryDto`;
- bounded `workspaces: Vec<ProjectWorkspaceSummaryDto>`;
- session count;
- project health.

`ProjectWorkspaceSummaryDto` provides stable `workspace_id` and display name, plus optional canonical root.

Rules:

- renderer route identity is project id + workspace id + session id;
- canonical workspace root remains Rust-host data;
- renderer must never send a filesystem path to choose/create session authority;
- session creation may use the canonical root internally because current `SessionCreate` still carries `directory`, but the Rust host supplies it from the authorized `ProjectGet` result together with `project_id` and `workspace_id`.

### 2.5 Session creation now supports explicit project/workspace identity

`CoreRequest::SessionCreate` carries optional `project_id` and `workspace_id` in addition to the compatibility `directory`.

M004 must always populate explicit project/workspace ids for desktop-created sessions and use the matching canonical root resolved by Rust host state.

A returned session must be rejected/stale-dropped if its canonical binding does not match the captured desktop route generation.

### 2.6 Shared-session controller authority already exists

ADR-0007 implementation is closed.

The daemon has:

- turn-scoped controller leases;
- `SessionControlGet`;
- request/transfer/release/force-takeover operations;
- permission/question response checks tied to the owning session/turn/controller;
- privacy-safe denial behavior.

M004 consumes that authority; it does not invent a desktop control model.

### 2.7 Built-app WebDriver should be inherited from M003

C003 is intended to establish the checked-in Tauri/WebdriverIO built-app harness.

M004 should extend that harness rather than introducing Selenium, Playwright, a second Tauri driver stack, or a custom renderer-only E2E runner.

## 3. Core invariants

- The singleton daemon remains durable execution/control authority.
- Project/workspace/session identity is explicit and never inferred from renderer cwd/path.
- The desktop has no durable session/message store.
- Canonical projection state is reduced only by `codegg-protocol` projection code.
- Renderer state is presentation-only and cannot become projection cursor authority.
- Projection delivery loss becomes typed resync/recovery state; it is never silently ignored.
- Prompt submission is at-most-once per explicit renderer intent.
- Session creation followed by prompt submission is route-generation fenced.
- Permission/question response authority remains daemon-side.
- Observer/non-controller clients fail closed for mutations/control responses.
- Large outputs remain behind bounded projection/artifact handles.
- Old project/session/connection/projection generations cannot mutate the current view.
- Desktop reconnect gets a new connection generation and never treats an old subscription id as current.
- Desktop close/reload detaches frontend-owned state only; daemon-owned turn/job/run/PTY work survives.
- TUI + desktop can observe one session without duplicate durable state.
- The WebView receives no generic CoreRequest, filesystem, shell, process, Git, credential, or PTY authority.

## 4. Non-goals

- Monaco/editor buffer model.
- File explorer or direct filesystem browsing.
- Direct file edits/writes.
- Graphical PTY/terminal.
- Git mutation UI.
- Provider credential entry/rotation.
- Full team/chat administration.
- WorkOrder/task authoring parity.
- Audit UI.
- Plugin UI parity.
- Remote `/core` access.
- Web/mobile frontend.
- Desktop release signing/updater.
- Windows support claim before M002 closes.

## 5. Target client architecture

Add a reusable projection I/O driver in `codegg-client`, built around `HeadlessProjectionConsumer`.

Target shape:

```text
LocalSocketClient
    |
    | request/response + typed event stream
    v
SessionProjectionDriver
    |
    +-- HeadlessProjectionConsumer
    |     +-- canonical ProjectionReducer
    |     +-- cursor
    |     +-- snapshot
    |     +-- artifact handles
    |
    +-- connection generation
    +-- subscription id / stream id
    +-- ack cadence
    +-- resume/resync state
    +-- cancellation/task owner
    |
    v
immutable bounded projection updates
    |
Desktop Rust session controller
    |
typed Tauri Channel
    |
React presentation
```

### Driver ownership

The driver owns:

- one authenticated `LocalSocketClient` generation reference;
- one `HeadlessProjectionConsumer`;
- exact daemon `ProjectionSubscriptionId` and stream descriptor;
- loss-aware event receiver;
- ack/replay/resume/resync orchestration;
- explicit unsubscribe;
- driver cancellation/join;
- immutable bounded snapshot publication.

It does **not** own:

- React state;
- UI tabs;
- provider credentials;
- daemon session persistence;
- agent execution;
- project catalog persistence.

### Initial bound

M004 requires one active primary session projection per desktop renderer route.

Do not introduce arbitrary multi-session projection fanout yet. A future IDE shell can widen this deliberately.

## 6. Client event-loss corrective required inside M004

Add a loss-aware subscription surface to `codegg-client`.

Requirements:

- report exact normal `CoreEvent` envelopes;
- report broadcast lag/drop as a typed condition;
- report terminal closure;
- remain bounded;
- preserve existing request correlation and reader ownership;
- no unbounded buffering to “solve” lag;
- no automatic local event fabrication.

Projection-driver response to `Lagged`:

1. stop applying live events;
2. mark projection presentation `resyncing`;
3. use retained canonical cursor to issue `ProjectionResume { include_snapshot_if_resync: true }`;
4. accept `ProjectionReplay` through `HeadlessProjectionConsumer`;
5. if daemon returns `ProjectionResyncRequired`, atomically install authoritative snapshot when present or perform a fresh subscribe;
6. publish a new immutable current snapshot;
7. resume live delivery only for the current connection/subscription generation.

Unit tests must force receiver lag; logging alone is not acceptance.

## 7. Desktop Rust route/session controller

Extend the C002 host lifecycle model with a separate bounded session-view controller.

Do not overload the M003 project-catalog `SubscriptionRegistry` with projection state.

Route identity:

```text
DesktopRouteToken
  connection_generation
  route_generation
  project_id
  workspace_id
  session_id?
```

Session-view state may include:

- current `ProjectDetailsDto` summary;
- selected workspace id and Rust-only canonical root;
- bounded session summaries;
- active session id;
- projection-driver owner;
- renderer session-subscription generation;
- pending prompt intent;
- pending permission/question intent ids.

### Project selection

1. `ProjectList` populates the bounded picker.
2. `ProjectGet { project_id }` loads project workspaces.
3. renderer selects workspace by stable id.
4. Rust host keeps canonical root hidden from renderer authority.
5. `SessionList { project_id, ... }` loads bounded sessions.
6. any completion is applied only when route token/generation remains current.

### Session open

For existing session:

- issue `SessionAttach`/`SessionLoad` as required by current daemon semantics;
- verify returned session binding matches selected project/workspace;
- create projection driver;
- negotiate/subscribe;
- publish initial bounded presentation.

### Session create

Desktop renderer supplies:

- optional title;
- selected project/workspace route.

Rust host supplies:

- canonical workspace root from the already-authorized project detail;
- project id;
- workspace id.

Returned binding must match captured route.

## 8. Projection presentation contract

The renderer gets bounded presentation DTOs derived from the canonical snapshot.

At minimum:

- project/workspace/session identity;
- connection/projection state:
  `connecting | subscribing | attached | resyncing | disconnected | unavailable`;
- bounded visible messages;
- current/most-recent turn coarse state;
- streamed public assistant text;
- tool execution summaries;
- job/run/subagent summaries;
- pending permission/question summaries;
- current controller summary;
- truncation/artifact handles;
- current projection sequence/cursor metadata only as diagnostic/non-authority fields.

Do not surface hidden reasoning/private projection classes.

### Update cadence

Do not emit a React-wide deep snapshot for every token if the canonical projection produces high-frequency deltas.

Acceptable pattern:

- bounded snapshot replacement on attach/resync;
- coalesced visible text/progress updates;
- low-frequency summary updates;
- immediate critical permission/question/controller change;
- explicit resync/connection diagnostic.

One Rust-owned ordered projection source remains authoritative.

## 9. Prompt submission contract

The original plan assumed prompt submission could be composed directly from the desktop bridge. Current `TurnSubmit` still carries low-level model/agent/message fields, so M004 must explicitly avoid duplicating TUI/provider runtime composition in React.

### Required audit before implementation

Audit the current selected-session model/agent state and existing reusable constructors used by TUI/ACP.

Preferred outcome:

- a Rust-side reusable helper composes a valid `TurnSubmit` from:
  - session id;
  - captured user text;
  - current daemon/session model/agent selection;
  - bounded plan-mode setting;
- desktop renderer sends only the user intent and route token.

If the only way to submit requires copying TUI `App` state or provider-specific logic into `apps/desktop`, stop and register a shared frontend/domain helper plan instead of duplicating it.

### At-most-once intent

Host state:

```text
PromptIntent
  intent_id
  route_token
  text
  text_digest
  state: captured | creating_session | submitting | accepted | failed
```

Rules:

- one renderer click/Enter = one intent id;
- repeated UI action while same intent is in flight is coalesced;
- if no session exists, create session once, validate route, then submit once;
- switching project/workspace/session cancels frontend continuation only; committed daemon work is not rolled back;
- failure restores editable draft without fabricating a visible durable user message;
- request completion from stale route/connection generation is discarded.

## 10. Permission/question/controller path

### Source of truth

Pending permission/question presentation must come from canonical projection/authorized daemon state.

Controller state comes from `SessionControlGet` and/or the canonical public projection fields already emitted for session control.

### Renderer bridge

Renderer sends only:

- opaque pending id;
- allowed permission choice; or
- bounded question answers.

Do not send:

- principal id;
- client id;
- controller principal;
- project path.

Daemon parses scoped ids and revalidates owning session/turn/controller lease.

### One-shot UI behavior

While a response is in flight:

- disable duplicate response;
- keep pending item visible with pending state;
- on daemon success wait for canonical projection/state update;
- on denial/stale controller clear or refresh from authoritative state;
- observer/non-controller state exposes no enabled mutation control.

Tests must include controller loss between render and click.

## 11. Reconnect, resume, and daemon restart

On desktop transport reconnect:

1. C002 connection generation changes.
2. session controller marks old driver generation stale.
3. old driver stops and releases old subscription ownership.
4. reconnect negotiates projection capabilities on the new client.
5. retained headless cursor is used for resume when valid.
6. replay/resync response is accepted through `HeadlessProjectionConsumer`.
7. old subscription id is never reused as current connection ownership.
8. if resume cannot converge, fresh subscribe installs the authoritative snapshot.
9. pending permission/question/controller state is refreshed.
10. renderer receives one new route/projection generation.

On daemon restart:

- expect new daemon/client identity;
- attempt cursor resume only according to canonical protocol response;
- typed gone/resync/unavailable is rendered rather than guessed;
- daemon-owned work survives only to the extent the daemon's durable runtime contract supports; desktop must not fabricate continuity.

## 12. Artifact/detail reads

Use `HeadlessProjectionConsumer::artifact_read_request` and authorized `ProjectionArtifactRead`.

Requirements:

- opaque handle only;
- bounded range;
- project ownership validated by consumer/daemon;
- no path-based artifact reads;
- no renderer filesystem access;
- stale handles produce bounded typed failure.

M004 only needs enough artifact reading to inspect a truncated visible projection item. General file explorer belongs to M005/M006.

## 13. Bridge surface

Add narrow Tauri commands/channels. Exact names may differ, but intended capability classes are:

- project detail/select;
- session list/open/create;
- session projection start/stop;
- projection presentation subscription;
- prompt submit;
- permission respond;
- question respond;
- session control refresh/request if needed for presentation;
- artifact excerpt read.

Do not expose a generic `CoreRequest` invoke.

Every mutating command is route-token/generation fenced in Rust before request construction and again before result application.

## 14. Ordered work packages

### WP A — Loss-aware client event stream + projection driver

Deliver:

- typed lag/closed event surface in `codegg-client`;
- `SessionProjectionDriver`;
- `HeadlessProjectionConsumer` integration;
- capabilities/subscribe/replay/resume/ack/unsubscribe;
- exact subscription/stream filtering;
- cancellation/join ownership;
- forced-lag/resync tests.

Hard prerequisite for projection UI.

### WP B — Project/workspace/session route controller

Deliver:

- `ProjectGet` workspace selection;
- Rust-only canonical root;
- bounded `SessionList`;
- attach/open;
- explicit-identity `SessionCreate`;
- route tokens and stale-drop tests.

Do not use global `WorkspaceList`.

### WP C — Projection presentation + renderer session view

Deliver:

- bounded presentation DTO;
- snapshot/install/resync UI;
- visible message/activity summaries;
- artifact excerpt path;
- coalesced streaming cadence;
- no TypeScript reducer.

### WP D — Prompt composition + at-most-once submission

Deliver:

- reusable Rust-side TurnSubmit composition decision;
- explicit prompt intent;
- create-then-submit continuation;
- double-submit coalescing;
- stale route failure behavior.

If reusable composition cannot be achieved without TUI/provider coupling, stop and split the helper dependency.

### WP E — Controller/permission/question interaction

Deliver:

- controller summary;
- pending controls;
- permission/question response;
- one-shot coalescing;
- controller-loss/observer negative tests.

### WP F — Reconnect/restart/two-client recovery

Deliver:

- cursor resume;
- typed loss/resync;
- fresh subscribe fallback;
- TUI + desktop simultaneous session observation;
- desktop close/reload while turn continues.

### WP G — Built-app M004 closure trajectory

Extend the checked-in M003 WebdriverIO harness.

Use a deterministic in-repo model/runtime fixture; external provider availability must not be an acceptance dependency.

## 15. Deterministic model/provider fixture

M004 must not require a paid/external model API.

Use an existing fake/provider/test runtime if one already exercises `TurnSubmit` through the real daemon.

If none provides the required permission/question path, add a test-only deterministic runtime/provider fixture that can:

- accept one known prompt;
- emit public assistant text/progress;
- emit one tool/job/subagent-visible activity sequence as required for projection proof;
- trigger one deterministic permission or question;
- accept response;
- finish the turn.

Do not add a special production bridge or bypass daemon authorization merely for E2E.

## 16. Required deterministic tests

### codegg-client / projection driver

- normal capability + subscribe + live apply;
- event receiver lag forces resync;
- duplicate event remains idempotent;
- sequence gap forces resync;
- exact subscription/stream mismatch rejected;
- replay continuation;
- snapshot-required resync;
- ack cadence;
- unsubscribe;
- reconnect retains cursor but drops subscription id;
- closed transport publishes disconnected state;
- artifact handle/range bounds.

### Desktop route/session host

- project switch drops stale `ProjectGet`;
- workspace switch drops stale `SessionList`;
- session open binding mismatch fails closed;
- session create uses captured project/workspace id and Rust-resolved root;
- stale session-create result cannot bind;
- route close joins projection driver;
- connection generation change invalidates old projection owner.

### Prompt

- double submit coalesced;
- create-then-submit exactly once;
- session-create failure preserves draft;
- route switch after create but before submit prevents submit;
- reconnect during submit never fabricates a second turn;
- stale response cannot mark current intent accepted.

### Permission/question/control

- current controller can respond;
- observer cannot;
- stale controller cannot;
- controller loss between render/click denied and refreshes state;
- repeated click coalesced;
- cross-session pending id fails closed;
- renderer cannot supply principal/client identity.

### Projection presentation

- canonical golden snapshot fixture parity;
- private reasoning never rendered;
- high-frequency text updates bounded/coalesced;
- artifact truncation uses handle;
- resync atomically replaces presentation.

## 17. Integration trajectories

### TUI + desktop

One daemon:

- attach TUI to session;
- attach desktop projection to same session;
- submit one turn from authorized controller;
- both see canonical visible state;
- close desktop;
- TUI remains live and turn continues;
- reconnect desktop;
- desktop converges through resume/replay/snapshot.

### Project/session switching under latency

Force delayed:

- ProjectGet;
- SessionList;
- SessionCreate;
- projection subscribe.

Switch route while each is in flight and prove no stale completion mutates current route.

### Transport lag

Force local client event lag and prove:

- desktop enters resync state;
- no post-gap event is applied as contiguous;
- resume/replay or snapshot converges;
- renderer never sees silently divergent state.

## 18. Built-app WebDriver closure trajectory

Extend C003's real-app harness:

1. launch isolated daemon + deterministic provider/runtime;
2. keep TUI client attached;
3. launch desktop;
4. choose deterministic project;
5. choose workspace;
6. list/create/open session;
7. attach canonical projection;
8. enter and submit prompt;
9. observe visible assistant/progress/activity via projection;
10. respond to deterministic permission/question when authorized;
11. observe turn completion;
12. force desktop reconnect;
13. assert state converges with no duplicate prompt/turn;
14. force renderer reload;
15. assert session projection resumes/converges;
16. close desktop;
17. assert daemon/TUI/daemon-owned work remain responsive.

This is M004's primary user-visible closure proof.

## 19. Verification

Expected commands; record exact commands actually run:

```bash
cargo test -p codegg-client --locked
cargo test -p codegg --test projection_replay --locked
cargo test -p codegg --test projection_transport_real --features server --locked
cargo test -p codegg --test headless_projection_consumer --locked
cargo test -p codegg --test single_daemon_lifecycle --locked
scripts/verify.sh quick
scripts/check-desktop-boundary.sh

cd apps/desktop
npm ci
npm run typecheck
npm test
npm run bindings:check
npm run build
rustup run 1.90.0 cargo test --manifest-path src-tauri/Cargo.toml --locked
npm run test:e2e
```

Hosted root CI plus the path-gated desktop E2E workflow must be green on the closure revision.

## 20. Security review

Closure must explicitly prove:

- no generic CoreRequest bridge;
- no renderer filesystem/shell/process/network plugin authority;
- no renderer path used as project/workspace/session authority;
- session/project reads honor daemon authorization;
- projection subscription is connection-owned and session-authorized;
- observer cannot invoke turn/control mutations;
- permission/question response uses daemon controller lease;
- no private/reasoning projection content rendered;
- artifact reads use opaque authorized handles;
- no prompt body/provider secret logged by bridge diagnostics.

## 21. Protocol/storage impact

Expected:

- no storage migration;
- no CoreFrame version bump;
- no new desktop database;
- additive `codegg-client` event-loss API only;
- existing projection protocol reused;
- bridge DTO additions bundled with desktop and checked by `bindings:check`.

If a missing frontend-neutral projection field is genuinely required, add it through the canonical projection protocol with independent consumer tests. A desktop-only projection endpoint is a stop condition.

## 22. Documentation

Update as implementation lands:

- `architecture/desktop.md` — session controller, bridge inventory, projection lifecycle, control path;
- `architecture/client.md` — typed event-loss surface and projection driver;
- `architecture/projection.md` — only if shared driver/consumer contract needs clarification;
- `architecture/protocol.md` — only for additive protocol changes;
- desktop E2E developer docs;
- roadmap/registry/closure record.

## 23. Acceptance criteria

M004 closes only when:

1. desktop routes through explicit project/workspace/session ids.
2. project workspaces come from authorized `ProjectGet`, not global path enumeration.
3. projection state is consumed via `HeadlessProjectionConsumer`/canonical reducer.
4. native-client event lag becomes typed resync, never log-only loss.
5. subscribe/live/replay/resume/ack/unsubscribe lifecycle is bounded and tested.
6. prompt create/submit is at-most-once under double action, latency, route switch, and reconnect.
7. permission/question/controller authority fails closed for observer/stale controller.
8. public assistant/tool/job/run/subagent activity is visible through projection-derived presentation.
9. project/session/connection/projection generations reject stale async work.
10. reconnect/restart converges through canonical resume/resync semantics.
11. one daemon serves TUI + desktop without duplicate durable state.
12. desktop close/reload does not cancel daemon-owned work.
13. built-app E2E completes using a deterministic local fixture without external provider availability.
14. production WebView authority remains least-privilege.
15. no unresolved high/medium finding remains.

## 24. Stop conditions

Stop and re-plan if:

- M003 is not strict-closed;
- implementation requires importing TUI `App` state;
- prompt composition requires duplicating provider/agent-runtime logic in the renderer;
- a missing projection field is solved with a desktop-only endpoint;
- event loss is “handled” by increasing buffers without typed resync;
- permission/question response bypasses ADR-0007 controller ownership;
- renderer needs filesystem/shell/process permissions;
- editor/file/PTY UI becomes necessary for M004;
- remote/team desktop access is required for closure;
- Windows support is claimed before M002 closes;
- an incompatible CoreFrame/projection break is proposed.

## 25. Closure evidence

Create:

- `plans/closure/desktop-frontend-ide-foundation/004-status.md`

Record:

- implementation commits;
- `codegg-client` loss-aware event contract;
- projection-driver ownership diagram;
- evidence of `HeadlessProjectionConsumer` reuse;
- route/session controller state diagram;
- bridge commands/events;
- project/workspace path-authority negative proof;
- at-most-once prompt test matrix;
- permission/question/controller fixtures;
- lag/resume/resync/restart evidence;
- TUI + desktop trajectory;
- built-app E2E result;
- exact commands/CI runs;
- protocol/storage/MSRV/security impact;
- unresolved findings;
- disposition for M005 document/buffer planning.

## 26. Handoff note

Do not add an editor merely to make M004 feel like an IDE milestone.

M004's purpose is to prove that CodeGG can support a second rich frontend using the same daemon authority, canonical projection truth, and human-control semantics as the TUI. Once that is proven, M005 can audit the harder document/buffer/LSP ownership problem on a stable frontend foundation.

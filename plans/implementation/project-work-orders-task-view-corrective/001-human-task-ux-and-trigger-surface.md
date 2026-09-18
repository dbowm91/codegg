# Project Work Orders and Task View Corrective C001 — Human Task UX and Trigger Surface

Status: closed

Closure record: `plans/closure/project-work-orders-task-view-corrective/001-status.md`

Implementation commit: `1bd77c6d`

Repository baseline: `e013acd282afd63e7ca2211190a1342f76ae3e5d`

Source corrective roadmap:

- `plans/subsystems/project-work-orders-task-view-ux-corrective-addendum.md#7-corrective-milestone`

Original milestone and closure evidence corrected by this plan:

- `plans/implementation/project-work-orders-task-view/003-project-task-composer-and-view.md`
- `plans/closure/project-work-orders-task-view/003-status.md`
- `plans/implementation/project-work-orders-task-view/005-external-task-trigger-endpoint.md`
- `plans/closure/project-work-orders-task-view/005-status.md`
- `plans/implementation/project-work-orders-task-view/007-work-order-trajectory-and-recovery-qualification.md`
- `plans/closure/project-work-orders-task-view/007-status.md`

Applicable ADRs:

- `plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md`
- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`

Primary class: **capability / usability corrective**.

Hard dependencies: closed M003, M005, and M007 Project Work Orders milestones.

## 1. Objective

Correct the remaining human-facing fidelity gaps in the closed Project Work Orders capability without redesigning its durable architecture.

The implementation must make the originally requested workflow true end to end:

1. normal Session composition remains the default and bare `Tab` at root prompt focus cycles to Task composition;
2. a focused sequential queue editor supports direct `j/k` and Up/Down reorder of eligible future work through the existing revisioned lane/CAS service;
3. the Task scheduling flow can enable an external-trigger gate, create/bind the already-landed M005 task trigger, and show the authorized user the one-time bearer/invocation instructions through a transient secret-safe surface;
4. trigger setup failures and lost one-time responses are recoverable from the Task view without deleting the WorkOrder or adding a secret-read path.

No second scheduler, session type, trigger service, authorization framework, or durable workflow abstraction is permitted.

## 2. Corrective findings and why prior verification passed

### Finding A — Task mode is reachable by `Ctrl+G`, not the requested bare Tab

Current evidence:

- `src/tui/app/state/work_orders.rs` owns `ComposerMode::Session | Task`, correctly separate from `InputMode`.
- `src/tui/input.rs` / configurable action wiring bind `ToggleComposerMode` to `Ctrl+G`.
- bare `Tab` remains `SwitchAgent`; Shift+Tab retains permission-mode cycling.
- M003 closure explicitly treats this collision-avoidance choice as passing behavior.

Why M003 did not catch the requirement mismatch:

The M003 plan allowed a collision-free alternative if integrating Task into the Tab selector would disturb existing `SwitchAgent`. Its tests therefore verified *absence of collision* rather than the stronger product requirement that Tab itself reaches Task mode.

Corrective requirement:

At root prompt focus, bare Tab must own composer-mode cycling. There must be exactly one handler/action path for that behavior. `SwitchAgent` must remain available through the configurable action catalog and receive a deliberate non-Tab default only after a key-collision/terminal-encoding audit. Do not overload `InputMode`.

### Finding B — external trigger is backend-complete but human setup remains disabled

Current evidence:

- M005 landed verifier-only trigger persistence, `WorkOrderTriggerCreate/Get/List/Revoke`, expiry/max-fire/revocation/idempotency, and `POST /api/v1/task-triggers/{id}/fire`.
- `src/tui/app/state/work_orders.rs` still marks the external-trigger scheduling field as a disabled placeholder.
- M005 closure explicitly records that there is no TUI/CLI trigger surface and defers human trigger management to a later milestone.
- no later Project Work Orders milestone owns that deferred UI.

Why M005/M007 did not catch the capability gap:

M005 allowed protocol response alone as sufficient backend closure when no safe secret-display surface existed. M007 qualified trigger mechanics/replay/security and TUI state transitions, not the human trigger-creation path.

Corrective requirement:

The scheduling sheet must expose the existing trigger capability when server capabilities permit it, and the Task view must provide the minimum management/recovery actions needed for one-time-secret semantics.

### Finding C — queue reorder is modifier-only in Task view, not direct in the scheduling queue

Current evidence:

- Task view uses `j/k`/arrows for selection and `Shift+J/K` for waiting-lane reorder.
- scheduling-sheet Tab focus exists, but direct `j/k`/Up/Down queue movement is not the human interaction requested in the original brief.

Why M003 did not catch it:

M003 explicitly allowed a reorder modifier/reorder mode to avoid colliding with list navigation, so tests pin the implementation choice rather than the literal scheduling workflow.

Corrective requirement:

When the queue editor/reorder control in the scheduling flow owns focus, `j/k` and Up/Down must directly move eligible future entries. Outside that focused control, existing navigation conventions may remain, including Task-view `Shift+J/K` as an additional shortcut.

## 3. Invariants that cannot regress

- TUI is not durable WorkOrder authority and must not write WorkOrder/trigger tables directly.
- WorkOrder creation uses the canonical daemon `WorkOrderCreate`/M001 service path.
- Trigger management uses the existing M005 project-authorized management service; trigger fire remains the narrow unaffiliated HTTP POST capability.
- `WorkOrderCoordinator` remains the only release/materialization owner and all starting work still reaches `JobSubmissionService`.
- `InputMode::Insert | Normal` remains unchanged by Session/Task composer selection.
- Session composer remains default after startup/new session/new project tab unless an existing explicit product preference says otherwise; do not persist Task as the default merely because it was last used.
- Task-model preference remains separately scoped and unchanged by this corrective.
- prompt text remains editable/unsubmitted until WorkOrder confirmation succeeds.
- WorkOrder/session/job creation is not rolled back from the frontend after a late/stale success.
- lane reorder remains future/unclaimed only, running/claimed head remains pinned, and every durable move carries expected lane revision.
- trigger bearer cannot become a principal credential and cannot authorize any normal Core operation.
- trigger plaintext is never persisted or exposed to model/tool context.
- gate All/Any, repeat, delay/not-before, sequence-ready, model/policy snapshot, and worktree/session behavior remain as closed by M002-M007.

## 4. Explicit non-goals

- changing WorkOrder/Occurrence/SequenceLane schemas or lifecycle;
- adding a new task/session scheduler or background execution loop;
- adding arbitrary webhook request bodies, prompt templating, event payload mapping, or third-party integrations;
- adding trigger creation to the model-facing `work_order` tool;
- adding a generic secrets vault, secrets database, or repository-wide credential abstraction solely for TUI display;
- adding a trigger-secret read/recover API;
- making GET or query-string URLs fire tasks;
- adding unbounded recurrence;
- redesigning the global Workspace dashboard;
- redesigning permission/Automatic/Yolo/FullHost modes;
- drag-and-drop/mouse queue reordering;
- broad keymap cleanup unrelated to the Tab/agent-selection collision;
- new hosted CI lanes, live provider/network tests, or a new UI automation framework.

## 5. Expected production-code ownership

Primary expected surfaces:

- `src/tui/input.rs` and/or the current action/keybinding tables — root `Tab` composer-cycle ownership and `SwitchAgent` migration;
- `src/tui/app/input.rs`, `src/tui/app/state/prompt.rs`, `src/tui/app/state/work_orders.rs` — composer-mode routing and corrective state;
- `src/tui/components/dialogs/task_schedule.rs` — focused queue reorder controls and external-trigger scheduling field;
- `src/tui/components/dialogs/task_view.rs` — trigger setup/metadata/retry/rotate actions and one-time-secret presentation entry point if the view owns it;
- `src/tui/commands/work_orders.rs` — WorkOrder→trigger creation continuation, async/stale guards, recovery/rotation operations, project-capability checks;
- `src/tui/app/state/dialog.rs` / modal wiring only if a dedicated one-time trigger-secret dialog is the cleanest safe surface;
- `crates/codegg-protocol` only if an existing capability flag/DTO is insufficient to discover M005 support. Prefer no protocol change;
- `architecture/tui.md`, `architecture/work_orders.md`, relevant TUI skill/help/keybinding docs.

M005 core trigger persistence/service and HTTP fire route should require no semantic redesign. If a defect is found there, classify it separately rather than folding unrelated trigger-backend changes into this UX pass.

## 6. Target interaction design

### 6.1 Root composer Tab semantics

At root prompt focus:

```text
ComposerMode::Session --Tab--> ComposerMode::Task --Tab--> ComposerMode::Session
```

Requirements:

- Session is the normal/default state.
- Bare Tab changes only `ComposerMode`; it does not mutate Insert/Normal editing state, selected model, permission mode, project tab, active session, or prompt text.
- the same semantic action exists in both Insert and Normal keymaps unless the current architecture intentionally routes both through one root prompt action layer;
- when a modal owns focus (Task schedule, Task view, model selector, etc.), that modal's existing Tab behavior wins; root composer mode must not change underneath it;
- help text, keybinding editor/action catalog, and architecture docs must describe the new ownership consistently.

#### `SwitchAgent` migration

Do not silently remove agent switching. The implementation agent must inspect current default key assignments and terminal-normalization behavior, choose a portable collision-free default if one exists, and keep `SwitchAgent` in the configurable action catalog/help. Do not use a replacement sequence that common terminals normalize to bare Tab. If no safe universal default exists, leaving `SwitchAgent` unbound by default but visible/configurable is preferable to creating an ambiguous double binding; document that choice in closure evidence.

`Ctrl+G` may remain as a backward-compatible alias for `ToggleComposerMode` only if the keybinding system cleanly supports aliases and collision tests remain simple. It must not be the sole route to Task mode after C001.

### 6.2 Scheduling queue reorder semantics

The scheduling sheet should expose an explicit queue/lane control when sequential execution is selected.

When that control owns focus:

- `j` / Down moves the selected eligible future member one position later;
- `k` / Up moves it one position earlier;
- running/claimed/pinned predecessors cannot move and render a clear pinned marker;
- movement is reflected locally only as an optimistic *selection/intention* until daemon CAS succeeds; canonical order must be refreshed on conflict;
- durable mutation uses the existing `WorkOrderLaneReorder`/lane move operation with expected revision;
- stale revision yields the existing `queue changed; retry` behavior and reloads canonical lane state;
- no Job dependency row is edited.

If the scheduling sheet is editing placement for a WorkOrder not yet created, direct movement should select insertion position relative to the canonical lane preview and the eventual WorkOrder creation/attach must use the corresponding revision/position contract. Do not fabricate a durable WorkOrder merely to let the user move a preview row.

The full Task view may retain current `j/k` selection + `Shift+J/K` reorder. C001 does not require flattening all contexts to one key behavior.

### 6.3 External-trigger scheduling field

The current disabled external-trigger row becomes capability-aware:

- enabled only when the server advertises/supports M005 trigger management;
- disabled with an actionable compatibility explanation on older servers;
- participates as a normal nontrivial `ExternalTrigger` gate in `ReleaseGateSet`;
- respects All/Any when combined with delay/not-before/sequence gates;
- does not expose raw gate JSON.

The scheduling summary should use human language such as:

```text
Run after previous task AND external trigger · model foo/bar
```

or

```text
Run at/after 2026-09-18 09:00 -04:00 OR external trigger
```

### 6.4 WorkOrder + trigger creation continuation

Preferred existing-contract flow:

1. validate Task scheduling draft;
2. submit exactly one canonical `WorkOrderCreate`;
3. if WorkOrder creation fails, restore/preserve prompt exactly once and stop;
4. if it succeeds with external trigger enabled, issue one project-authorized `WorkOrderTriggerCreate` bound to that WorkOrder/gate;
5. use a bounded deterministic/idempotency key derived from the frontend request identity + durable WorkOrder identity so retry cannot accidentally mint multiple credentials;
6. if trigger creation succeeds, transition to one-time secret display;
7. if trigger creation fails, retain the already-created WorkOrder and expose `trigger setup incomplete` with a retry path;
8. never start a session/job directly from either step.

Do not delete the WorkOrder on trigger setup failure. The unsatisfied external gate makes the partial state safe; frontend rollback would violate daemon ownership and can race other viewers/edits.

If existing service semantics make it impossible to bind an unambiguous single external gate without a new protocol contract, stop and record that finding. Do not invent a second trigger creation path in TUI code.

### 6.5 One-time secret display

On successful `WorkOrderTriggerCreate`, show the returned bearer exactly once to the authorized human.

The surface must provide:

- public trigger ID;
- one-time bearer token;
- endpoint path `/api/v1/task-triggers/<id>/fire`;
- bounded curl/example invocation using `Authorization: Bearer ...` and optional `Idempotency-Key`;
- a warning that the token cannot be shown again and must be rotated if lost;
- close/acknowledge action.

If the TUI can reliably determine the configured externally reachable server base URL, it may show a full URL. Otherwise show the path or a `${CODEGG_BASE_URL}` template; do not guess `localhost` for a remote daemon.

Secret-safety rules:

- use a TUI-local opaque/redacted wrapper or equivalent so ordinary `Debug`/logs/state dumps do not print the bearer;
- do not derive serialization for transient secret-bearing state;
- do not place the bearer in `PromptState`, prompt history, transcript, notification history, audit/event metadata, WorkOrder summaries, dashboard rows, or model/tool messages;
- do not store it in `runtime_preferences` or any SQLite table;
- clear/drop transient secret state on dialog close, project/tab switch, reconnect, logout/authority loss, and TUI shutdown;
- clipboard integration is optional only if an existing safe clipboard path exists; do not add a new platform clipboard dependency solely for C001.

### 6.6 Stale/lost-response and recovery semantics

The one-time nature of M005 makes async route handling part of correctness.

Cases:

**A. WorkOrder response stale before trigger request starts**

- durable WorkOrder may exist;
- do not foreground or delete it;
- if the external trigger was not created, Task detail later shows setup incomplete and offers retry.

**B. Trigger create commits and response arrives on stale route**

- do not log or persist the secret to make it recoverable;
- drop foreground presentation according to route/generation rules;
- metadata later shows that an active trigger exists;
- because plaintext cannot be reconstructed, Task detail offers explicit rotation (`revoke` then `create`) to obtain a new one-time bearer.

**C. Trigger create returns a transient/authorization error without commit**

- WorkOrder remains intact;
- show setup-incomplete/denied state if current route is still valid;
- authorized retry is allowed and idempotent.

**D. Ambiguous timeout where commit status is unknown**

- list/get trigger metadata first rather than blindly creating again;
- if an active trigger exists but secret is unavailable, require explicit rotate to mint a new bearer;
- if no trigger exists, retry creation using the original idempotency key.

**E. Revocation/capability loss races**

- fail closed;
- no secret display after authority loss;
- preserve existing opaque-project privacy behavior.

## 7. Task-view trigger management

The Task view/detail for a WorkOrder with an external gate must expose the minimum metadata/actions needed to make the scheduling feature operable:

- status: not configured / active / revoked / expired / exhausted;
- trigger public ID where authorized;
- expiry/max-fire/fire-count metadata already permitted by M005 DTOs;
- create/retry when not configured;
- revoke when active;
- rotate/recreate when active but the human needs a new bearer;
- no `show secret` action after initial creation.

Rotation is an explicit user mutation:

1. revoke old trigger (idempotent/monotonic);
2. create replacement bound to the same WorkOrder external gate;
3. show new one-time bearer.

Do not revoke automatically just because the TUI lost the original response; another authorized user or automation may already possess/use that bearer. The human must choose rotation.

## 8. Authorization, team, and privacy behavior

- WorkOrder creation and trigger create/revoke use the existing project-scoped authorization descriptors; frontend capability checks are hints only.
- Viewers/read-only observers may see only metadata allowed by existing M005 list/get authorization and cannot create/revoke/rotate.
- an unauthorized caller must not learn whether an opaque trigger/WorkOrder exists beyond existing privacy-safe shapes;
- a trigger bearer displayed to one authorized user does not become a team credential and is never posted to project chat/presence;
- concurrent authorized trigger setup must converge via existing idempotency/binding rules rather than produce two effective credentials accidentally;
- concurrent lane reorder remains CAS-first-writer-wins with refresh for losers.

## 9. Storage, migration, protocol, and compatibility effects

### Storage

Expected: **none**. Reuse M001 WorkOrder/lane tables, M003 runtime preference fields, and M005 trigger/receipt tables.

A new schema field must not be added merely to remember whether the TUI showed a secret. Secret display is transient frontend state. Existing durable trigger metadata is sufficient to know whether a capability exists.

### Protocol

Expected: **none or additive capability-discovery only**. Reuse:

- `WorkOrderCreate`;
- lane list/reorder/move requests;
- `WorkOrderTriggerCreate`;
- `WorkOrderTriggerGet/List`;
- `WorkOrderTriggerRevoke`.

If current server capability negotiation does not expose trigger-management support, add the narrowest additive capability bit/version field needed; do not create parallel management operations.

### Compatibility

- old server lacking WorkOrders: retain existing M003 compatibility behavior;
- WorkOrder-capable server lacking M005 triggers: external-trigger row is visibly disabled;
- current server: row enabled and management flow active;
- current client against newer server: unknown additive trigger metadata continues to decode compatibly.

No protocol-version bump unless required by the repository's existing additive/breaking-change rules.

## 10. Ordered work packages

### A — Root composer selector/keybinding convergence

1. audit current `ActionKey`, `InputAction`, default keymaps, help entries, keybinding editor, and terminal normalization;
2. make root bare Tab dispatch one composer-cycle action (`Session <-> Task`);
3. preserve `InputMode` and prompt/model/policy state;
4. migrate `SwitchAgent` to a documented collision-free default if available and keep it configurable/discoverable;
5. ensure modal-local Tab focus consumes Tab before root routing;
6. update help/keybinding architecture docs and add collision/regression tests.

### B — Focused scheduling queue direct reorder

1. expose/correct queue editor focus/state in `TaskScheduleDraft`/dialog;
2. consume `j/k` and Up/Down as movement only while queue reorder owns focus;
3. enforce pinned/unclaimed eligibility in pure state and command layer;
4. submit existing lane CAS mutation/placement and refresh on conflict;
5. preserve Task-view navigation/Shift+J/K behavior unless a simpler coherent reuse emerges;
6. add render/narrow-terminal and contention tests.

### C — External-trigger composer enablement

1. capability-detect existing M005 trigger management;
2. enable external-trigger scheduling field when supported;
3. build/validate `ExternalTrigger` gate with existing All/Any logic;
4. extend create continuation so WorkOrder success conditionally invokes `WorkOrderTriggerCreate`;
5. use bounded idempotency identity and project/route guards;
6. represent partial setup truthfully in current Task projection/detail state.

### D — One-time secret display and recovery/rotation

1. add/reuse focused modal state for one-time bearer + invocation example;
2. make secret-bearing state non-persistent and redacted from ordinary debug/log paths;
3. clear/drop on close/reconnect/scope loss;
4. add Task-detail trigger metadata/actions (retry, revoke, rotate; never reread secret);
5. implement ambiguous/stale response reconciliation through trigger metadata before retry;
6. test authorization loss and team concurrency.

### E — End-to-end regression qualification and docs

1. run a human-flow fixture: root Session → Tab Task → prompt → Enter → schedule external trigger → confirm → WorkOrder + trigger → one-time bearer → fire POST → occurrence materializes into ordinary session/job;
2. run the same flow with sequential placement/direct queue reorder;
3. inject stale create/trigger completions and ambiguous trigger-create timeout;
4. rerun M003, M005, M007 and ownership/security guards;
5. update architecture/help/skills and produce closure evidence mapping all three corrective findings.

## 11. Required focused tests

### Composer/keymap

- Session is default composer mode.
- bare Tab at root prompt toggles Session→Task→Session in Insert mode.
- same semantic behavior exists in Normal mode without changing `InputMode`.
- prompt contents/cursor/model/permission/project/session remain unchanged across composer Tab.
- modal TaskSchedule Tab moves modal focus and does not toggle composer mode.
- no duplicate bare-Tab action exists after key normalization.
- `SwitchAgent` remains present in action catalog/help and is reachable by its post-migration binding/configuration.
- optional `Ctrl+G` alias, if retained, produces identical composer transition without becoming a competing state owner.

### Queue

- focused queue control `j`/Down moves eligible future row later.
- focused queue control `k`/Up moves it earlier.
- pinned running/claimed predecessor never moves.
- boundary movement is no-op with stable selection.
- stale expected lane revision causes refresh + `queue changed; retry`, no local speculative durable order.
- two concurrent reorder attempts admit one CAS winner.
- scheduling placement does not create a fake session or Job dependency.

### Trigger scheduling/management

- trigger option disabled on server without M005 capability and enabled on current server.
- enabled option creates `ExternalTrigger` gate; All/Any combinations preserve all selected gates.
- WorkOrder failure performs zero trigger create.
- WorkOrder success + trigger success shows one-time secret surface exactly once.
- trigger create uses existing authorized project scope and deterministic bounded idempotency key.
- trigger create failure leaves WorkOrder intact and surfaces setup-incomplete state.
- retry with no durable trigger converges to one trigger.
- ambiguous timeout performs metadata reconciliation before any new credential creation.
- stale/lost successful response does not persist/log secret; Task detail shows active metadata and explicit rotate action.
- rotate revokes old capability then creates one replacement and displays only the new secret.
- unauthorized/viewer create/revoke/rotate fails closed.
- project/tab/reconnect stale completion never displays a bearer in the wrong scope.
- bearer never appears in debug-formatted Task view state, transcript/history, WorkOrder/dashboard DTOs, audit/event test captures, or model-facing tool results.
- existing POST fire route with displayed bearer latches only the trigger gate and materializes through coordinator once other gates permit.

### Regression

- ordinary Session Enter behavior unchanged.
- Task model preference remains separate from ordinary session model preference.
- `/tasks`, `/task`, `/schedules`, `/workspace` behavior unchanged except new trigger actions where relevant.
- existing TaskTool vs `work_order` tool separation remains intact.
- M005 trigger replay/revoke/expiry/max-fire tests remain green.
- M007 15-plan/recovery/ownership trajectory remains green.

## 12. Static guards and observability

Reuse and keep green:

```bash
python3 scripts/check_authorization_matrix.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_task_trigger_boundaries.py
python3 scripts/check_work_order_coordinator.py
python3 scripts/check_tui_project_authority.py
bash scripts/check-core-boundary.sh
```

Prefer unit/integration tests for keybinding and secret-display behavior rather than adding a new permanent static script. Extend `check_task_trigger_boundaries.py` only if the new TUI path creates a concrete secret-leak boundary that the existing guard can check narrowly without becoming a generic source scanner.

Observability may record structural trigger ID/state/action and WorkOrder identity according to existing policy. It must never record the bearer token, authorization header, prompt content, or generated curl line containing the token.

## 13. Required verification

Focused first:

```bash
cargo test -p codegg --lib -- tui::commands::work_orders
cargo test -p codegg --lib -- tui::app::state::work_orders
cargo test -p codegg --lib -- tui::components::dialogs::task
cargo test --test work_orders_m003_foundation 2>/dev/null || true
cargo test --features server --test work_orders_m005_trigger
cargo test --test work_orders_m007_trajectory
cargo test --test tui_project_tabs
cargo test --test tui_project_picker
cargo test --test tui_project_routing
python3 scripts/check_task_trigger_boundaries.py
python3 scripts/check_work_order_coordinator.py
python3 scripts/check_tui_project_authority.py
python3 scripts/check_authorization_matrix.py
python3 scripts/check_execution_ownership.py
bash scripts/check-core-boundary.sh
```

The implementation agent must replace the optional/nonexistent `work_orders_m003_foundation` literal with the actual current M003/TUI test targets discovered in the repository and record that mapping in closure evidence; do not create a test target solely to make the command exist.

Broad local posture:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/verify.sh quick
```

Do not add a new CI lane. Existing hosted CI may be reported if it runs naturally, but lack of a new hosted run is not a reason to invent additional verification infrastructure.

## 14. Documentation updates

At minimum reconcile:

- `architecture/work_orders.md` — human external-trigger setup/recovery and C001 UX correction;
- `architecture/tui.md` — root Tab composer ownership, migrated `SwitchAgent`, modal-local Tab behavior, focused queue reorder, trigger secret dialog/state;
- `.opencode/skills/tui/SKILL.md` if it documents relevant keymaps/dialogs/commands;
- user-facing help/default keybinding tables;
- corrective addendum and implementation-plan statuses during closure;
- `plans/registry.md` during closure.

Do not rewrite M003/M005/M007 closure records to pretend these behaviors were present at their original closure. The C001 closure must reference those records and state exactly what changed.

## 15. Acceptance criteria

C001 is complete only when all of the following are demonstrated against the landed implementation, not inferred from isolated helpers:

- root prompt starts in Session mode and bare Tab reaches Task mode;
- agent switching remains an intentional, discoverable configurable action after Tab migration;
- Enter in Task mode still opens the scheduling sheet and default zero-delay behavior remains immediate;
- focused sequential queue editing directly supports `j/k` and Up/Down reorder of eligible future entries with pinned-head/CAS safety;
- external trigger can be selected from the human scheduling sheet on a capable server;
- confirmation creates one canonical WorkOrder and one bound trigger capability without creating sessions/jobs directly in TUI code;
- the one-time trigger bearer/invocation is shown to the authorized human and is not durably recoverable or leaked elsewhere;
- external POST fire using that bearer releases only its gate and execution still materializes through `WorkOrderCoordinator`/`JobSubmissionService`;
- trigger setup partial failure, ambiguous timeout, stale response, authority loss, and lost secret all have deterministic safe recovery behavior;
- explicit rotation is the only way to obtain a replacement bearer after the one-time display is lost;
- existing WorkOrder backend, agent tool, Workspace dashboard, permission, model/policy, restart, security, and 15-plan trajectory regressions remain green;
- closure evidence includes a requirement-to-evidence matrix for Findings A-C and classifies any residual limitation.

## 16. Stop conditions

Stop and split/register a new plan instead of expanding C001 if implementation requires any of the following:

- a new scheduler/executor/background loop;
- a second WorkOrder or trigger persistence owner;
- a breaking protocol redesign;
- a new storage migration whose only purpose is frontend convenience;
- an atomic cross-resource WorkOrder+trigger public contract that cannot be expressed safely with existing operations;
- a new general-purpose secrets/credential framework;
- model-facing access to trigger credentials;
- a broad keymap redesign beyond Session/Task/agent selector ownership;
- changes to WorkOrder release, repeat, worktree, session, or job semantics not directly required by an identified defect.

If a pre-existing M005 backend correctness/security defect is found, stop that portion and create a dedicated trigger-backend corrective plan with its own evidence rather than masking it as UX work.

## 17. Closure evidence required

The closure record must include:

- implementation commit(s)/PR(s);
- explicit Findings A/B/C requirement-to-evidence matrix;
- before/after keybinding table including Tab, modal Tab, `SwitchAgent`, and any retained alias;
- end-to-end Task-with-trigger request trace showing WorkOrder create → trigger create → one-time display → POST fire → coordinator materialization, with secret redacted in the record;
- partial-success/stale/ambiguous-timeout/rotation evidence;
- proof that trigger bearer does not appear in persistence/log/audit/event/model-facing projections;
- lane direct-reorder and CAS contention evidence;
- authorization/privacy evidence for viewer/authority-loss/team cases;
- exact focused and broad verification commands/results;
- migration/protocol statement (expected none; document any additive capability discovery change);
- documentation changes;
- known limitations and severity classification;
- recommendation: closed, conditionally closed, or another corrective pass required.

# Multi-Project TUI Frontend Convergence M007 — Modal and Focus State Convergence

Status: implemented

Repository baseline: `98bc89fa613f5a1390202a90b497f59d5732d431`

Source roadmap:

- `plans/subsystems/tui-project-sessions-frontend-convergence-corrective-addendum.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable ADRs: none.

Primary class: invariant / polish

Closure record to create: `plans/closure/tui-project-sessions/007-status.md`

## 1. Objective

Establish one writable owner for live modal/dialog component state and one coherent focus contract. Remove the normal clone-and-resynchronize pattern between `dialog_state`, `ui_state.dialog`, and `FocusManager`, reconcile the overlapping `Dialog`/`DialogType` discriminators enough that open/render/update/close has one authority, and correct keyboard focus traversal including reverse wrapping.

This is not a visual redesign. It is a state-ownership correction required before additional sidebar/agent-tree keyboard work and before broad physical decomposition of `App`.

## 2. Why this milestone is ready

Modal/focus behavior is frontend-local and does not depend on M005. Existing dialogs already implement the `Component` trait and `FocusManager` already centralizes top-modal input/render dispatch. The defect is that component instances/state are also stored and mutated elsewhere. The work can therefore converge existing seams without a new protocol or daemon dependency.

## 3. Current implementation evidence

Before editing, build a modal ownership census covering:

- `src/tui/app/types.rs::Dialog` and every variant;
- `src/tui/components/component.rs::DialogType`, `From<DialogType> for Dialog`, modal classification, and variants with no `Dialog` counterpart;
- `src/tui/components/component/focus.rs`: component stack, duplicate suppression, `replace_top_dialog`, `focus_index`, Tab/Shift-Tab behavior;
- every `DialogState` field that stores a concrete dialog component or mutable dialog payload;
- `ui_state.dialog` reads/writes and render decisions;
- helpers such as `open_info_dialog`, `show_short_or_info`, `close_dialog`, provider/model/session/theme/research/security/run/plugin modal open/update paths;
- asynchronous completion handlers that mutate a stored dialog and then clone/replace it in `FocusManager`;
- remote/plugin UI dialogs and generic `InfoDialog` use for Collaborators/ProjectChat;
- mouse hit-test/selection synchronization.

For each modal family record: discriminator, live state owner, open path, update path, render path, close path, async update path, and focusable count.

## 4. Invariants that must not regress

- Exactly one live writable state instance exists for each open modal.
- Modal keyboard input is handled before underlying prompt/global input and does not leak through when consumed.
- Closing the top modal exposes the previous modal correctly; nested modal behavior remains deterministic.
- A modal update from an async completion cannot mutate an unrelated modal of the same broad info type.
- Permission/question dialogs retain fail-closed behavior and cannot lose pending authorization state through presentation cleanup.
- Observer/project-chat/collaborator dialogs remain read-only where currently required.
- Plugin UI effects remain bounded and source-filtered; plugin dialogs cannot acquire broader TUI authority.
- Existing dialog appearance/content semantics remain compatible unless a current inconsistency is the defect being fixed.
- Forward and reverse traversal use the component's current `focusable_count` and wrap symmetrically.

## 5. Scope

### In scope

- Selecting one canonical modal-state ownership model using existing `Component`/`FocusManager` machinery.
- Removing or narrowing concrete dialog copies from `DialogState` where they are redundant.
- Making `ui_state.dialog` derived/read-only compatibility state, or removing it from paths where the modal stack is authoritative.
- Converging `Dialog`/`DialogType` to one canonical discriminator where practical; otherwise defining an explicit one-way compatibility adapter with no second writable authority.
- Replacing `replace_top_dialog` synchronization with direct mutation of the canonical live component/state.
- Focus index ownership per active modal and correct Shift-Tab wrapping.
- Regression tests for open/update/render/close/nesting, async updates, mouse selection, permission/question handling, and generic info dialogs.

### Explicitly out of scope

- Rewriting every dialog widget API.
- New modal framework/dependency.
- Sidebar keyboard focus (M009), except shared focus primitives needed by both.
- App physical decomposition (M008).
- Changing daemon permission/question semantics.
- Visual/theme redesign.

## 6. Required production changes

### Core/domain, storage, protocol

No core/storage/protocol changes expected. Modal state is frontend presentation state. Permission/question DTOs remain daemon-owned data projected into a modal component.

### Runtime/concurrency

Asynchronous handlers must update modal state by a stable identity/type/request-generation reference, not by mutating a detached clone and replacing the rendered copy. If a modal has closed or been replaced when completion arrives, stale completion must be ignored or routed to non-modal state according to current semantics.

### Frontend ownership model

Preferred direction:

```text
FocusManager / ModalStack
  owns Box<dyn Component> live instances
  + modal identity/discriminator
  + per-top-component focus index derived/set on component

App/DialogState
  owns non-visual pending domain data and async request state
  but not a second mutable copy of the rendered modal
```

An alternative central `ModalStateStore` with the stack holding stable handles is acceptable if it is materially simpler for typed async updates. What is not acceptable is two independently mutable component copies requiring synchronization.

`Dialog` and `DialogType` should not both be used as peer authorities. Preserve public/internal compatibility adapters only where needed. Generic info dialog subtypes may remain content kinds, but their modal identity/close behavior must be unambiguous.

### Security/authorization

Do not move pending permission/question authority into a presentation-only object if that would allow close/drop to resolve or lose the daemon request. Keep domain pending state separate and explicit.

### Documentation/static guards

No new static guard is required unless the implementer can enforce a simple durable invariant such as prohibiting concrete dialog component fields in the non-modal state container. Prefer unit/integration tests over source scanners here.

## 7. Ordered work packages

### Work package A — Modal ownership matrix

Create the census described in section 3 and select the canonical owner. Identify the smallest compatibility layer needed for `Dialog`/`DialogType` callers.

Acceptance evidence: closure record contains before/after ownership matrix and no modal has two writable live component instances after migration.

### Work package B — FocusManager/state API

Add only the primitives needed to mutate/query canonical top/matching modal state safely. If downcasting is needed, expose a narrow typed helper using the existing `Any` bound rather than leaking the stack. Move/reset focus index correctly on push/pop and make reverse traversal `(index + count - 1) % count` or equivalent.

Acceptance evidence: Tab and Shift-Tab wrap across 0/1/N focusable controls; push/pop restores valid focus.

### Work package C — Migrate stateful dialogs

Prioritize dialog families currently using clone/`replace_top_dialog`, then model/session/theme/import/research/security/run/provider/generic info/plugin paths. Remove redundant component fields after callers migrate. Keep non-visual request/pending data in `DialogState` when semantically appropriate.

Acceptance evidence: `replace_top_dialog` has no production caller and is removed, or any retained caller has an explicit compatibility-only reason and no detached writable clone.

### Work package D — Discriminator convergence

Make one discriminator authoritative for open/close/render. Remove impossible mappings such as a modal type converting to `Dialog::None` from normal control flow, or make the conversion explicitly compatibility-only and unused for authority. Preserve generic info subtype labels separately.

Acceptance evidence: every open modal has one canonical type and close path; tests enumerate supported modal families.

### Work package E — Async/mouse/permission regression

Exercise delayed async dialog updates, click selection, nested modal close, permission/question pending flows, Collaborators/ProjectChat generic info views, and plugin dialogs.

Acceptance evidence: stale updates do not resurrect closed modals; keyboard/mouse mutate the same live state.

## 8. Failure, cancellation, restart, and contention semantics

Closing a modal cancels only the async request states the current behavior designates as modal-owned; it must not cancel daemon jobs merely because a view closes. A delayed completion for a closed/replaced modal is discarded or updates non-modal cached state, never recreates a stale component silently.

No modal state is persisted across process restart unless already represented in the safe tab manifest, which this milestone does not expand. Permission/question durable truth is recovered from daemon projections, not modal persistence.

Rapid opening of the same modal type follows one deterministic rule: focus/update the existing instance or reject duplicate push. It must not create hidden duplicate state.

## 9. Compatibility and migration

No persisted migration. Existing component implementations may continue to expose `dialog_type()` during transition. Existing callers of `Dialog` may be migrated gradually within this milestone; if a public path requires the enum, keep a facade conversion but prevent it from controlling live state independently.

## 10. Required tests

### Focused unit tests

- FocusManager push/pop/nested restore;
- Tab and Shift-Tab wrapping for 0/1/3 controls;
- duplicate modal behavior;
- typed canonical state mutation if introduced;
- discriminator conversion/identity exhaustive test.

### Integration tests

- open -> async update -> render -> close for stateful dialog families;
- close before delayed completion does not resurrect modal;
- generic Collaborators/ProjectChat info dialogs close correctly;
- plugin dialog and mouse selection update canonical state;
- permission/question flow retains pending domain state.

### Restart/recovery tests

- reconnect/reprojection can reopen required permission/question UI from canonical daemon state without stale presentation state.

### Contention/cancellation tests

- rapid open/close/open and nested dialogs preserve deterministic top focus;
- async completion for prior request cannot mutate replacement modal.

### Security/negative tests

- modal key consumption cannot submit prompt underneath;
- observer modal paths cannot expose mutation action through focus changes.

## 11. Required verification commands

```bash
cargo test -p codegg --lib -- tui::components::component
cargo test -p codegg --lib -- tui::components::dialogs
cargo test --test tui
cargo test --test tui_render
# focused permission/observer/collaboration TUI tests affected by migration
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Use exact existing target names at implementation time. No new CI lane.

## 12. Documentation updates

- `architecture/tui.md`: canonical modal/focus ownership and async update behavior.
- `.opencode/skills/tui/SKILL.md`: component/focus guidance and source shape.
- comments in `component.rs`/`focus.rs` describing ownership.
- remove stale clone/synchronization comments.

## 13. Acceptance criteria

M007 closes when every normal modal has one writable live state owner, `replace_top_dialog`-style synchronization is eliminated from production behavior, one discriminator controls lifecycle, reverse/forward focus wrap symmetrically, delayed updates cannot resurrect or mutate the wrong modal, and permission/observer/plugin semantics remain intact.

## 14. Stop conditions

Stop if:

- convergence requires changing permission/question durable ownership;
- a new framework is proposed solely to avoid migrating existing components;
- typed updates would require unsafe downcasts or broad `Any` exposure rather than a narrow API;
- visual redesign or sidebar work begins to dominate this milestone;
- current HEAD already has one-owner modal state and the cited duplicate paths are gone.

## 15. Closure evidence required

Include implementation commits, modal ownership census/matrix, before/after canonical owner, `replace_top_dialog`/duplicate-state search results, exhaustive modal discriminator evidence, focus wrapping tests, delayed-completion tests, permission/question/observer/plugin regression results, focused/broad verification run, and severity-classified residual findings.

## 16. Handoff notes

Implementation is complete. Closure evidence is recorded in
`plans/closure/tui-project-sessions/007-status.md`. M008 and M009 may now be
handed off; M008 remains sequenced after the closed M005-M007 contracts, while
M010 remains technically ready with M009 as a soft preference.

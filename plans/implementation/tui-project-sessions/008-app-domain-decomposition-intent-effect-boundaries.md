# Multi-Project TUI Frontend Convergence M008 — App Domain Decomposition and Intent/Effect Boundaries

Status: implemented

Repository baseline: `98bc89fa613f5a1390202a90b497f59d5732d431`

Source roadmap:

- `plans/subsystems/tui-project-sessions-frontend-convergence-corrective-addendum.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md#5-milestone-sizing`

Relevant precedent:

- `plans/subsystems/post-audit-maintainability-surface-roadmap.md` M003/M004 physical-decomposition rules.
- `plans/subsystems/post-implementation-maintainability-closure-roadmap.md` M001 responsibility-oriented decomposition rules.

Applicable ADRs: none. This milestone must preserve existing frontend/core ownership.

Primary class: polish / maintainability

Closure record to create: `plans/closure/tui-project-sessions/008-status.md`

## 1. Objective

After M005-M007 establish correct active-context, prompt-continuation, and modal ownership contracts, physically decompose the oversized TUI `App` implementation and command-dispatch surface into responsibility-oriented modules. Make the directionality between synchronous component/user intents (`TuiMsg` or successor) and asynchronous/runtime app commands/completions (`TuiCommand` or successor) explicit, removing accidental duplicate representations where safe without introducing a new state-management framework.

The goal is locality and testability, not a numerical file-size target.

## 2. Why this milestone is blocked/readiness condition

At the audit baseline `src/tui/app/mod.rs` is about 722 KiB and roughly 15K lines. It spans app composition, render preparation, modal orchestration, prompt/session lifecycle, remote/plugin effects, project/session helpers, input handling, message projection, status/sidebar shaping, and a very large `TuiCommand` enum. `src/tui/runtime/command_dispatch.rs` is also a broad switchboard.

Physical decomposition before M005-M007 would simply distribute transitional cwd/session/modal ownership across more files and make subsequent correction harder. M008 becomes ready only after those three contracts close.

## 3. Current implementation evidence

Before editing, produce a responsibility census of `src/tui/app/mod.rs`, `src/tui/runtime/command_dispatch.rs`, `src/tui/app/types.rs`, and `src/tui/runtime/*`. Group top-level fields/functions/impl blocks/dispatch arms by stable responsibility, at minimum:

- construction/composition/shutdown;
- render layout and view-model preparation;
- raw input -> `InputAction` -> component/global handling;
- project/tab/session selection and route validation;
- prompt/turn submit lifecycle;
- modal open/close/update;
- remote projection/reconnect handling;
- plugin UI effect application;
- provider/model/agent selection;
- sidebar/status/header projection shaping;
- shell/terminal/run/research/memory/LSP command completion routing;
- message/history/search presentation helpers;
- testing-only constructors/helpers.

Separately inventory `TuiMsg` and `TuiCommand` variants by origin and direction:

```text
component/user input -> synchronous UI intent
app -> async effect request
async task/core event -> completion/result
completion -> synchronous state mutation
```

Mark variants that represent the same logical action in both enums and determine whether that is a required adapter boundary or unnecessary duplication.

## 4. Invariants that must not regress

- `App` remains the TUI composition root; no second frontend coordinator/state store is introduced.
- M005 explicit project/workspace context remains the only project-scoped dispatch authority.
- M006 prompt/session-create continuation remains nonblocking and route-generation safe.
- M007 modal stack/state remains one-owner.
- `CoreClient`, daemon authorization, projection replay, scheduler, worktree, provider, and durable run/session owners remain unchanged.
- Render functions perform no network/filesystem/process work.
- Async tasks return typed completions; no mutable `App` reference crosses an await boundary.
- Cancellation, reconnect epochs, request generations, tab routing, observer restrictions, and permission/question semantics remain unchanged.
- Public/test import paths are preserved through re-exports where there is real compatibility value.
- Moving code must not change error text/protocol values solely as cleanup.

## 5. Scope

### In scope

- Responsibility map and extraction of coherent `App` implementation families into a shallow module tree.
- Moving `TuiCommand`/`TuiMsg` definitions to clearer type modules if helpful.
- Clarifying and documenting intent/effect/completion directionality.
- Removing duplicate message variants only where there is one clear source/consumer and no compatibility reason.
- Reducing pass-through helper chains and overly broad imports exposed by movement.
- Focused tests colocated with extracted responsibilities where useful.

### Explicitly out of scope

- Adopting a third-party event/state framework or a custom TEA runtime.
- Rewriting every component to a new trait.
- New features from M009/M010.
- Renaming every `TuiMsg`/`TuiCommand` variant for aesthetics.
- New protocol/storage types.
- Dependency injection framework, service locator, actor model, or message bus.
- Line-count/complexity CI gates.

## 6. Required production changes

### Target source shape

The exact modules follow the census. A plausible shallow shape is:

```text
src/tui/app/
  mod.rs                 # App type/composition + narrow facade
  types.rs               # stable app-facing types/intent/effect enums
  render.rs              # render composition/view-model preparation
  input.rs               # App-level input routing after component handling
  modal.rs               # M007 modal lifecycle integration
  project_session.rs     # active tab/session transitions and route helpers
  prompt_turn.rs         # M006 submit continuation + turn presentation
  projection.rs          # remote/projection/reconnect application
  plugin_ui.rs           # plugin UI effect application
  presentation.rs        # status/sidebar/header shaping if cohesive
  state/...
```

Names are illustrative. Prefer 5-9 coherent modules over dozens of tiny files. Do not extract modules that only contain pass-through wrappers.

### Intent/effect contract

Document one directional rule. For example:

- `TuiMsg`: synchronous component/user UI intent, never a daemon completion;
- `TuiCommand`: app/runtime effect requests and async completions delivered on the TUI command channel.

If current reality supports a better distinction, use it. The important requirement is that a new contributor can determine which enum to add to without searching the entire app.

Where the same action appears in both enums, retain a mapping only if it crosses a real component -> app or async -> app boundary. Otherwise collapse it to the canonical direction. Do not create generic `Action<T>` abstractions.

### Storage/protocol/runtime/security

No storage/protocol change expected. Movement must preserve all existing security/authorization/cancellation behavior. No new background tasks.

### Documentation

Update `architecture/tui.md` and `.opencode/skills/tui/SKILL.md` with the final responsibility map and intent/effect rules. Source-layout documentation should name stable ownership domains rather than line numbers.

## 7. Ordered work packages

### Work package A — Responsibility and message-flow census

Record module-sized responsibility clusters and enum variant directionality. Identify public/internal callers and likely merge-conflict hotspots. Reject proposed extraction units that cannot be described independently.

Acceptance evidence: before/after responsibility map in closure; each target module has one sentence of ownership.

### Work package B — Extract low-coupling pure/presentation domains

Move render/view-model/presentation helpers and similarly cohesive pure code first. Preserve function bodies where possible; update tests/imports without semantic edits.

Acceptance evidence: render regression tests unchanged; no new IO in render code.

### Work package C — Extract lifecycle domains

Move M005 project/session context helpers, M006 prompt-turn lifecycle, and M007 modal lifecycle into their final homes without reworking their behavior. Keep route/request state ownership explicit.

Acceptance evidence: M005-M007 focused suites remain green.

### Work package D — Command dispatch/domain routing

Split `command_dispatch.rs` only along actual command families if that improves locality. Keep one obvious top-level dispatch entrypoint. Family modules may apply/start operations, but must not create a second router or duplicate command matching.

Acceptance evidence: exhaustive/representative dispatch tests still reach the same handlers; no command becomes unreachable.

### Work package E — Intent/effect cleanup

Move enum definitions if useful, document directionality, and remove a small set of proven duplicate variants/adapters. Preserve stable names where tests/plugins/internal callers depend on them. Add compile-time/exhaustiveness tests where practical.

Acceptance evidence: no ambiguous peer ownership between the two message enums; mappings are explicit and minimal.

### Work package F — Dependency-direction cleanup/docs

Review imports for cycles, `super::*`, child modules reaching into unrelated app internals, and pass-through wrappers. Keep `App` facade methods when they define a useful stable boundary; remove wrappers that only preserve the monolith shape.

Acceptance evidence: final dependency graph is shallower/easier to explain, not merely more files.

## 8. Failure, cancellation, restart, and contention semantics

This milestone changes none of these semantics. Moving lifecycle code must keep request IDs, reconnect/view epochs, route tokens, cancellation handles and task ownership unchanged. Do not reorder state mutation around daemon requests or completion publication merely to satisfy module borrowing.

If extraction would require cloning mutable state into background tasks, changing lock/transaction ordering, or widening a child module to global mutable access, leave that cluster in the facade and record why.

## 9. Compatibility and migration

No persisted migration. Maintain facade re-exports for documented/tested internal Rust paths where worthwhile. This is a pre-1.0 internal frontend, so dead private helpers may be removed; public protocol/config/serialized names are not part of this cleanup.

## 10. Required tests

### Focused unit tests

- extracted pure render/presentation helpers;
- intent/effect mapping/exhaustive handler coverage where existing structure permits;
- M005-M007 state/lifecycle tests after movement.

### Integration tests

- `tests/tui.rs`, `tests/tui_render.rs`;
- project tabs/routing/picker tests;
- terminal/collaboration/observer/plugin/permission TUI paths touched by extraction.

### Restart/recovery/contention tests

Run existing projection/reconnect and rapid-tab lifecycle targets affected by moved code. No new stress framework.

### Security/negative tests

Observer and permission/question regression suites remain green; render/input extraction cannot bypass modal/observer gates.

## 11. Required verification commands

```bash
# focused module tests chosen from the final extraction map
cargo test --test tui
cargo test --test tui_render
cargo test --test tui_project_tabs
cargo test --test tui_project_routing
# M005-M007 focused regression targets
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Do not add line-count, dependency-graph, or coverage CI gates.

## 12. Documentation updates

- `architecture/tui.md`: final module ownership, command/message flow, render/async invariants.
- `.opencode/skills/tui/SKILL.md`: current source map and edit guidance.
- source module docs for extracted domains.
- planning roadmap/registry/closure status after evidence exists.

## 13. Acceptance criteria

M008 closes when the major TUI responsibilities are physically localized behind one `App` composition root; a contributor can identify the owner of project/session, prompt/turn, modal, render, projection, plugin UI, and command-completion behavior; `TuiMsg`/`TuiCommand` directionality is explicit; M005-M007 and broad TUI behavior remain unchanged; and no framework or alternate authority was introduced.

There is no numeric line-count target. A smaller `app/mod.rs` is expected evidence, not the success criterion.

## 14. Stop conditions

Stop if:

- M005, M006, or M007 is not closed;
- decomposition requires semantic changes to routing, modal, prompt, protocol, daemon or scheduler ownership;
- a proposed abstraction has one implementation and only exists to hide module movement;
- borrow-checker pressure is being solved by duplicating mutable state or detaching tasks;
- current HEAD has already materially decomposed these responsibilities and a new census invalidates the plan.

## 15. Closure evidence required

Include implementation commits, before/after responsibility map, source-size/line counts only as descriptive evidence, intent/effect direction table, list of removed/retained duplicate variants with rationale, moved-test results, M005-M007 regression evidence, full focused/broad verification outcomes, confirmation of unchanged storage/protocol/authority, and residual findings with severity.

## 16. Handoff notes

Treat this as a physical maintainability pass after correctness state contracts stabilize. Move coherent code with minimal edits first, then remove obsolete wrappers. Do not combine M009/M010 user-facing enhancements into this milestone even if the new modules make them easier.

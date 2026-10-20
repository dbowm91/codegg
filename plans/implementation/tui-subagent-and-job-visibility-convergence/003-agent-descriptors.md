# TUI Subagent and Job Visibility Convergence — Milestone 003 — Agent Descriptors

Status: **implemented** — closed at
`plans/closure/tui-subagent-and-job-visibility-convergence/003-status.md`

Repository baseline: `e8a6b5ca2` (the committed C001/C003/C004/C006 corrective tree, which descends from roadmap baseline `2acce2138`), plus the **uncommitted C002 working tree** — C002's `/sessions` sections render `SubagentRow`s and are therefore a fifth surface this milestone must keep consistent.

Source roadmap: `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md#c005--agent-descriptors`
Parent roadmap: `plans/subsystems/tui-subagent-and-job-visibility-roadmap.md`
Long-term requirements: `plans/000-long-term-specification.md`
Applicable ADRs: None required. C005 adds two additive projection fields and no new authority, consumer, or protocol surface; the `plans/adrs/README.md` thresholds are not met.

Primary class: capability, delivering **K3** and respecting invariant **I1**.

## 1. Objective

A subagent row shows what its agent **is**, not only what it was asked to do: the
pool entry's `profile` travels from the curated pool, through the spawn event and
the projection, onto every surface that already renders a subagent row — with no
second derivation of the row vector and no client-side lookup.

## 2. Why this milestone is ready

**C004 is the hard dependency and it is implemented, not closed.** The chain
coverage it exists to install is present and named in the baseline tree:

| Link | Test | Site |
|---|---|---|
| event → node | `subagent_started_carries_name_and_run_id_into_the_node` | `crates/codegg-protocol/src/projection/adapters.rs:1055` |
| event without the field | `subagent_started_without_a_name_is_still_a_node` | `crates/codegg-protocol/src/projection/adapters.rs:1081` |
| node → rendered row | `a_named_subagent_reaches_the_rendered_row` | `src/tui/app/render.rs:2901` |
| replay determinism | `replaying_a_subagent_script_is_deterministic` | `crates/codegg-protocol/src/projection/reducer.rs:2047` |

C005 extends exactly the additive-field contract C004 pinned: carried **in the
event**, copied verbatim, defaulted on decode, never re-drawn by the reducer.
That contract is stable in the tree.

**Honest caveat:** C004 has no closure record and is registered `implemented,
unclosed`. C005's dependency is satisfied in substance and not in process. If
C004's closure review finds a defect in this chain, C005 must be re-reviewed
against the corrected chain before it lands. Do not read "ready" as "the
dependency is closed".

C002 contributes no hard dependency — it only adds a render surface.

## 3. Current implementation evidence

The descriptors are authored, compiled, and unreachable. The chain breaks at the
**first** hop, and every later hop is already correct for `display_name`.

| Hop | State | Evidence |
|---|---|---|
| Pool source | `profile`, `usefulness`, `sprite` authored per entry | `assets/subagent-names/names.toml:13-16` |
| Compiled entry | `SubagentName { name, profile, usefulness, sprite }` | `src/agent/subagent_names/generated.rs:10-21` |
| Name → entry lookup exists | `pool_entry("Sally-2")` → the `Sally` entry; `None` for a non-pool name | `src/agent/subagent_names/registry.rs:264-276` |
| Publisher holds a name, not a descriptor | `SubagentNameLease` exposes `name()`/`name_owned()`/`task_id()`; no entry accessor | `registry.rs:219-250`; `src/agent/worker.rs:994-1009` |
| Event | carries `display_name`, `run_id`, nothing else | `crates/codegg-core/src/bus/events.rs:307-318`; mapped at `src/core/mod.rs:581-593` |
| Adapter | copies `display_name`/`run_id` into the node verbatim | `crates/codegg-protocol/src/projection/adapters.rs:462-486` |
| Node DTO | both fields `#[serde(default)]`; `normalise()` truncates | `crates/codegg-protocol/src/projection/dto.rs:465,473`, `normalise` at `:475-490` |
| TUI row | `SubagentRow` has 6 fields; no descriptor | `src/tui/app/state/subagents.rs:38-58` |
| Snapshot → row | field-by-field copy | `src/tui/app/render.rs:2596-2619` |
| Render surfaces | 5 read `SubagentRow` | overlay `state/tasks.rs:179`; sidebar; strip `subagents.rs:135-139`; `/sessions` `dialogs/session.rs:1025-1030` |

### 3.1 Measured descriptor sizes and the resulting bound

Measured over all 46 pool entries at this baseline:

| Quantity | Bytes |
|---|---|
| longest `profile` | 223 |
| longest `usefulness` | 107 |
| longest `profile` + `usefulness` | **291** |
| whole pool, both fields summed | 10,973 |

`MAX_PROJECTION_SUBAGENTS` is **16** nodes per turn
(`crates/codegg-protocol/src/projection/limits.rs:67`), so the worst-case
snapshot growth is `16 × 291 = 4,656` bytes ≈ 4.5 KB.

`MAX_PROJECTION_STRING_BYTES` is 4,096 — far larger than any descriptor and
wrong as a per-descriptor cap, because a node already carries up to 4 KB of
`description`. **This milestone must introduce a named descriptor cap** and assert
the resulting per-snapshot total. Reusing `MAX_PROJECTION_STRING_BYTES` leaves
the bound unenforced and is rejected here.

### 3.2 The desktop bridge is already narrower than the node — and the guard will not notice

`SubagentSummaryView` is
`['task_id','agent','description','status','result_summary']`
(`apps/desktop/scripts/check-bindings.mjs:22`). It has **no `display_name` and no
`run_id`** — C001's two additive fields never reached the desktop bridge. The
mapping is field-by-field at `apps/desktop/src-tauri/src/present.rs:345-352`,
capped at `MAX_VISIBLE_SUBAGENTS = 10`.

`check-bindings.mjs` asserts that every field **listed** in its `expected` map is
present on both sides. It does **not** detect a Rust struct gaining a field the
list never mentions.

**Consequence:** the addendum's "keep `apps/desktop` bridge field parity" is
satisfiable by doing nothing, and doing nothing leaves that guard **vacuous for
this milestone**. See §5 for the decision and §14 for the stop condition.

### 3.3 Two source comments become false the moment this lands

- `assets/subagent-names/names.toml:13-16` — "Not rendered by the TUI today."
- `src/agent/subagent_names/generated.rs:13,16` — the same two sentences.

These are **emitted by the generator**
(`scripts/generate_subagent_names.py:132,134`), not hand-written in
`generated.rs`. `generate_subagent_names.py --check` compares `names.toml` to
`generated.rs` and therefore **will not** flag the staleness; it passes on two
identical lies. Correcting them requires editing the **generator** and
regenerating, or the source of truth stays wrong for the next person who reads it.

## 4. Invariants that must not regress

- **I1 — projection-derived rows.** A descriptor is rendered only when it
  arrived in the projection. No client-side fabrication, no client-side name
  minting, and specifically **no TUI-side pool lookup**: the frontend has no pool
  and must not grow one.
- **Descriptor provenance.** The profile is drawn from the pool entry at spawn and
  carried in the event, exactly as the name is. The reducer must not re-draw it —
  a re-drawn value would make replay non-deterministic and would let the live
  view and the durable record disagree, the exact failure C001 recorded for
  `display_name`.
- **Additive-only wire shape.** Every new field is `Option<String>` with
  `#[serde(default)]`, so a mixed-version daemon and frontend still connect in
  both directions.
- **I2** — `scripts/check_tui_transport_authority.py` stays green. C005 binds no
  key.
- **I5** — rendering a descriptor cancels nothing.
- **One row vector.** `sync_subagents_from_snapshot` remains the single
  projection→row derivation, now carrying two more fields.

## 5. Scope

**In scope**

1. Two additive `Option<String>` fields — `profile` and `usefulness` — threaded
   from the pool entry at spawn, through the event, the adapter, the node, and
   onto `SubagentRow`.
2. A **named** per-descriptor byte cap in `limits.rs`, applied in
   `normalise()`, plus an asserted per-snapshot total.
3. Rendering the descriptor on every surface that already renders a subagent
   row, with the existing `label()`-style fallback when it is `None`.
4. Correcting the two now-false source comments, in the generator.

**Out of scope**

- **No new pool authoring.** The descriptors are already curated and compiled;
  this only makes them reachable. Editing `names.toml` content is not this work.
- **`sprite` stays out.** All 46 values are empty strings; adding a third field
  for a value that does not exist yet is scope the milestone does not need. The
  addendum says two fields and this plan says two.
- **The desktop bridge.** `SubagentSummaryView` already omits `display_name` and
  `run_id`; descriptors follow that precedent. Routing descriptors to desktop
  *and* leaving the name behind would present a row with a personality and no
  name. Routing all three is a separate milestone with its own
  `check-bindings.mjs` field-list update — see §14.
- **Agent-type descriptions.** The `assets/agents/*.toml` descriptions already
  surface in the agents management panel; they are a different concept from a
  pool profile and already reachable.
- No storage, no migration, `STORAGE_LAYOUT_VERSION` untouched.
- No new binding, no new surface, no C007.

## 6. Required production changes

**`src/agent/subagent_names/registry.rs`** — give the publisher the descriptor.
Either a lease accessor returning the entry behind its own name, or a `pool_entry`
call at the publisher. The name used must be **the lease's own name**, so the
descriptor is by construction the one that was leased; a second lookup keyed on
anything else is the defect C001 already recorded once.

**`src/agent/worker.rs:994-1009`** — the single publisher. Populate both fields
from the leased entry. `profile` and `usefulness` are `&'static str`; a
non-pool name yields `None` from `pool_entry`, which is the honest answer rather
than a wrong attribution — carry `None`, do not substitute.

**`crates/codegg-core/src/bus/events.rs:307-318`** — two additive
`Option<String>` fields on `AppEvent::SubagentStarted`, with the same
determinism-rationale doc comment `display_name` carries. Mirror them in the
`AppEvent → CoreEvent` mapping at `src/core/mod.rs:581-593`.

**`crates/codegg-protocol/src/projection/dto.rs:465,473`** — two
`#[serde(default)]` fields on `AgentTreeNodeProjection`, plus `normalise()`
truncation at the new cap. Mirror the `display_name` doc comment's reasoning.

**`crates/codegg-protocol/src/projection/limits.rs`** — a named
`MAX_PROJECTION_DESCRIPTOR_BYTES`. Size it above the measured 223-byte maximum
so no curated descriptor is silently truncated, with headroom for authoring.

**`crates/codegg-protocol/src/projection/adapters.rs:462-486`** — copy both
fields verbatim into the node, alongside `display_name`.

**`src/tui/app/state/subagents.rs:38-58`** — two `Option<String>` fields on
`SubagentRow`, with the same "recorded before descriptors existed" doc note as
`display_name`. The render helper keeps its current fallback: a `None` descriptor
renders what the row renders today.

**`src/tui/app/render.rs:2596-2619`** — copy both fields in
`sync_subagents_from_snapshot`.

**`scripts/generate_subagent_names.py:132,134`** — drop "Not rendered by the TUI
today" from the two generated doc comments, regenerate `generated.rs`, and
update the equivalent prose in `assets/subagent-names/names.toml:13-16`.

## 7. Ordered work packages

**WP-1 — Publisher.** Descriptor on the event. *Before* any DTO change, because
it is the only hop that must not be faked downstream. *Acceptance:* a test at the
publisher asserts the event's `profile` equals `pool_entry(name).profile`, and a
suffixed name (`Sally-2`) resolves to the same entry.

**WP-2 — Wire shape.** Event fields, adapter copy, node DTO with
`#[serde(default)]`, `normalise()` truncation at the named cap. *Acceptance:* the
two existing adapter tests gain descriptor assertions; a new test proves an
event **without** descriptors still produces a node.

**WP-3 — Row.** `SubagentRow` plus `sync_subagents_from_snapshot`. *Acceptance:*
`a_named_subagent_reaches_the_rendered_row` is extended rather than duplicated —
C004's point was that this chain has exactly one seam.

**WP-4 — Render.** Every existing subagent-row surface shows the descriptor with
the current fallback when absent. No new surface, no new key.

**WP-5 — Comments, guards, docs.** The generator's false sentences, the
descriptor-cap guard, `architecture/tui.md`, `docs/tui.md`.

## 8. Failure, cancellation, restart, and contention semantics

Descriptors are static properties of a **name**, not of a run. A released name
redrawn later resolves to the same entry, so there is no staleness window and no
descriptor that drifts from its row.

A snapshot recorded before this milestone decodes with both fields `None` and
renders exactly as it does today. A daemon and a frontend on either side of the
change connect without error, because every new field defaults.

The reducer must not re-derive a descriptor from the node's name. It cannot: the
reducer has no pool, and a re-drawn value would break
`replaying_a_subagent_script_is_deterministic`.

The name lease's `Drop` releases only the name. C005 must not add a second
release path or extend lease lifetime — the RAII guarantee in the addendum's
cross-cutting requirements is unchanged.

## 9. Compatibility and migration

None. No storage migration, no durable session change, no configuration key. A
session recorded before this milestone replays with absent descriptors.
`STORAGE_LAYOUT_VERSION` untouched.

## 10. Required tests

- Pool: `generate_subagent_names.py --check` stays green.
- Publisher: the event's descriptors equal `pool_entry(lease name)`; a numeric
  suffix resolves to the same entry; a non-pool name yields `None`, not a guess.
- Adapter: descriptors reach the node; **no** descriptor still yields a node
  (mirrors `subagent_started_without_a_name_is_still_a_node`).
- Reducer: replay determinism still holds with descriptors present.
- Bounds: a 16-node tree carrying the pool's maximal descriptors stays within an
  asserted total budget, and `normalise()` truncates an over-long descriptor at
  the named cap.
- Row: `SubagentRow` carries both fields; a `None` descriptor renders the
  existing fallback.
- Every render surface shows the descriptor — the C002 dialog section included,
  since C002 is where this is most likely to be forgotten.
- **Negative:** no TUI module imports the name pool. A surface that composes a
  descriptor locally fails this test. This is the guard against the milestone's
  own I1 violation.
- No regressions: `tui_main_buffer`, `tui_side_panels`,
  `tui_sessions_pending_sections`, `codegg-protocol` projection tests.

## 11. Required verification commands

```bash
scripts/verify.sh quick
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings

python3 scripts/generate_subagent_names.py --check
python3 scripts/check_projection_transport_isolation.py
python3 scripts/check_projection_transport_lifecycle.py
bash    scripts/check_projection_disclosure.sh
python3 scripts/check_websocket_bounds.py
python3 scripts/check_tui_transport_authority.py
node    apps/desktop/scripts/check-bindings.mjs

cargo test -p codegg-protocol --lib projection
cargo test --lib subagent
cargo test --test tui_main_buffer
cargo test --test tui_side_panels
cargo test --test tui_sessions_pending_sections
```

`git diff --stat docs/execution-ownership.toml` must stay empty — C005 is
presentation over projected state and adds no execution surface.

## 12. Documentation updates

- `assets/subagent-names/names.toml:13-16` — the "not rendered today" note.
- `scripts/generate_subagent_names.py` + regenerated `generated.rs` — same note.
- `architecture/tui.md` — the `SubagentRow` contract gains the two fields, with
  the same provenance paragraph `display_name` has.
- `docs/tui.md` — what a subagent row now shows.
- Event field counts in `architecture/bus.md` / `architecture/protocol.md` are
  **variant** counts and do not change; verify rather than assume.

## 13. Acceptance criteria (externally observable)

1. A running subagent's row shows its pool profile on every surface that shows
   subagent rows, including the `/sessions` dialog.
2. A subagent whose node predates this milestone still renders, falling back to
   what it shows today — no empty column, no error.
3. Old daemon + new frontend, and new daemon + old frontend, both connect.
4. No surface displays a descriptor that did not arrive in the projection.
5. A 16-subagent session carrying maximal descriptors stays inside the asserted
   snapshot budget.

## 14. Stop conditions

Report rather than improvise if:

- Threading descriptors requires a **second** pool lookup at render time, or any
  TUI-side derivation. The boundary is one derived vector, carried.
- The snapshot growth cannot be brought under a named, asserted budget.
- Anyone proposes adding a third field (`sprite`) or authoring new pool content.
- Anyone proposes routing descriptors through the desktop bridge **without**
  also routing `display_name` and `run_id`. That is a third milestone's scope
  and needs its own plan; do not ship a personality without a name.
- A descriptor needs to vary per **run** rather than per name. That is a
  different concept from a curated pool entry and would change what
  `SubagentName` means.
- The guard in §10 (no TUI import of the pool) cannot be expressed. Without it
  the milestone's own I1 boundary is unenforced, which is the defect class this
  whole addendum exists to remove.

## 15. Closure evidence required

`plans/closure/tui-subagent-and-job-visibility-convergence/003-status.md` with the
requirement-to-evidence matrix, the measured-size and bound evidence, the
non-vacuity proof for the no-TUI-pool guard, and truthful local-vs-CI labels. A
milestone is not closed because it compiles.

## 16. Handoff notes

Three details are easy to get wrong and hard to notice:

1. **`SubagentNameLease` holds a name, not an entry.** The descriptor is one
   `pool_entry` call away, but nothing hands it over today. That is the whole
   first work package.
2. **The desktop parity requirement is satisfiable by doing nothing**, and
   `check-bindings.mjs` will pass either way (§3.2). Reading it as "no work" is
   the trap; the real content is the recorded decision not to widen the bridge.
3. **Two source comments become lies.** They live in the generator, and
   `--check` passes on two identical lies (§3.3).
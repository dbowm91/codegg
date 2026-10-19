# TUI Subagent and Job Visibility Convergence — Milestone 004 — Closure Status

Status: **closed**

Source implementation plan:

- **None.** C004 was implemented against the addendum's deliverable boundary
  without one. This is governance gap **D-8** of the corrective addendum —
  "M001–M005 were marked landed without the implementation plan or closure
  record the process requires" — repeating in the corrective workstream itself.

  No plan was written retrospectively. A plan authored after the fact is not the
  specification the work was built against, and minting one here would convert a
  recorded governance defect into an invisible one. This record therefore closes
  C004 **directly against the addendum's boundary and exit conditions**, which is
  the only specification that ever existed for it.

Source subsystem roadmap:

- `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md`
  §7 (C004), §2 (I1), §4 (D-3)

Repository baseline: `e8a6b5ca2` plus the **uncommitted working tree** carrying
C001–C006.

Implementation: **working tree, uncommitted** at the time of this record. No
commit hash is claimed because none exists.

Scope note: the tree also contains C002's and C005's uncommitted implementation
and the planning changes of both. This record describes only what it verified.

## 1. Executive finding

C004 is closed, and closing it changed the tree: the review found that the
"middle of the naming chain" was covered **link by link but not at two of its
links**, and both gaps sat on the exact path the milestone exists to protect.

Two named tests were added — one on the `AppEvent → CoreEvent` mapping, one on
the serde wire round-trip — and both were then **proven non-vacuous** by
injecting the defect each exists to catch. Both passed before injection and
failed after, naming the dropped field.

**No defect was found in the additive-field chain.** The fields are copied at
every hop and survive serialization; the gap was coverage, not correctness. The
consequence for C005 is recorded in §9.

## 2. Requirement-to-evidence matrix

C004's deliverable boundary asks for an end-to-end test driving
`CoreEvent::SubagentStarted { display_name, run_id, .. }` → adapter →
`SessionProjectionSnapshot.agent_tree` → `sync_subagents_from_snapshot` →
`SubagentRow.display_name.is_some()`, plus the mirror paths.

| # | Requirement | Evidence | Result |
|---|---|---|---|
| 1 | Adapter hop preserves the identity fields | `subagent_started_carries_name_and_run_id_into_the_node` (`crates/codegg-protocol/src/projection/adapters.rs`) | pass |
| 2 | A node without a name is still a node | `subagent_started_without_a_name_is_still_a_node` (same file) | pass |
| 3 | Node reaches the snapshot's active turn tree intact | reducer test asserting `display_name`/`run_id`/`profile`/`usefulness` on `active_turn.agent_tree` (`crates/codegg-protocol/src/projection/reducer.rs`) | pass |
| 4 | **Bus → core mapping preserves them** | `subagent_started_maps_its_identity_fields_onto_the_core_event` (`src/core/mod.rs`) — **added by this closure** | pass (added) |
| 5 | **Serde wire round-trip preserves them** | `a_subagent_node_survives_the_serde_wire_boundary` (`crates/codegg-protocol/src/projection/dto.rs`) — **added by this closure** | pass (added) |
| 6 | Snapshot → row derivation | `a_named_subagent_reaches_the_rendered_row` (`src/tui/app/render.rs`, `mod subagent_row_chain_tests`) | pass |
| 7 | An unnamed node still renders a row | `an_unnamed_node_still_renders_a_row` (same module) | pass |
| 8 | Rows survive the parent turn completing | `rows_survive_the_parent_turn_completing` (same module) | pass |
| 9 | An empty snapshot yields no rows | `an_empty_snapshot_yields_no_rows` (same module) | pass |
| 10 | Replay determinism reproduces identical trees | `replaying_a_subagent_script_is_deterministic` (reducer) | pass |
| 11 | Replay copies the recorded name/run id rather than re-drawing | `replay_preserves_the_recorded_display_name`, `replay_preserves_the_recorded_run_id`, `an_event_without_a_display_name_replays_as_absent` (`crates/codegg-core/src/projection_replay/publication.rs`) | pass |
| 12 | Mirror: `sidebar.set_subagents` renders one name per `task_id` | `rendered_rows_are_exactly_the_rows_the_snapshot_reported` and siblings (`src/tui/components/sidebar.rs`) | pass |
| 13 | Mirror: `build_task_rows` renders one name per `task_id` | `a_running_subagent_and_a_queued_job_both_appear`, `a_subagent_row_carries_the_identity_a_tab_is_keyed_on`, `subagent_label_falls_back_to_agent_type_when_unnamed` (`src/tui/app/state/tasks.rs`) | pass |
| 14 | Mirror: the `/sessions` sections list the live subagents | `the_painted_dialog_lists_the_live_subagents_and_jobs` and 10 siblings (`tests/tui_sessions_pending_sections.rs`) — drives node → snapshot → **render** → painted dialog | pass |
| 15 | Exit: every surface covered by a named test | This matrix | pass **only after #4 and #5** |

## 3. The two gaps, and why they were real

C004's premise is that "a regression in `CoreEvent → adapter → snapshot → row`
would be invisible to CI" (addendum D-3). The implementation closed most of that,
but the chain has **eight** links, not four, and the four tests each constructed
their input as a **Rust literal**. That bypasses two real links.

**Gap 1 — the bus → core mapping (`src/core/mod.rs`).** This is the hop a
daemon-mode TUI depends on, and daemon mode is the **default**: the sole
publisher writes an `AppEvent`, and `map_app_event_to_core_event` turns it into
the `CoreEvent` the adapter reads. Its test module covered `TestRun*` and
`SessionUpdated`; nothing covered `SubagentStarted`. Deleting `display_name` from
that one match arm would have removed **every name in the default
configuration** with the whole suite green — precisely the "the feature stopped
working and CI says fine" outcome C004 exists to prevent.

**Gap 2 — the serde wire boundary (`crates/codegg-protocol/src/projection/dto.rs`).**
This is not an in-process convenience: `TuiMessage::ProjectionSnapshot`
(`crates/codegg-protocol/src/tui.rs:128-131`) ships a `SessionProjectionSnapshot`
to the frontend over the WebSocket, so an agent-tree node genuinely crosses a
serde boundary. The DTO had `a_node_without_descriptors_decodes_with_them_absent`
— the **missing** direction — and nothing covered the **present** one, which is
the direction every live subagent takes. A `#[serde(skip)]`, a stray `rename`,
or a `skip_serializing_if` on `display_name` would have left every other chain
test green.

## 4. Non-vacuity proof

A test that cannot fail is not evidence. Each new test was run against a real
injection of the defect it exists to catch.

### Gap 2 — `#[serde(skip)]` on `AgentTreeNodeProjection::display_name`

```
test projection::dto::tests::a_subagent_node_survives_the_serde_wire_boundary ... FAILED
panicked at crates/codegg-protocol/src/projection/dto.rs:1217:
  assertion `left == right` failed: display_name dropped crossing the wire
    left: None
   right: Some("Sally")
```

File restored; the suite returned green.

### Gap 1 — `display_name: None` in the `SubagentStarted` mapping arm

```
test core::tests::subagent_started_maps_its_identity_fields_onto_the_core_event ... FAILED
panicked at src/core/mod.rs:1253:
  assertion `left == right` failed: display_name dropped on the AppEvent -> CoreEvent mapping
    left: None
   right: Some("Sally")
```

File restored; re-run after restore:

```
test core::tests::subagent_started_maps_its_identity_fields_onto_the_core_event ... ok
```

Each failure named the dropped field, which is what makes these tests useful in
CI output rather than merely non-vacuous.

## 5. Verification executed

**All verification was local. No CI receipt exists for any of this work, and no
runtime observation was made — the TUI was never launched.**

```bash
cargo fmt --check                                                     # pass
scripts/verify.sh quick                                              # pass ("Quick verification passed")
cargo clippy --workspace --all-targets --locked -- -D warnings      # pass, no warnings
cargo test -p codegg-protocol --lib projection                       # 94 passed, 0 failed
cargo test -p codegg-core --lib projection                           # 76 passed, 0 failed
cargo test --lib subagent                                            # 98 passed, 0 failed
cargo test --test tui_main_buffer                                    # 48 passed, 0 failed
cargo test --test tui_side_panels                                    # 31 passed, 0 failed
cargo test --test tui_sessions_pending_sections                      # 11 passed, 0 failed
python3 scripts/check_projection_transport_isolation.py              # pass
python3 scripts/check_projection_transport_lifecycle.py              # pass
python3 scripts/check_projection_disclosure.sh                       # pass
python3 scripts/check_websocket_bounds.py                            # pass
python3 scripts/check_tui_transport_authority.py                     # pass
python3 scripts/check_tui_subagent_name_pool_isolation.py           # pass
python3 scripts/check_adr_integrity.py                               # pass
python3 scripts/generate_subagent_names.py --check                   # pass
(cd apps/desktop && node scripts/check-bindings.mjs)                 # pass
```

`cargo test -p codegg-protocol --lib projection` reports **94**, against 93
before this record: the delta is the new wire test. No other suite's count
changed.

`scripts/check_tui_subagent_name_pool_isolation.py` is C005's I1-boundary guard
(fails if any module under `src/tui/` names the curated pool). It is green here
and is the reason C007 may address a subagent by a name carried **on the row**
rather than by a send-time pool lookup.

`apps/desktop/scripts/check-bindings.mjs` must be run **from `apps/desktop`**; it
resolves paths relative to cwd and fails with `ENOENT` from the repo root. This
is a change-triggered guard, not part of `verify.sh` or CI. It asserts only the
fields it lists, so it cannot detect a Rust struct gaining an unlisted field —
it is **not** evidence of desktop parity for the descriptor fields, and is not
cited as such (see §9).

## 6. Invariant review

- **I1 — projection-derived rows.** Holds. Every row in §2 originates in a
  `SessionProjectionSnapshot` field; no client-side fabrication or name minting
  was found or added. `sync_subagents_from_snapshot` is the single derivation and
  is now covered at four behaviours.
- **Addendum D-3 (zero coverage of the chain's middle).** Resolved, with the two
  links this record added.

## 7. Failure and recovery review

- **Duplicate delivery / idempotency.** Unchanged by this record; only tests were
  added. No production behaviour moved.
- **Cancellation races.** Unchanged. C004 pins visibility, never cancellation,
  and I5 stands.
- **Restart / stale lease.** Unchanged. The name-release lease remains RAII and
  exactly-once; no second release path exists or was added.
- **Bounded behaviour.** Unchanged. `MAX_PROJECTION_SUBAGENTS` and the C005
  descriptor caps are untouched.

## 8. Migration and compatibility review

None. This record adds **two tests and no production change**. No protocol field,
storage layout, configuration key, or `STORAGE_LAYOUT_VERSION` change.
`crates/codegg-core/src/session/schema.rs` untouched;
`git diff --stat -- docs/execution-ownership.toml` empty.

## 9. Consequence for C005 — the conditional re-review

C005's closure record carried this caveat: *"C004 remains unclosed… If C004's
closure review finds a defect in the additive-field chain, C005 must be
re-reviewed against the corrected chain."*

**No defect was found, so no re-review is required.** Both new tests passed
against the unmodified chain before any injection, which is direct evidence that
`display_name`, `run_id`, `profile`, and `usefulness` are copied correctly at
every hop and survive serialization. The additive-field contract C005 extends is
intact, and C005's closure stands unchanged.

What changed is C005's **recorded** premise, not its implementation: C004's
closure now names two links that no test covered at the time C005 was closed, and
states that a field dropped on either would now be caught. That strengthens the
claim C005 already made; it does not weaken it.

The desktop-bridge caveat also carries forward unchanged. C005 chose not to widen
`SubagentSummaryView`, and `check-bindings.mjs` would have passed identically
either way. Citing that guard as desktop parity would be a vacuous claim and is
not made here.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | C004 has no implementation plan — the D-8 governance defect repeated inside the corrective workstream | The milestone had no bounded handoff specification; closure had to be argued against the addendum directly | Accepted and recorded in the header. **No retrospective plan was written** — see the header's reasoning. Do not cite C004 as a process exemplar. |
| low | Surface-level tests for the sidebar and the overlay hand-seed `SubagentRow`/`SidebarSubagent` rather than driving the chain | These would not fail if the snapshot→row link dropped a field. That link is covered by `a_named_subagent_reaches_the_rendered_row`, so the chain is pinned; the surfaces are pinned at the row level | Accepted. Extending every surface test to the full chain would duplicate one derivation test per surface. Reconsider only if a surface test is ever found asserting a name without any chain coverage. |
| low | `C003`, `C006` remain implemented-but-unclosed in the same tree | They are outside C004's boundary and were not reviewed | Closed in their own records. |

No critical or high finding remains. C004 is an invariant milestone whose entire
deliverable is coverage, and coverage is now complete at every link.

## 11. Roadmap disposition

**Milestone closed; no corrective pass required; C007's dependency is discharged
in process as well as in substance.**

Every requirement in C004's deliverable boundary is met, and every exit condition
holds:

- *Every surface is covered by a named test that fails if any link drops the
  field* — met, and was **not** met before the two tests this record added.
- *A replay-determinism test reproduces identical names* — met
  (`replaying_a_subagent_script_is_deterministic`).

C005's hard dependency on C004 is now satisfied in **process** as well as
substance, which was not true when C005 closed.

## 12. Registry updates

Required, and applied with this record:

- `plans/registry.md` — subsystem status cell to **C001, C002, C004 and C005
  closed**; add a C004 row to *Recently closed*; update the C005 row's dependency
  note, which currently says "C004 — implemented, not closed".
- `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md`
  §7 C004 and §11 status table: `implemented, unclosed` → `closed`, with the
  closure path recorded.

**Unblock audit.** C004 is a hard dependency of **C005** (closed) and **C006**
(implemented, unclosed, no plan). C007 depends on C005, not on C004. So this
closure **unblocks nothing new**: C006 was never gated on a C004 *closure*, and
C007 was already unblocked by ADR-0040's acceptance. Recording that explicitly
rather than leaving the audit implicit.

## 13. What was **not** verified

Stated plainly so nothing here reads as stronger than it is:

- **No CI receipt.** Every command above ran locally.
- **No runtime observation.** The TUI was never launched. Buffer-level and
  dialog-level assertions prove text reaches the rendered buffer; they do not
  prove a human read it comfortably.
- **No stashed-baseline comparison** for the pre-existing full-suite failures
  recorded in C002's and C005's closure records. This record did not re-run
  `cargo test --lib` in full, so it makes no claim about the whole-suite totals
  and inherits the C005 attribution unchanged.
- The chain is covered at **unit/integration level**. Nothing here proves the
  daemon's WS transport of a live snapshot carries the fields on a real socket.
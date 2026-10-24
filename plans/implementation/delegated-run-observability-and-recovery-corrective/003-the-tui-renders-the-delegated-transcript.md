# Delegated Run Observability and Recovery Corrective Milestone 003 — The TUI Renders the Delegated Transcript

Status: ready for handoff

Repository baseline: `a21b5fffa` on `fix/tui-composer-helix-parity-post-closure-corrective` (2026-10-24). Revalidate the actual implementation head before editing.

Source roadmap: `plans/subsystems/delegated-run-observability-and-recovery-corrective-addendum.md` §7 (C003)

Long-term requirements:

- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#28-observability`
- `plans/001-terminology-and-domain-model.md` — delegated run, session

Applicable ADRs:

- `plans/adrs/ADR-0051-durable-delegated-run-transcript-and-continuation-authority.md` — **accepted (option B) on 2026-10-24.** §"What may cross the frontend" fixes the projection contract this milestone implements: a bounded, redacted transcript DTO, delivered only to a client bound to the owning session, with the bound declared and a refusal rather than a silent truncation.
- `plans/adrs/ADR-0043-session-owned-subagent-lifecycle-projection.md` (accepted) — the delegated lifecycle projection this milestone extends. Its Decision text is unchanged.
- `plans/adrs/ADR-0041-subagent-run-addressing-and-human-interaction-authority.md` (accepted) — unchanged; this milestone makes a child's transcript *visible* and gives the human no new channel to the child.

Primary class: capability

## 1. Objective

Turn the delegated run's tab from a **run inspector** into a **transcript buffer**, keeping the inspector's metadata.

One bounded outcome: the delegated run's tab renders the delegated run's recorded conversation, bounded and redacted, delivered over the projection seam to a client bound to the owning session; and the repository states plainly — in the code, in `architecture/tui.md` and in `docs/tui.md` — that this is a transcript and what it is not.

This is the milestone that answers the reported symptom directly: *"we can not seem to directly see a subagent's buffer."*

## 2. Why this milestone is ready

**It is.** The milestone's gates are discharged:

1. **C002 is closed** (2026-10-24) — `plans/implementation/delegated-run-observability-and-recovery-corrective/002-a-durable-transcript-for-a-delegated-run.md`, closure record `plans/closure/delegated-run-observability-and-recovery-corrective/002-status.md`. The store, the redaction and the bounds exist.

ADR-0051 was **accepted on 2026-10-24 as option B**, so the second former blocker is discharged: §"What may cross the frontend" is now the accepted contract this milestone implements — a bounded, redacted DTO, delivered only to a bound client, with the bound declared and a refusal rather than a silent truncation.

**Interface dependency (stable, but not sufficient):** the delegated lifecycle projection and the tab's own structure exist. `src/tui/app/render.rs:1563-1697` renders the run inspector today; `src/tui/app/state/subagent_tabs.rs` owns the tab state; `crates/codegg-protocol/src/projection/dto.rs:545-548` defines the delegated summary that carries no transcript.

**Soft dependency:** C001. Not a hard dependency — C003 can render a transcript for a run whose notice was or was not delivered. But C001 must close before this milestone's closure, because the user-visible claim "the orchestrator learns what happened to its child" and the claim "the user can see what the child did" are the same story told to two audiences, and a milestone that ships the second half alone leaves the first half's symptom standing.

## 3. Current implementation evidence

### 3.1 The tab is a run inspector, and says so

`src/tui/app/render.rs:1567-1572`:

> "It is a run inspector, not a transcript — it shows only what the projection actually vouches for… It deliberately does not synthesize progress text: the projection adapter hardcodes `progress: None`, so an invented 'working on step 3' line would be a lie the user acts on."

The rendered content is the name, the agent, the status, the task description, the persona and the two ids. That is a faithful rendering of `AgentRunSummary` — the tab is not at fault.

### 3.2 The projection carries no transcript, on purpose

`crates/codegg-protocol/src/projection/dto.rs:545-548` — `AgentRunSummary` "contains no prompt, transcript, mailbox body, hidden reasoning, credentials, or raw logs." `crates/codegg-core/src/projection_replay/agent_runs.rs:92` — the adapter hardcodes `progress: None`.

### 3.3 The tab state is already per-run and already switchable

`src/tui/app/state/subagent_tabs.rs` owns `SubagentTabId`, `SubagentTabState`, `open_or_focus`, `cycle`, `sync_with_rows` and `bar_tabs`; `src/tui/app/state/subagents.rs` owns `SubagentRow`, whose fields carry `child_instance_id`, `task_id`, `run_id`, `display_name`, `profile`, `usefulness`, `agent`, `description` and `status` — **no transcript field**, and a `run_id` precisely so a view can fetch run detail.

### 3.4 The projection seam is a guarded trust boundary

`scripts/check_projection_disclosure.sh`, `scripts/check_projection_publication_seam.sh`, `scripts/check_projection_transport_isolation.py`, `scripts/check_projection_transport_lifecycle.py` and `scripts/check_websocket_bounds.py` all exist and all run in `verify.sh quick`. A new projection payload must be admitted by them, not merely pass them.

### 3.5 Why the existing verification passed

Nothing was wrong. The tab rendered exactly what the projection vouched for, and the projection vouched for metadata. The absence of a transcript was never a check failure; it was a data-model absence, and this milestone's predecessor C002 is what changes it.

## 4. Invariants that must not regress

- **What crosses the seam is bounded and declared.** A payload that exceeds its declared bound is **refused**, not silently truncated. A silently truncated transcript is a transcript the user reads as complete.
- **Redaction is inherited, not re-implemented.** The body is redacted at write (C002). The projection must not attempt to redact again — a second redaction pass is a second recogniser, and a second recogniser is a second set of holes.
- **The projection carries no credential, environment value or raw provider reasoning.**
- **A client that does not know the transcript field still works.** The DTO is additive and `#[serde(default)]`-tolerant.
- **Only a client bound to the owning session receives the transcript.** Exact-owner routing, as the delegated lifecycle projection already does.
- **The run inspector's metadata is retained.** The transcript is added to the tab, not substituted for the status line. The user still needs to know the run's status at a glance.
- **The tab is a buffer, not a live progress feed.** The projection adapter's `progress: None` stands; this milestone must not synthesize progress.
- **The session catalog, the scheduler's authority and the delegated run's isolation are unchanged.**
- **`codegg-core` stays UI-, server-, plugin- and auth-free.** The DTO belongs in `codegg-protocol`; the replay adapter in `codegg-core`; the rendering in the TUI.

## 5. Scope

### In scope

- A bounded, redacted transcript DTO in `codegg-protocol`, with its byte bound declared on the type.
- The projection replay adapter's population of that DTO from the C002 store.
- The delivery path to a client bound to the owning session.
- The tab's rendering: the transcript, plus the retained inspector metadata.
- The tab's behaviour when there is no transcript (a run recorded before C002, the legacy path, a truncated transcript): a plain statement, never an empty pane that reads as "nothing happened".
- The `architecture/tui.md` and `docs/tui.md` statements that a delegated tab is now a transcript.
- Extending the projection guards to cover the new payload.

### Explicitly out of scope

- **Any change to the delegated run's store, its redaction or its bounds.** C002 owns those.
- **Any change to the delegated run's authority, worktree, sandbox or tool policy.**
- **Any human-to-child channel.** ADR-0041 stands: the human steers the parent. Seeing the child's transcript is not addressing the child.
- **Live streaming of the transcript.** The transcript is durable and read at a boundary; it is not a live feed. A live feed would be a new projection capability with its own bounds.
- **Any change to the parent's own transcript rendering.**
- **Search, filter or export over a transcript.**
- **The project-wide transcript viewer.** This milestone is the delegated tab only.
- **A retention or compaction product.**

## 6. Required production changes

### Core/domain

- **The replay adapter** (`crates/codegg-core/src/projection_replay/agent_runs.rs`) populates the new transcript field from the C002 store, bounded by the DTO's declared bound.
- **Bounded read.** The adapter must request a bounded page. It must not load a whole transcript into memory to truncate it afterwards — the bound is enforced at the store (C002) and honoured at the seam.

### Storage and migrations

None. This milestone reads the C002 store.

### Protocol and DTOs

- A new field on the delegated run's projection DTO, carrying the transcript's rows in order, **with the byte bound declared on the type** so that the bound is a property of the contract rather than a comment.
- The DTO is additive and `#[serde(default)]`-tolerant: a client that does not know the field ignores it; a newer daemon against an older client degrades to the run inspector.
- The DTO carries a statement of completeness: whether the transcript is **complete**, **truncated** (and at what point), or **absent**. "Absent" and "truncated" and "complete" are three different facts and the UI must be able to say which.

### Runtime and concurrency

- **Exact-owner routing.** The transcript reaches the client bound to the owning session and no other, matching the delegated lifecycle projection's existing routing.
- **Read at a boundary.** The tab reads the transcript when it opens or refreshes, not on a timer.
- **A concurrent append** while the tab is open must not produce a torn view. The tab renders a consistent page: either the page it requested, or a stated truncation point.

### Frontend or operator surface

- **The tab** (`src/tui/app/render.rs:1563-1697`) renders the transcript beneath the retained inspector metadata: the run's identity and status first, then the conversation in order.
- **The three transcript states** are rendered distinctly: complete, truncated (with the truncation point shown), and absent (a plain statement of why). A tab that renders an absent transcript as an empty pane is a defect: it reads as "the child did nothing", which is a claim the system does not know.
- **Scroll and navigation** must work over a transcript that is longer than the viewport. The tab must not silently show only the first rows with no indication that more exist.
- **Rendering a tool call or a tool result** must show it as what it is — a tool call, a tool result — and must not render an unbounded blob.
- The run inspector's metadata is retained. The user still needs the status at a glance.

### Security and authorization

- The transcript body was redacted at write. The seam must not re-redact, and must not widen what may cross.
- A client bound to a **different** session must not receive the transcript. The negative case is a test, not an assumption.
- A transcript body must never be logged in full by the daemon. The existing redaction applies to logs as well as to storage.

### Documentation and static guards

- `architecture/tui.md` — the delegated tab is a transcript; what it shows; what it does not show.
- `architecture/projection.md` — the new DTO, its bound, and its completeness statement.
- `docs/tui.md` — the user-facing description of the delegated tab, in the same terms as the panel.
- **Static guards (required).** The existing projection guards must be extended to the new payload rather than merely run against it:
  - `scripts/check_projection_disclosure.sh` must fail when the new DTO field is declared but never populated.
  - `scripts/check_websocket_bounds.py` must cover the new payload's bound.
  - `scripts/check_projection_publication_seam.sh` must cover the new payload's routing.
  - Each extension carries its own non-vacuity proof (the repository's guards use `--self-test`; where a shell guard cannot, the extension must include an injected-violation case).

## 7. Ordered work packages

### Work package A — The DTO and its bound

Intent: the contract states what crosses and how much.

Required changes: per §6 "Protocol and DTOs".

Acceptance evidence: the DTO carries a declared bound and a completeness variant; a test that a payload over the bound is refused rather than truncated.

### Work package B — The replay adapter

Intent: the projection carries the transcript, bounded.

Required changes: per §6 "Core/domain".

Acceptance evidence: a test that the adapter's payload equals the stored transcript for a short run, and that a long run's payload is bounded and states its truncation point.

### Work package C — Routing and the TUI

Intent: only the owning session's client receives it, and the tab renders it honestly.

Required changes: per §6 "Runtime and concurrency" and "Frontend or operator surface".

Acceptance evidence: a test that a client bound to another session does not receive the transcript; a render test that the tab shows the conversation; a render test that the absent case says so; a render test that a truncated transcript says so.

### Work package D — The guard extensions

Intent: the new payload cannot drift out of the guarded set.

Required changes: per §6 "Documentation and static guards".

Acceptance evidence: each extended guard fails against the pre-change tree and passes after, with a non-vacuity proof.

### Work package E — Docs and closure

Intent: the repository states the truth.

Required changes: the documentation updates in §6.

Acceptance evidence: `bash scripts/verify.sh quick` exits 0.

## 8. Failure, cancellation, restart, and contention semantics

- **No transcript** (a run recorded before C002, or the legacy path). The tab states that there is no transcript and why. It does not render an empty pane.
- **A truncated transcript.** The tab says it is truncated and shows the truncation point. The user must never read a truncated transcript as complete.
- **The store is unavailable.** The tab renders the inspector with a stated reason; it does not fail the panel.
- **A concurrent append** while the tab is open. The tab shows a consistent page; a later refresh shows more.
- **A cancelled or failed run.** Its transcript renders like any other; the status line already says what happened.
- **The client reconnects.** The transcript is re-read from the durable store; there is no live-stream state to reconcile.
- **A very long transcript.** The tab must remain usable — the viewport shows a window with an indication that more exists, and the store's page bound is honoured on the wire.

## 9. Compatibility and migration

- **No migration.**
- **Additive protocol.** The new DTO field is optional. An older client renders the run inspector, which is what it renders today. A newer client against an older daemon sees no transcript field and renders the same.
- **No change to the delegated lifecycle projection's identity or ownership.** ADR-0043's Decision text is unchanged; only a payload is added.
- **No change to the session catalog.**
- **No change to the delegated run's behaviour.** This milestone reads; it does not write.

## 10. Required tests

### 10.1 Focused unit tests

- The DTO's declared bound is enforced on construction or on serialization; a payload over it is refused.
- The completeness variant distinguishes complete, truncated and absent.

### 10.2 Integration tests — `tests/projection_replay/`, `tests/tui_render/`

- The projection carries the transcript in order for a short run.
- A long run's payload is bounded and states its truncation point.
- A client bound to another session does **not** receive the transcript.
- A run with no transcript yields the absent variant.
- An older client, given the new field, still renders the inspector.

### 10.3 Render tests — `tests/tui_render/` (the delegated-tab member)

- The tab renders the conversation.
- The tab retains the inspector metadata (name, status, ids) above the transcript.
- The absent case renders a statement, not an empty pane.
- The truncated case renders the truncation point.
- A transcript longer than the viewport is navigable and indicates that more exists.
- A tool call and a tool result render as such, and an unbounded tool output renders bounded.

### 10.4 Guard tests

- Each extended projection guard fails against the pre-change tree and passes after.

## 11. Required verification commands

```bash
# narrow first
cargo test -p codegg-protocol -- projection
cargo test -p codegg-core -- projection_replay
cargo test --test projection_replay -- <member>
cargo test --test tui_render -- <member>

# static guards, including the extended ones
bash scripts/check_projection_disclosure.sh
bash scripts/check_projection_publication_seam.sh
python3 scripts/check_projection_transport_isolation.py
python3 scripts/check_projection_transport_lifecycle.py
python3 scripts/check_websocket_bounds.py
bash scripts/check-core-boundary.sh
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py

# formatting and linting
cargo fmt --all
CARGO_BUILD_JOBS=8 cargo clippy --workspace --all-targets --locked -- -D warnings

# the canonical gate
bash scripts/verify.sh quick
```

`bash scripts/verify.sh quick` is the canonical gate and must exit 0. Never run `cargo test --workspace`, a bare `cargo test`, or anything with `--all-features`.

## 12. Documentation updates

- `architecture/tui.md` — the delegated tab is a transcript buffer; the inspector metadata is retained; the three transcript states.
- `architecture/projection.md` — the new DTO, its declared bound, its completeness statement, and its routing.
- `docs/tui.md` — the user-facing description of the delegated tab.
- `src/tui/app/render.rs:1567-1572` — the comment that currently says "It is a run inspector, not a transcript" must be rewritten, because after this milestone it **is** a transcript. Leaving that comment in place would be a lie in the source.

## 13. Acceptance criteria

1. The delegated tab renders the delegated run's recorded conversation, in order, with the run's identity and status retained.
2. The payload is bounded by a **declared** bound, and a payload over the bound is refused rather than silently truncated.
3. The tab distinguishes complete, truncated and absent, and never renders an absent transcript as an empty pane.
4. A client bound to a different session does not receive the transcript.
5. A client that does not know the transcript field still works.
6. The body is not re-redacted at the seam, and no credential, environment value or raw provider reasoning crosses it.
7. The projection guards cover the new payload, each with a non-vacuity proof, and each failing against the pre-change tree.
8. `bash scripts/verify.sh quick` exits 0 and `cargo clippy --workspace --all-targets --locked -- -D warnings` is clean.

## 14. Stop conditions

The implementing agent must stop and report rather than improvise when:

- **C002 is not closed** — the milestone's remaining gate. (ADR-0051's gate is discharged: accepted as option B on 2026-10-24.)
- the store's redaction cannot be relied on, so that the seam would have to redact — a second recogniser is out of scope and a defect.
- the bound cannot be declared on the DTO — an undeclared bound is not a bound.
- the transcript would need a live-streaming path to be useful — that is a new projection capability with its own bounds, and it is out of scope.
- ADR-0041 would need to change for the human to see the transcript — it does not, and if it appears to, the design is wrong.
- the change would expand into C004's continuation scope.

## 15. Closure evidence required

The closure record at `plans/closure/delegated-run-observability-and-recovery-corrective/003-status.md` must contain:

1. The C002 closure reference and ADR-0051's acceptance commit.
2. The requirement-to-evidence matrix for §13, each row naming a **file:line** and a named test.
3. **Render evidence**: the tab's rendered output for the complete, truncated and absent cases, quoted.
4. **Routing evidence**: the cross-session negative test, quoted.
5. **Bound evidence**: the over-bound payload refusal, quoted.
6. The **guard extension evidence**: each extended guard's failure against the pre-change tree and its non-vacuity proof.
7. The exact commands run and their real results, labelled local versus CI.
8. An invariant review against §4, item by item.
9. A failure-and-recovery review against §8.
10. Any deviation from this plan, recorded as a deviation.
11. The registry updates and the **unblock audit**.

## 16. Handoff notes

- **Blocked** on C002 and ADR-0051.
- **The comment at `src/tui/app/render.rs:1567-1572` must change.** It currently states the tab "is a run inspector, not a transcript". After this milestone that statement is false, and a false statement left in the source is a defect in its own right. It must be rewritten to describe the transcript and its three states.
- **Do not re-redact at the seam.** The body was redacted at write. A second recogniser is a second set of holes.
- **Do not synthesize progress.** The projection adapter's `progress: None` stands. Inventing a progress line is the exact failure the existing comment warns about.
- **Test conventions.** Root integration tests are **families**: `cargo test --test <family> -- <member>`. `cargo test --test tui_render` alone runs the whole 1,094-member family; use `--test tui_render -- <member>` to select one.
- **Assertion quality.** Every new test body must assert a checkable property or delegate to a named helper that does, or be recorded in `docs/test-assertion-quality.toml`. `scripts/check_test_assertion_quality.py` and `scripts/check_test_duplication.py` both run in `verify.sh quick`.
- **Scan Rust with `scripts/rust_test_scan.py`**, never with a regex.
- **Budget.** `CARGO_BUILD_JOBS=3` while iterating; 8 for the final workspace clippy.
- **Comment budget.** `scripts/check_comment_budget.py` is a ratchet: re-bank **after** `cargo fmt --all`.
- **Guard wiring.** The extended guards must remain in **both** `scripts/verify.sh`'s `run_guards()` and `.github/workflows/ci.yml`.

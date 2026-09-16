# Context Continuity and Compaction M004 — Transactional Rollover and Multi-Compaction Qualification

Status: ready

Repository baseline: `db3b69d94fa790bf59a9b0ac093572754eb133af`

Source subsystem roadmap:

- `plans/subsystems/context-continuity-compaction-roadmap.md`

Long-term requirements:

- `plans/000-long-term-specification.md#4.2-explicit-ownership`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`

Applicable ADRs:

- None if M001-M003 contracts remain intact.
- Stop and register an ADR if production activation requires changing durable session ownership, public protocol authority, or provider-specific context semantics.

Primary class: correctness / capability / closure

## 1. Objective

Integrate the M001-M003 continuation contracts into the real compaction lifecycle and prove that one logical coding task stays coherent through repeated context-window reductions and restart boundaries.

M004 owns:

- the transactional ordering around checkpoint generation, persistence, replacement-history installation, and durable commit;
- loading the latest installed continuation checkpoint for subsequent turns/restart;
- reconciling the current legacy-vs-hybrid production selection so the documented continuity-aware path is actually used where configured;
- hard-capacity degraded fallback semantics;
- repeated-compaction trajectory tests;
- crash/restart tests at every checkpoint transition boundary;
- final documentation/default activation.

The central invariant is:

> CodeGG must never destroy the active model history first and hope that a usable checkpoint was persisted afterward.

## 2. Why this milestone is dependency-gated

M004 is intentionally last because transactional rollover is only useful after:

- M001 provides durable checkpoint lifecycle/lineage;
- M002 provides authoritative bounded checkpoint content and one-frame semantics;
- M003 provides verified exact evidence references.

Landing rollover before those foundations would either persist incomplete state or create another temporary compaction path.

## 3. Current implementation evidence to re-inspect

Before implementation, re-inspect:

- M001-M003 closure records and final APIs;
- `src/agent/context_runtime.rs::compact_if_needed`;
- `src/context/compaction.rs::compact_context`;
- `src/context/compaction.rs::compact_with_policy*`;
- `src/agent/turn_runtime.rs` prompt assembly;
- `src/agent/loop.rs` request lifecycle and current-turn/user-origin handling;
- `src/agent/follow_up.rs` secondary compaction call site;
- `src/context/volatile_tail.rs` if active behavior changed after baseline;
- `crates/codegg-config/src/schema.rs::CompactionConfig`;
- `src/agent/policy.rs` compaction capacity/threshold resolution;
- session/goal/todo load paths;
- existing cancellation token and compaction hook behavior;
- architecture and tests changed by M001-M003.

Baseline behavior M004 must reconcile:

1. `compact_if_needed()` mutates `messages` only after `compact_context()` returns, which is a useful existing transactional seam.
2. `ContextCompactionResult` already classifies ready/required/compacted/capacity/provider/invalid/cancelled outcomes.
3. provider-backed compaction already receives `ProviderRequestContext` and cancellation support.
4. `ResolvedCompactionConfig` defaults to Hybrid internally, but `compact_context()` only chooses the hybrid engine when `compaction.mode` is explicitly present.
5. when hybrid mode is not explicitly configured, production can still use legacy truncate/summarize/drop-middle behavior.
6. current unit/integration coverage is strong for individual compaction mechanics but does not qualify a long trajectory through many compactions with steering and restart.

## 4. Invariants

M004 MUST preserve:

- no replacement-history mutation before checkpoint candidate persistence and read-back verification;
- only an `Installed` M001 checkpoint is resume authority;
- one current continuation frame after every rollover;
- active current user input always remains visible after a mid-turn/pre-provider compaction;
- host-owned Goal/Todo/plan revisions remain authoritative;
- tool-call/result invariants;
- provider session/request context propagation;
- cancellation;
- hard context-limit availability protection;
- existing permissions, tool authority, workspace state, Git state, and runtime-asset pinning;
- no model-specific private API requirement;
- short sessions that never compact pay minimal/no extra persistence/model cost;
- compaction state does not become a frontend-owned concern.

## 5. Scope

### In scope

- `compact_if_needed()` transaction ordering;
- continuation candidate plumbing through typed compaction results;
- checkpoint prepare/read-back/install event flow;
- latest-installed checkpoint load/injection;
- restart and stale-source validation;
- hard-capacity degraded fallback;
- compaction strategy/default reconciliation;
- multi-compaction trajectory test harness;
- final architecture/config docs;
- closure metrics/diagnostics.

### Out of scope

- arbitrary user-facing `/context epoch` management UI;
- semantic history search;
- cross-session checkpoint reuse;
- external/provider-hosted compaction service;
- distributed checkpoint replication beyond existing storage ownership;
- broad context-packer activation unrelated to this work;
- increasing model context limits.

## 6. Required production changes

### 6.1 Extend typed compaction input/output for continuation candidates

Do not make `AgentLoop` reverse-engineer checkpoint content from final messages.

`ContextCompactionRequest` should receive the captured authoritative M002 baseline and, where needed, the proposed M001 checkpoint identity/evidence-materialization context.

`ContextCompactionResult` / `CompactionOutput` should return enough typed state to persist the final candidate, for example:

```text
messages
continuation_candidate
diagnostics
tokens_before
tokens_after
status
```

The candidate must include the final M002 semantic merge and only verified M003 refs.

No persistence SQL belongs inside pure compaction policy functions.

### 6.2 Transactional rollover ordering

Implement the ordinary successful path as:

```text
A. capture authoritative source revisions/state
B. run deterministic compaction + optional semantic enrichment
C. materialize/verify required evidence refs
D. validate replacement message invariants and post-compaction capacity
E. persist checkpoint as Prepared
F. read back + verify payload digest/schema/parent
G. revalidate authoritative source revisions needed for install
H. replace in-memory/provider-visible messages
I. atomically mark checkpoint Installed + append ContextCompacted event
J. reset tracker / publish bounded runtime diagnostics
```

The exact sequencing of C/D/E may change if M003 needs checkpoint ID before artifact materialization. If so:

- allocate checkpoint identity/candidate before evidence writes;
- do not mark it installed until all required evidence and payload digest are final;
- an abandoned candidate remains `Prepared`/`Aborted`.

The key requirement is unchanged: step H cannot precede durable checkpoint verification.

### 6.3 Source-revision revalidation

Before installation, compare captured host-owned revisions against current state for fields that would make the checkpoint misleading.

At minimum:

- active goal ID/revision;
- plan digest when plan file was used;
- todo state revision/updated timestamp if a stable revision exists after M002;
- latest installed checkpoint parent.

Do not reject installation merely because unrelated telemetry changed.

If a relevant source changed while semantic compaction was running:

- discard/abort the stale candidate;
- rebuild once from fresh state if budget/cancellation permits;
- otherwise keep current history and return a typed stale/retry diagnostic.

Bound retries to one immediate rebuild; do not create a livelock under frequent user steering.

### 6.4 Post-install durable event and in-process event

Use M001's atomic install/event operation.

Continue publishing the existing in-process `AppEvent::CompactionTriggered` for live consumers, but derive its values from the installed result where practical.

Do not let a successful in-process bus publish substitute for the durable commit.

### 6.5 Restart/turn-start recovery

At turn construction, load the latest installed checkpoint for the session.

Validate:

- supported checkpoint schema version;
- payload digest;
- session ID;
- lineage self-consistency;
- optional goal/plan provenance.

Inject its M002 bounded model-visible projection as a dedicated continuation prompt block before the current user turn.

Rules:

- `Prepared` and `Aborted` rows are ignored for resume.
- If there is no installed checkpoint, behavior remains current.
- Current user input/steering is always newer than checkpoint next steps.
- An active goal revision newer than the checkpoint is merged using M002 precedence, not hidden by the checkpoint.
- A missing optional M003 artifact handle does not block turn start.
- A corrupt installed checkpoint emits a diagnostic and falls back to current durable goal/todo/session state rather than failing the session closed unless no safe context can be built.

### 6.6 Hard-capacity fallback

Separate ordinary continuity rollover from emergency availability protection.

When checkpoint persistence or verification fails **before** the model is at hard capacity:

- leave messages unchanged;
- abort candidate;
- emit a bounded warning/diagnostic;
- retry on a later turn/threshold.

When failure occurs at a point where sending unchanged history would exceed the provider's safe capacity:

- use the existing pair-safe emergency compaction/truncation necessary to keep the turn operable;
- inject the best current host-owned M002 frame in memory;
- set `continuity_degraded_reason`;
- do **not** mark a durable checkpoint installed if its persistence/verification failed;
- surface a durable/in-process diagnostic that the epoch continuity guarantee degraded.

This fallback must remain rare and tested.

### 6.7 Reconcile legacy vs hybrid production selection

The reviewed baseline has an implementation/documentation mismatch: Hybrid is the resolved default, but the canonical path only enters the hybrid engine if `compaction.mode` was explicitly configured.

After M001-M003 qualification, reconcile this deliberately.

Preferred outcome:

- explicit `mode=programmatic|agent|hybrid` remains honored;
- when compaction is configured for model-backed auto compaction and `mode` is omitted, use the resolved default rather than an unrelated legacy branch;
- ordinary no-model/no-auto operation may remain deterministic/programmatic;
- legacy helper APIs may remain for compatibility/tests but are no longer the normal production path that generates recursively accumulated free-form system summaries.

Do not remove compatibility functions in the same commit unless caller evidence proves they are dead. Classify them in architecture docs.

If changing the default would materially affect provider cost/latency for users who did not opt into model-backed auto compaction, choose a deterministic programmatic continuity path rather than silently introducing model calls.

### 6.8 Prevent recursive summary dependence

Add an explicit regression invariant:

> checkpoint N+1 is derived from host state + current epoch evidence + the structured semantic fields of the prior installed checkpoint, not by asking a model to summarize the rendered text of checkpoint N.

The prior checkpoint's rendered continuation frame should be stripped before semantic input construction except where its typed fields are supplied separately.

Free-form legacy summaries must not stack indefinitely in system history after the new path activates.

### 6.9 Trajectory qualification harness

Add a deterministic integration harness that can force a small effective context limit without requiring a live provider.

The core scenario must span **at least eight compactions** and include:

- a large initial task/plan;
- goal with completion criteria and plan path;
- multiple todos;
- touched files and commands;
- passing and failing tests;
- a multi-tool assistant turn;
- user steering/correction after at least the second compaction;
- a decision/constraint added after a later compaction;
- one optional evidence artifact intentionally missing;
- daemon/store recreation after at least one installed checkpoint;
- one prepared-but-uninstalled checkpoint at restart;
- semantic provider failure on one compaction;
- final continuation asserting the latest objective/current task/next action.

After every installed epoch assert:

- exactly one continuation frame;
- active goal ID/revision preserved;
- origin provenance preserved;
- newest user steering present;
- old superseded next action absent;
- current decisions/constraints preserved;
- todo/plan phase correct;
- touched files/tests/errors bounded but correct;
- all required evidence refs resolve;
- no orphan tool calls/results;
- tokens are below the configured send budget.

A second test should run the same deterministic source state through compaction repeatedly and assert stable checkpoint digests for equivalent input where timestamps/IDs are excluded from payload hashing.

### 6.10 Observability

Record bounded tracing/diagnostic fields:

```text
session_id
checkpoint_id
checkpoint_sequence
previous_checkpoint_id
tokens_before
tokens_after
checkpoint_bytes
intent_inline_tokens
recovery_ref_count
semantic_enrichment = success|fallback|disabled
continuity = installed|degraded|deferred
reason
```

Never log checkpoint body or evidence content.

## 7. Ordered work packages

### WP1 — Candidate plumbing and persistence seam

Extend compaction request/result types and wire M002/M003 candidate production without changing final installation ordering yet.

### WP2 — Transactional install sequence

Implement prepare/read-back/revalidate/message-replace/install-event ordering plus cancellation and stale-source abort.

### WP3 — Restart/turn-start injection

Load latest installed checkpoint, merge newer goal/current-user state, and inject one continuation block.

### WP4 — Production strategy reconciliation

Make the actual production selection consistent with resolved mode/continuity policy and prevent recursive legacy summary accumulation. Retain/document compatibility helpers as needed.

### WP5 — Hard-capacity degraded fallback

Implement and test the explicit degraded-continuity path without falsely installing a checkpoint.

### WP6 — Multi-compaction trajectory and crash matrix

Land the eight-compaction deterministic test plus targeted crash/restart transition tests.

### WP7 — Documentation, config, and closure evidence

Update architecture/config docs, remove stale claims, and record behavioral/default changes.

## 8. Failure, cancellation, restart, and contention semantics

### Cancellation

- before checkpoint prepare: no durable row;
- after prepare but before replacement: mark aborted when practical; otherwise prepared row remains non-resumable;
- after replacement but before install transaction: current process may have reduced in-memory history, but restart still ignores the prepared row; live execution should attempt install immediately and fail the turn with explicit degraded continuity if it cannot commit;
- after install commit: checkpoint is resumable.

Do not detach a provider-backed semantic compaction after cancellation.

### Restart

Restart recovery uses only `Installed`. It does not infer success from artifact files or a prepared row.

### Contention

If a newer checkpoint installs while one candidate is being built, the older candidate fails the parent/revision check and must not install. One bounded rebuild is allowed.

### Provider failure

Semantic failure uses host-only state; it should not by itself prevent checkpoint installation.

### Storage failure

Ordinary threshold: defer rollover. Hard capacity: emergency pair-safe degradation as specified above.

## 9. Compatibility and migration

No new migration is expected beyond M001 unless accepted M001/M002 contracts require it.

Existing sessions without continuation rows work unchanged.

Existing explicit compaction modes continue to parse.

If the production default selection changes, document the before/after matrix in `architecture/compaction.md` and config docs. Do not silently introduce billable/model-backed compaction for a configuration that previously did not request it.

Old `[codegg compacted session state]` and legacy summary history must remain consumable during transition but should be superseded/cleaned when a new installed checkpoint is created.

## 10. Required tests

### Transaction tests

- prepared checkpoint before replacement;
- read-back digest verification;
- replacement only after verification;
- install event after replacement;
- stale parent abort;
- one bounded rebuild;
- persistence failure leaves history unchanged under ordinary threshold.

### Cancellation tests

Inject cancellation at:

1. before semantic call;
2. after candidate built;
3. after prepared persistence;
4. before message replacement;
5. before install commit.

Assert legal durable state and no false installed checkpoint.

### Restart tests

Restart with:

- no checkpoint;
- installed checkpoint;
- prepared checkpoint only;
- installed + newer prepared;
- corrupt digest;
- unsupported schema;
- missing optional evidence;
- newer Goal revision than checkpoint.

### Strategy tests

- explicit programmatic;
- explicit hybrid;
- explicit agent;
- omitted mode with auto/model configured;
- no-model deterministic path;
- legacy compatibility path not used accidentally.

### Trajectory tests

The eight-compaction scenario described above is required.

Also test:

- repeated compaction frame count == 1;
- no `"unknown"` objective when host knows it;
- newest steering wins;
- prior decisions persist;
- stale next step is removed;
- multi-tool pairs remain valid;
- token budget remains below send limit.

### Negative/security tests

- checkpoint body absent from logs/events;
- evidence remains same-session;
- no hidden reasoning;
- no cross-session checkpoint load;
- hard-capacity degraded event does not claim installed checkpoint.

## 11. Verification commands

Expected narrow targets:

```text
cargo test -p codegg --test compaction
cargo test -p codegg-core -- continuation
cargo test -p codegg -- agent
cargo test -p codegg -- context
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Run the exact new trajectory/restart test target directly and record its command in closure.

No live external model/provider is required for acceptance.

## 12. Documentation

Update:

- `architecture/compaction.md`
- `architecture/context-compaction-ownership.md`
- `architecture/context-ledger.md`
- `architecture/goal.md`
- `architecture/session.md`
- `architecture/agent.md`
- `architecture/config.md`

Document:

- context epoch/checkpoint mental model;
- transaction sequence;
- restart behavior;
- current production strategy selection;
- degraded-continuity behavior;
- recovery-handle behavior;
- bounds and security exclusions.

## 13. Acceptance criteria

M004 is complete only when:

1. replacement history is never installed before a verified prepared checkpoint exists on the continuity path;
2. installed checkpoint + durable event commit atomically;
3. latest installed continuation state is restored on later turns/restart;
4. prepared/aborted rows never become resume authority;
5. source revisions are revalidated before install;
6. stale candidates cannot install over newer state;
7. semantic failure falls back to host state;
8. ordinary storage failure leaves history unchanged;
9. hard-capacity fallback is explicit and does not falsely claim durable continuity;
10. production strategy selection matches documented/resolved policy;
11. recursive/stacked CodeGG compaction summaries are eliminated from the normal path;
12. the required eight-compaction trajectory test passes;
13. restart/cancellation/contention/security tests pass;
14. broad quick verification passes;
15. no new memory/history service or live-provider CI requirement was added.

## 14. Stop conditions

Stop and write a corrective plan/ADR if:

- safe rollover requires changing daemon/session ownership;
- current provider-history persistence makes restart fundamentally ambiguous and cannot be resolved using installed checkpoint + current turn state;
- the implementation must persist hidden reasoning;
- default-path reconciliation would unexpectedly introduce model/provider cost for previously deterministic configurations and no deterministic continuity path is viable;
- M001's install transaction cannot be composed with current session-event storage;
- M002/M003 closure left unresolved medium-or-higher continuity/security defects.

## 15. Closure evidence required

Closure must include:

- final transactional sequence diagram;
- exact production strategy/default matrix;
- implementation commit(s);
- M001-M003 accepted closure references;
- eight-compaction trajectory test command/output;
- crash/restart matrix results;
- cancellation injection results;
- stale-parent/contention evidence;
- hard-capacity degraded fallback evidence;
- provider semantic-failure fallback evidence;
- no-stacked-frame assertion;
- tool-pair invariant assertion;
- verification commands/results;
- remaining limitations classified by severity.

## 16. Handoff notes

This milestone should close the workstream rather than create another context subsystem. If the trajectory harness reveals isolated defects, write bounded corrective plans under the same subsystem instead of widening M004 indefinitely.

Future semantic history search, distributed artifact replication, or user-facing epoch management require separate product justification and are not closure prerequisites.

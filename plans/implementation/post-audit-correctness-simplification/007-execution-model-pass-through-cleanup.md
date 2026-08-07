# Post-Audit Correctness, Simplification, and Footprint Milestone 007 — Execution-Model Pass-Through Cleanup

Status: ready

Source subsystem roadmap:

- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`
- Milestone 007

Repository baseline reviewed: `0323d68e0c37c0495540d39ec0d6d9520f124125`

Primary class: maintainability and code-duplication reduction

Dependencies:

- hard: none
- soft: none

Target closure record:

- `plans/closure/post-audit-correctness-simplification/007-status.md`

## 1. Objective

Reduce unnecessary command-execution representations and compatibility/pass-through layers where they merely mirror another type or forward calls without establishing a distinct invariant.

The intended conceptual pipeline remains:

```text
typed intent -> validated execution plan -> executor -> persisted outcome
```

The milestone succeeds by deleting real complexity, not by renaming or moving it.

## 2. Explicit non-goals

Do not:

- redesign command policy, approval, scheduler, managed-process, Git, Tool Program, or Bash execution semantics;
- combine raw shell and native argv into one ambiguous representation;
- remove provenance, risk, sandbox, ownership, scheduler admission, or persisted run information;
- change public tool schemas or daemon protocol merely to simplify internal types;
- merge `codegg-git` and `egggit`, which have distinct typed-model versus execution/read responsibilities;
- perform a broad module/crate reorganization;
- remove a compatibility layer whose external consumer still requires it;
- add new abstraction traits to compensate for deleting old abstractions.

## 3. Current implementation evidence

Inspect at minimum:

- `src/command_intent/`;
- `src/command_planner.rs` and/or its current successor/re-export surface;
- `src/command_routing.rs`;
- `src/command_outcome.rs`;
- `src/tool/bash.rs`;
- `src/managed_process.rs`;
- scheduler executor translation points;
- `crates/codegg-core/src/run_store.rs` invocation/outcome persistence types;
- Git typed execution integration;
- static execution-ownership guard assumptions;
- tests that assert routing/backend/provenance behavior.

Audit hypothesis:

- some planning modules are compatibility/re-export surfaces rather than true ownership boundaries;
- routing enums/structures may mirror an already-selected `ExecutionBackend` without adding state;
- `ActualExecutor`/`ActualInvocation`-style representations may overlap persistent run invocation/provenance representations;
- several conversion functions may exist solely to translate identical concepts between neighboring modules.

These are hypotheses to verify. A type must not be removed merely because its names look similar.

## 4. Layer-retention test

For each candidate layer/type, answer:

1. Does it represent a distinct lifecycle state that callers can observe?
2. Does it enforce validation/policy that would otherwise disappear?
3. Does it carry information not available in adjacent types?
4. Is it a stable compatibility/public API boundary?
5. Does persistence/provenance require this exact representation?
6. Does removing it create more coupling or circular dependencies than it removes?

Retain the layer when any answer is materially yes.

Delete/collapse it when all answers are no and callers can consume the adjacent canonical type directly.

## 5. Invariants that cannot regress

- native argv remains typed/lossless for the currently supported representation;
- raw shell remains an explicit shell route with distinct risk/policy semantics;
- backend selection remains deterministic and auditable;
- permission/risk decisions are not recomputed inconsistently downstream;
- scheduler admission and daemon ownership remain authoritative where currently required;
- `ManagedProcessService` remains the canonical finite subprocess supervision boundary where established;
- actual backend/provenance and persisted run outcomes remain truthful;
- Git structured/native routing behavior remains intact;
- no new direct `Command::new` bypass is introduced;
- existing execution-ownership/sandbox guards still pass or are updated only to reflect a genuinely simpler equivalent boundary.

## 6. Preferred canonical ownership

Use current repository evidence to identify one owner for each concern:

- command syntax/intent classification;
- validated executable plan including native argv vs shell;
- backend selection/risk/policy result;
- subprocess execution request/result;
- persisted run invocation/provenance/outcome.

When two adjacent types own the same concern, select the more canonical/stable type and migrate consumers.

Do not make one mega-type containing every concern. Separation remains useful when states genuinely differ.

## 7. Candidate simplifications

Evaluate, but do not assume, the following:

- replace pass-through `command_planner` re-export/wrapper surfaces with direct canonical imports when no compatibility consumer exists;
- eliminate routing result types that are isomorphic to already-selected `ExecutionBackend` plus no additional information;
- merge or remove `ActualExecutor`/`ActualInvocation` representations when persistent run invocation/provenance already carries the same data and source use is internal;
- remove duplicate backend-name conversion helpers;
- remove dead fallback variants/routes left after typed argv and managed-process convergence;
- consolidate repeated outcome-to-run-store mapping where one owner can provide it.

Do not remove an enum variant if it corresponds to a real execution mode even when rarely used.

## 8. Ordered work packages

### Work package A — Representation inventory

1. list relevant types and conversion functions from intent through persistence;
2. map producers/consumers and whether each type crosses crate/public/protocol boundaries;
3. mark each layer by the retention test in section 4;
4. identify concrete deletable pass-throughs and expected lines/types removed.

### Work package B — Collapse one boundary at a time

For each accepted simplification:

1. choose canonical owner/type;
2. migrate direct consumers;
3. delete conversion/wrapper/pass-through code;
4. run focused routing/outcome tests;
5. ensure static ownership guards still describe actual architecture;
6. stop the candidate if deletion increases coupling or obscures policy.

Avoid one giant commit that simultaneously changes every execution representation.

### Work package C — Remove dead compatibility/fallback code

1. search for obsolete aliases/re-exports after migration;
2. remove unreferenced conversion helpers and stale comments;
3. preserve compatibility surfaces with real external/test consumers;
4. update architecture docs to show the simplified pipeline.

### Work package D — Duplication review

Inspect only directly adjacent code touched by the migrations for duplicate mapping/match blocks. Extract one helper only when it replaces literal duplicated policy-free conversion. Do not start an unrelated deduplication sweep.

## 9. Quantitative success criteria

Record simple source-level reductions:

- types/enums/aliases deleted;
- conversion/wrapper functions deleted;
- direct imports/call paths replacing pass-throughs;
- approximate net production lines removed.

There is no required line-count threshold. If inspection proves the apparently duplicated layers each own distinct invariants, the valid outcome is a documented no-change decision for those candidates.

Binary-size reduction is not required for M007 and should not drive type ownership.

## 10. Storage, protocol, migration, and compatibility effects

Storage:

- no schema migration expected;
- persisted run fields/meaning must remain compatible.

Protocol:

- no wire change expected.

Compatibility:

- internal Rust paths may change;
- public library APIs should remain compatible unless confirmed repository-private;
- tool behavior, shell/native routing, permissions, errors, and provenance remain equivalent.

## 11. Focused verification

Choose tests based on actual migrated boundaries. Expected categories include:

```bash
cargo test --lib command_intent
cargo test --lib command_routing
cargo test --lib command_outcome
cargo test --lib tool::bash
python3 scripts/check_execution_ownership.py
python3 scripts/check_sandbox_contract.py
scripts/verify.sh quick
```

Adjust selectors to repository reality. If a static guard is modified, run its self-test locally because the guard itself changed; do not infer that this requires restoring guard self-tests to routine CI.

## 12. Static guards

Prefer fewer guards after simplification.

Update `check_execution_ownership.py` only when paths/types it inventories genuinely move or disappear. Do not expand its pattern set to encode every new helper name.

No new duplication or architecture grep guard is permitted by default.

## 13. Acceptance criteria

M007 closes only when:

- every changed/deleted representation has an explicit retention-test rationale;
- at least the concretely redundant pass-through layers found in implementation inspection are removed, or closure records why the audit hypothesis was disproven;
- no policy, provenance, sandbox, risk, scheduler, raw-shell/native distinction, or persisted outcome information is lost;
- direct subprocess bypasses are not introduced;
- no public protocol/storage behavior changes;
- stale aliases/conversions/docs are removed;
- focused routing/execution tests, high-value execution/sandbox guards, and `scripts/verify.sh quick` pass;
- source complexity decreases rather than being replaced by a new generic abstraction layer.

## 14. Stop conditions

Stop a candidate simplification when:

- the layer is a public or cross-crate compatibility contract with active consumers;
- it captures a real state transition or policy result;
- removing it creates circular dependencies or forces higher-level modules into lower-level ownership;
- tests reveal semantics differ despite similar type shapes;
- migration requires redesigning scheduler/tool/managed-process architecture.

Do not force a predetermined deletion count.

## 15. Required closure evidence

`plans/closure/post-audit-correctness-simplification/007-status.md` must include:

- implementation commit/PR;
- before/after representation map;
- each candidate's keep/delete rationale;
- deleted types/functions and net simplification summary;
- proof that execution/provenance/persistence semantics remain intact;
- guard/test/quick-verification outcomes;
- any deferred architectural cleanup explicitly separated from this milestone.

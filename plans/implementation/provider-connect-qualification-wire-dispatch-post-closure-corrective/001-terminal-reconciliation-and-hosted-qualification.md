# Provider Connect Qualification / Multi-Surface Post-Closure Corrective — C001 Terminal Reconciliation and Hosted Qualification

Status: ready for handoff

Repository baseline: `69221d6b4d33e08aecad7ad37edb5bc2da281bd1` on `codex/plan-provider-connect-qualification`

Source corrective addendum:

- `plans/subsystems/provider-connect-qualification-wire-dispatch-post-closure-corrective-addendum.md`

Predecessor work retained as historical evidence:

- `plans/subsystems/provider-connect-qualification-wire-dispatch-corrective-addendum.md`
- `plans/closure/provider-connect-qualification-wire-dispatch-corrective-addendum/010-status.md`
- `plans/closure/provider-connect-qualification-wire-dispatch-corrective-addendum/011-status.md`
- EggPool Shared Provider Profile M001 closure at immutable revision `9ac6a1318e8db3c034b5ab54987317752d5ffea6`

Long-term references:

- `plans/000-long-term-specification.md#13-provider-architecture-and-eggpool`
- `plans/001-terminology-and-domain-model.md`
- `plans/003-planning-process.md#7-corrective-passes`
- `architecture/provider.md`

Primary class: verification / planning reconciliation

No ADR is required. This pass must not change the accepted provider ownership boundary.

## 1. Objective

Strict-close the M010/M011 corrective line by:

1. proving the original operator trajectory at one bounded cross-layer test seam;
2. obtaining hosted CI on the exact corrected head;
3. reconciling stale roadmap/registry status text so current authority agrees with the accepted M010/M011 closures;
4. auditing the two low M011 findings without silently expanding them into unrelated upstream work.

The expected production-code delta is **zero**. A test-only seam is allowed only when necessary to exercise the cross-layer trajectory and must be gated by `#[cfg(test)]` or an existing test-support feature.

## 2. Corrective trigger

M010 and M011 are technically implemented and locally green, but the line is not yet strict-merge-ready for three reasons.

### Finding A — current-authority planning state is contradictory

The parent corrective roadmap still has top-level:

```text
Status: active
```

and its M011 section still says:

```text
Status: blocked.
```

while the same file's terminal status table, both closure records, and `plans/registry.md` state M010 and M011 are closed.

The registry also leaves the parent row marked `active` even though it says both milestones are closed.

Historical M010/M011 closure records must remain immutable. Current authority must instead be reconciled additively through this C001 pass.

### Finding B — hosted CI is absent

The M011 closure explicitly records that hosted CI was not observed. No PR-triggered workflow run is associated with current head `69221d6b4d33e08aecad7ad37edb5bc2da281bd1`.

Local evidence is strong, but this change crosses:

- a storage migration;
- provider connection lifecycle/qualification state;
- agent-to-core inference feedback;
- provider dependency pins;
- direct Responses/Chat/Messages transport;
- shared wire/profile crate boundaries.

Strict closure therefore requires hosted repository verification on a head containing the complete C001 state.

### Finding C — the original user-facing trajectory is covered compositionally, not as one path

Existing evidence proves the individual layers:

- provider provisioning and qualification;
- durable session selection and no-silent-fallback behavior;
- OpenCode Go model-to-wire resolution;
- real socket capture for Chat/Responses/Messages;
- M010 inference outcome reporting.

Repository search found no single test that composes the original failure path:

```text
/connect-like provisioning
    -> persisted connection + bounded executable model catalog
    -> session selects that connection/model
    -> durable runtime/provider resolution
    -> OpenCode Go request reaches the expected wire surface
```

This is not evidence that the path is broken. It is a closure-quality gap because the original report was precisely that `/connect` appeared unable to produce a usable model connection.

## 3. Invariants

- M010 and M011 accepted behavior remains unchanged.
- No live OpenCode Go credential is required.
- No billable request is introduced.
- No production endpoint override is added.
- No arbitrary provider-header injection is added.
- No EggPool daemon/runtime dependency is introduced.
- CodeGG remains the owner of connection state, credentials, transport, lifecycle, and session selection.
- `eggpool-provider-profile` remains the secret-free provider metadata owner.
- `eggpool-wire` remains the wire grammar/stream owner.
- Both EggPool dependencies must remain aligned to the same immutable revision while the profile crate carries a path dependency on `eggpool-wire`.
- Unknown OpenCode Go models continue to fail closed before network I/O.
- Historical closure records 010/011 are not rewritten to hide the later reconciliation.
- The low stale-hint finding in EggPool does not block CodeGG unless a current CodeGG runtime path is proven to consume those old hints.

## 4. Non-goals

- New provider capability.
- New provider-profile data.
- Updating the OpenCode Go model table unless current shared-profile data is demonstrably broken.
- Changing M010 qualification semantics.
- Changing the v69 storage migration.
- General provider factory dependency injection in production.
- Reworking session selection, routing, retries, fallback, or health scheduling.
- Removing stale EggPool `eggpool-wire` hints in this repository.
- Publishing `eggpool-provider-profile`.
- Opening a new provider architecture milestone.

## 5. Required work

### Work package A — Cross-layer OpenCode Go connection trajectory

Add one deterministic integration/trajectory test that proves the original operator path at the narrowest practical boundary.

The test must establish:

1. an OpenCode Go durable connection record with secret reference semantics equivalent to provisioning;
2. a persisted bounded model catalog containing at least one known wire-resolved model;
3. a durable session selection of that exact connection and model;
4. runtime resolution through the existing connection/provider factory path, not direct construction of an unrelated provider;
5. one inference request with a stable session context;
6. capture at a local fake endpoint proving the selected model reached the expected surface/path and auth shape;
7. M010 revision-scoped credential feedback from the successful fake inference;
8. no secret in storage/protocol/loggable diagnostic output.

Preferred representative model: a Responses model such as `gpt-5.6-luna` or `gpt-6-luna`, because Responses was previously unreachable through the one-surface wrapper.

If a second assertion can be added without materially increasing the harness, use a Messages model to prove the `x-api-key` path. Do not build a three-surface E2E duplication; the provider-level capture suite already exhaustively owns per-surface transport.

#### Test seam rule

Use existing fake-daemon/store/provider test seams where available.

If the fixed profile endpoint prevents local capture at this composite layer, add the smallest test-only origin/factory override needed. It must be:

- `#[cfg(test)]` or existing test-support-feature gated;
- impossible to activate from config, protocol, environment, or production CLI;
- scoped to the test construction path;
- absent from the public production provider contract.

Do not introduce a general production provider factory registry merely for this test.

### Work package B — Pin/dependency audit

Prove on the C001 head:

- `eggpool-provider-profile` resolves to `9ac6a1318e8db3c034b5ab54987317752d5ffea6`;
- `eggpool-wire` resolves to that same revision;
- `Cargo.lock` has exactly one `eggpool-wire` package identity;
- no EggPool runtime/account/catalog crate was added;
- CodeGG's durable provider ID remains `opencode_go` and the adapter maps only at the shared-profile boundary.

If upstream EggPool `main` has advanced, do **not** opportunistically repin. This corrective qualifies the accepted M011 revision, not latest upstream.

### Work package C — Planning/documentation reconciliation

Update the parent roadmap:

`plans/subsystems/provider-connect-qualification-wire-dispatch-corrective-addendum.md`

to state:

- top-level status: closed, with M010/M011 closure references;
- M011 section: closed, not blocked;
- dependency graph: historical prerequisites satisfied;
- completion definition: satisfied by accepted M010/M011 closures;
- terminal status table remains aligned.

Update `plans/registry.md`:

- parent corrective row becomes `closed/current` or the repository's equivalent terminal wording;
- new post-closure corrective C001 is `active` during execution and `closed` at acceptance;
- no M010/M011 row remains in ready/active/blocked tables;
- current gate paragraph names C001 as strict merge/closure authority until it closes.

Do not edit the contents of the M010/M011 closure records except, if required by convention, a minimal forward pointer that does not alter historical claims.

### Work package D — Low-finding disposition audit

Audit the two M011 low findings.

1. EggPool `eggpool-wire` stale OpenCode hint rows:
   - prove CodeGG dispatch reads only `eggpool-provider-profile`;
   - if no runtime reader exists, record as non-blocking upstream documentation/data debt;
   - do not modify EggPool from this corrective.
2. Opaque `ProviderProfile::runtime_capabilities` passthrough:
   - prove CodeGG does not consult it for wire/auth/qualification decisions;
   - record no-action disposition if true.

If either low finding is actually on a production decision path, stop and register a separate bounded corrective rather than silently broadening C001.

### Work package E — Hosted strict qualification

Run the complete local gates first, then obtain hosted CI on a pushed C001 head.

Required hosted evidence:

- root `CI / verify` — SUCCESS on the C001 head or a commit with byte-identical production/test tree plus only closure-document changes;
- any provider-specific/path-gated workflow triggered by the changed test/source paths — SUCCESS.

If the repository's CI does not run on a plain branch push, open a PR for the existing branch or use the repository-supported workflow trigger. Do not merge merely to obtain CI evidence.

Record workflow run IDs and exact tested SHA in the C001 closure record.

## 6. Required tests

At minimum:

```text
new cross-layer OpenCode Go connection trajectory
existing core::eggpool provider-connection suite
existing provider_qualification suite
existing opencode_go provider capture/catalog suite
existing provider_profile suite
existing wire suite
existing session-selection suite
```

The cross-layer trajectory must fail against a synthetic regression where either:

- OpenCode Go is constructed as the old single-surface compatible provider; or
- the selected durable model is not carried to runtime resolution.

A test that merely calls `OpenCodeGoProvider::new()` directly does not satisfy this corrective.

## 7. Required verification

Run and record the exact available target names, including at least:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings

cargo test -p codegg-providers --lib
cargo test -p codegg --lib core::eggpool::tests
cargo test -p codegg --lib provider_qualification
# exact session-selection target(s) present in the tree
# exact new cross-layer trajectory target

python3 scripts/check_provider_qualification.py --verbose
python3 scripts/check_provider_qualification.py --self-test
python3 scripts/check_provider_multi_surface_dispatch.py --verbose
python3 scripts/check_provider_multi_surface_dispatch.py --self-test
python3 scripts/check_provider_wire_boundary.py

scripts/verify.sh quick
git diff --check
```

Also inspect:

```bash
cargo tree -p codegg-providers -i eggpool-wire
cargo tree -p codegg-providers | grep -E 'eggpool-(wire|provider-profile)'
```

or repository-equivalent commands proving one aligned shared-wire package.

## 8. Hosted-CI failure policy

A hosted failure is not automatically evidence that M010/M011 are wrong.

Classify every failure:

- attributable M010/M011/C001 regression — fix within C001 only if narrow and in scope;
- existing reproducible main failure — record baseline evidence, do not absorb it;
- environmental/flaky — require same-SHA rerun evidence before classification;
- unrelated new product defect — register a separate corrective.

Do not weaken Clippy, tests, guards, or workflow coverage to achieve green status.

## 9. Compatibility and migration

No new storage migration is authorized.

Migration v69 remains the accepted M010 schema transition and must be exercised by existing migration tests.

No provider ID, connection ID, model ID, DTO field, or credential format change is expected.

Any test-only injection seam must not alter release builds or serialized/public API.

## 10. Documentation updates

Expected files:

- `plans/subsystems/provider-connect-qualification-wire-dispatch-corrective-addendum.md`
- `plans/subsystems/provider-connect-qualification-wire-dispatch-post-closure-corrective-addendum.md`
- `plans/implementation/provider-connect-qualification-wire-dispatch-post-closure-corrective/001-terminal-reconciliation-and-hosted-qualification.md`
- `plans/registry.md`
- `plans/closure/provider-connect-qualification-wire-dispatch-post-closure-corrective/001-status.md` at closure

Architecture/provider docs should change only if the audit finds a real mismatch. Do not churn already-correct architecture prose for status bookkeeping.

## 11. Acceptance criteria

C001 may close only when all are true:

1. One cross-layer test proves persisted OpenCode Go connection/model selection reaches the correct multi-surface provider runtime without live credentials.
2. The trajectory uses the durable connection/provider resolution path, not a direct unrelated provider constructor.
3. The test exercises M010 inference credential feedback at the selected connection revision.
4. Any new injection hook is test-only and unreachable in production.
5. Both EggPool crates resolve to the accepted `9ac6a1318e8db3c034b5ab54987317752d5ffea6` revision with one `WireSurface` identity.
6. No EggPool runtime crate has entered CodeGG.
7. Parent roadmap and registry both report M010/M011 terminally closed.
8. The new C001 row is terminally closed with no stale ready/blocked row.
9. Both M011 low findings have explicit non-blocking dispositions or have spawned separately registered work.
10. Local provider/core/session/guard/quick gates pass.
11. Hosted root `CI / verify` passes on the qualified C001 head.
12. No unresolved critical/high/medium provider-connection defect remains.

## 12. Stop conditions

Stop and register a narrower follow-up if:

- the cross-layer test requires a production endpoint/factory override;
- session selection does not in fact resolve the durable provider connection used by normal turns;
- the selected model can bypass the wire-resolved catalog filter;
- inference success/failure cannot be attributed revision-safely through the real turn path;
- hosted CI reveals an attributable production defect larger than a local corrective;
- the stale EggPool hints are discovered to influence CodeGG runtime dispatch;
- closing the line would require changing shared provider-profile semantics.

## 13. Closure evidence required

Create:

`plans/closure/provider-connect-qualification-wire-dispatch-post-closure-corrective/001-status.md`

with:

- baseline and implementation commits;
- exact new trajectory test and the layers it crosses;
- proof the test uses durable connection selection/runtime resolution;
- test-only seam security disposition;
- dependency-tree single-`WireSurface` evidence;
- M011 low-finding audit;
- roadmap/registry reconciliation diff;
- local verification commands/results;
- hosted workflow run IDs, SHA, and result;
- main-baseline comparison for any unrelated failure encountered;
- unresolved findings by severity;
- final merge recommendation.

## 14. Handoff notes

This is a terminal corrective, not the start of a new provider feature line. If it closes cleanly, the provider connection qualification/multi-surface workstream should have no active successor and the branch is eligible for merge under the repository's normal review process.

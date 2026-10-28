# Agent Worktree Placement and Build Affinity A005 — Conservative Placement Advisor

Status: **closed (no-go)** — closure `plans/closure/agent-worktree-placement-and-build-affinity/005-status.md`
Repository baseline: `493e5ee11` (`plan/worktree-affinity-cwd-build-admission-20261027`)

Source roadmap:
- `plans/subsystems/agent-worktree-placement-and-build-affinity-corrective-addendum.md` §7 A005

Long-term requirements:
- `plans/000-long-term-specification.md` (workspace, agent orchestration, security, and resource predictability)
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md` (agent workstreams and scheduler admission)

Applicable accepted ADRs: ADR-0002, ADR-0032, ADR-0047, ADR-0048, ADR-0055. Primary class: **capability / polish**; the advisor is explanatory and cannot grant placement authority.

## 1. Objective

Give the delegating model a conservative, evidence-based rule for choosing the already explicit task placement. Make the choice and its reason visible in the accepted task response. Preserve `Auto` as the default, never infer a user-approved checkout or shared-write permission from task wording, and do not add compiler-cache or shared-target behavior.

## 2. Why this milestone is ready

- A002 implements typed `existing_managed_read_only` by exact managed `WorktreeId`, with an exclusive run-owned lease and a closed read-only tool surface.
- A003 provides optional repository-scoped admission for supported Build/Format jobs.
- A004's three-repetition production-scheduler Cargo pilot is recorded at `plans/validation/agent-worktree-placement-and-build-affinity/004-cargo-sccache-economics.md`. Its no-go result found zero Rust sccache hits and material CPU/wall overhead, so A005 must recommend separate writable Cargo targets and must not suggest a cache or shared target.
- Recheck closure records and registry state before executing. A005 has no dependency on a production cache integration.

## 3. Evidence basis and limits

The evidence supports only these bounded rules: keep default placement when the user has not directed a special checkout; use a new isolated managed checkout for work expected to mutate files; use an existing checkout only for inspection when an exact authorized ID was supplied; use a serialized shared group only within that restricted profile and never for process-class builds. Repository quotas govern eligible scheduler builds and are configuration-owned.

The A004 sample is a macOS ARM64 Cargo 1.89 pilot, not a universal performance study. It supports a no-cache recommendation for that measured sccache setup, not a claim that every compiler cache is uneconomic on every platform. No automated prose classifier, workload predictor, dynamic cache affinity, or cross-principal scoring is justified by this sample.

## 4. Invariants

1. Placement intent remains explicit and typed. The advisor may explain a selected mode but cannot select a path or turn a model-supplied ID into authorization.
2. `Auto` remains backward-compatible and unchanged by default.
3. Existing managed read-only selection requires an exact per-child ID and all A002 authorization, worktree health, active-owner, lease, and allowlist checks.
4. Mutation stays in a new isolated checkout unless the user explicitly selects a separately authorized supported mode. No automatic shared-checkout write or process authority.
5. `serialized_shared` remains limited to its existing profile; Rust/Cargo processes are not admitted there.
6. Build admission remains operator configuration. A model may not request a larger quota, change repository identity, share a target directory, or enable a cache.
7. Advisor output contains no absolute path, secret, environment value, or unapproved cross-workspace metadata.

## 5. Scope

**In:** concise task-tool/schema guidance for placement selection; a deterministic summary of selected placement and rationale in spawn/spawn_many acceptance output; tests and documentation; controlled before/after evaluation using approved real-model routes and bounded fixtures.

**Out:** changing the scheduler's placement default; classifying arbitrary natural-language tasks into writable/read-only intent; selecting a worktree automatically; automatic cache selection, cache service lifecycle, shared Cargo targets, new quota controls, expanded serialized-shared permissions, or new user-facing approval grants.

## 6. Required implementation

### Task tool advice

State the selection rules in the task schema and tool guide:

- `auto`: choose when the user did not direct an alternate checkout strategy.
- `isolated_new`: choose for independent edits/build fixes when isolated work is desired or requested; it creates a distinct managed checkout.
- `existing_managed_read_only`: choose only when the user specifically requests inspection of an existing managed checkout and the caller has its exact `worktree_id`; this mode cannot edit, build, or run child tasks.
- `serialized_shared`: use only when the caller deliberately formed a supported group and the restricted profile is suitable. It cannot run arbitrary Cargo/build processes.

Include a short rationale alongside each accepted child so the next model action does not mistake requested mode for inferred authority. Report a refusal without suggesting a fallback checkout.

### Resource affinity

Keep writable target directories separate. The task tool does not promise compiler-cache reuse. Supported Build/Format jobs use configured repository-scoped admission. Report no cache benefit; any future cache integration requires a separate plan and evidence with Rust hits, net time/CPU/disk gains, trust partitioning, containment qualification, and lifecycle.

## 7. Real-model qualification

Use the repository's Harness Lab native evaluation path, not a mocked provider, for the required before/after comparison. Freeze the harness commit, task fixtures, evaluator, model/provider route, request limits, and cost/time ceiling before the first request. Do not place credentials in receipts.

For A005, the preregistered baseline is an **A002-compatible schema ablation**: both arms run the same current source/binary with identical scheduler, tools, placement authority, and provider budget gate. The baseline arm receives the generic typed placement contract; the candidate arm receives the full strategic placement guidance. `CODEGG_HARNESS_DISABLE_PLACEMENT_ADVISOR` is set only in baseline trial environments that also carry the provider allowance gate. The receipt records this arm setting. This avoids using a pre-A002 binary that would have changed the authority under test. Equal binary hashes are expected and do not imply equal advisor input.

The workload set must include:
- a Rust dependency-heavy inspect-only task;
- a Rust edit/build task that runs through the supported scheduler Build path;
- a Python read-only inspection and a bounded Python edit/debug task;
- a multilingual user request whose task intent is unchanged by translation;
- a busy/dirty/unavailable existing-worktree case that must refuse without silently rerooting.

Use paired randomized baseline/advisor arms across at least three repetitions per case, with a hidden verifier. Record task success, verifier result, placement requested and resolved, refusal/fallback/collision outcome, end-to-end and scheduler wait time, provider request count/cost where available, CPU/RSS/disk/target growth, and any build-quota delay. The advisor arm must not win by receiving extra permissions or a different tool set. Report medians, ranges, and every failure; count a refusal as correct only when the fixture's expected policy requires refusal.

**Authorization gate:** provider-backed work requires an operator-approved route and explicit request/time/spend limit. On 2026-10-28 the operator authorized OpenCode Go and a cumulative ceiling of 480 requests, 120 minutes, and $10 of provider-reported Go allowance, and accepted a low-cost model such as `muse-spark-1.3-contributor`. OpenCode reports that allowance in `usage.opencode.allowance`, distinct from cash charged. CodeGG and Harness Lab carry and enforce that field; missing allowance data refuses further provider work. Do not substitute an estimated price for the allowance cap or fabricate real-model evidence. The provider can only report a response's allowance after that response completes, so at most one in-flight response may cross the remaining allowance ceiling.

## 8. Failure and contention

A refused placement remains refused, never falls back to the parent or an arbitrary path. Busy same-checkout selection has one exclusive lease winner. Independent isolated worktrees may execute concurrently subject to scheduler quota. Quota waits carry no process permit. Cache unavailability or low predicted benefit cannot change placement authority.

## 9. Required tests

- Schema/tool-description tests assert the four recommendation rules and state that build/cache use is not authorized by placement.
- Parser and spawn output tests cover every placement, per-child IDs, explicit rationale, and typed refusal without fallback.
- Existing A002 production scheduler, lease contention, generation release, read-only allowlist, and refusal tests stay green.
- Scheduler tests prove repository quota remains configuration-owned and a Rust job cannot widen it.
- Harness dry-run/fixture validation proves paired-arm identity, hidden verifier, randomized order, denominator, cost ceiling, and receipt redaction before any provider call.
- Provider and Harness Lab fixtures prove streamed and Responses usage decode `opencode.allowance`, the ACP receipt carries it on success/failure, and the per-request boundary refuses another OpenCode Go request at the configured request or remaining allowance cap.
- The paired comparison schedule randomizes baseline/advisor arm order and shares one request, allowance, and wall-clock budget across both arms and all repetitions.
- With A002 strictly closed and the authorized allowance gate implemented, run the frozen paired multi-language and Rust workload set; evaluate all failures and collisions against the pre-registered rubric. This qualification is complete and recorded as a no-go in `plans/validation/agent-worktree-placement-and-build-affinity/005-live-placement-comparison-20261028.md`.

## 10. Required verification

```bash
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 scripts/check_existing_worktree_read_only.py --self-test
python3 scripts/check_existing_worktree_read_only.py
cargo test --test agent_loop workspace_root_execution_authority::existing_managed_read_only_runs_through_the_production_scheduler
cargo test -p codegg-core --lib read_only_lease
cargo test -p codegg --lib placement_rationale_preserves_conservative_authority_boundaries
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Validate each family/member selection before treating a zero-test exit as evidence. The opt-in live run is not part of routine CI.

## 11. Acceptance and decision

The deterministic advice is useful only if the model follows user intent without increasing authority, keeps placement failures fail-closed, and yields no correctness regression in the paired real-model evaluation. A positive performance claim requires the full cost and resource denominator. A no-go is acceptable if the advisor worsens success/collisions/cost or no stable rule improves outcomes. The completed evaluation produced no independently verified task success, and the placement advisor showed no substantive correctness improvement. Its favorable reliability and duration movement is advisory under the frozen scoring contract; the result is a no-go, not acceptance.

## 12. Closure evidence

Record the implementation source revision, exact focused and repository verification, placement output examples, paired receipts with secrets removed, per-language success/collision/cost table, evidence limits, and a go/no-go rationale. The live qualification and deterministic implementation are closed as no-go: the data do not support a positive placement-strategy or performance claim. The machine-readable receipts disclose the scoring-only correction that classified the unchanged unavailable-checkout scenario as a protected negative control and reran its six paired rows.

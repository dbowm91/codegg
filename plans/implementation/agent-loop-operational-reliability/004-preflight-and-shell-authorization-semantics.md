# Agent Loop Operational Reliability M004 — Preflight and Shell Authorization Semantics

Status: ready for handoff

Repository baseline: `f37840ef5fca36d44b35daec5da6b26cd049a3ee` (main; verify actual starting head)

Source roadmap: `plans/subsystems/agent-loop-operational-reliability-roadmap.md#7-milestones`

Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `plans/000-long-term-specification.md#28-observability`; `plans/000-long-term-specification.md#29-system-invariants`; `plans/001-terminology-and-domain-model.md#7-agent-terms`.

Applicable ADRs: `plans/adrs/ADR-0028-...` accepted command-policy direction (resolve exact filename before implementation). ADR-0040 does not gate this milestone.

Primary class: invariant

## 1. Objective

Make model-facing preflight `block/warn` reports and actual `bash` admission decisions semantically consistent and actionable, without elevating a heuristic risk classifier into security authority or weakening real Deny/sandbox boundaries.

## 2. Why this milestone is ready

Command authorization, ApprovalRouter, shell execution and eggsact preflight are existing production owners. The prior ADR-0028 policy accepts low-friction normal commands and rules out regex-only security gates. No new ADR is required to clarify advisory vs authoritative results.

## 3. Current implementation evidence

- `src/agent/tool_batch.rs` calls `PreflightService::check_command()` but stores only summary on findings in `CommandPreflightResult`; that summary contributes to `Escalate`, not an authoritative scanner-enforced Deny.
- `src/tool/bash.rs`: when an authorization receipt exists, reuse its preflight summary as a warning; standalone invocation performs separate check and returns `ToolError::Execution` for `PreflightDecision::Block`. These entrypoints can behave differently for identical commands.
- `src/tool/bash.rs` also calls `check_command_security` and validates sandbox, root and child ceiling pre-spawn. Explicit Deny and security policy have their own precedence in `src/agent/tool_batch.rs`.
- The earlier bug where `Some(preflight)` escalated every command is already corrected; tests must pin it.

## 4. Invariants

Explicit saved Deny, sensitive-path hard gates, immutable snapshot, parent/child ceiling, sandbox-required fail-closed checks, authorized workspace and effective approval decision remain binding. No command parser can claim network/process containment; no model-facing preflight conclusion may supersede those controls. Current inert-fixture safety rules apply to shell tests.

## 5. Scope

In: unify typed preflight result semantics across harness and standalone calls; distinguish scanner recommendation, security-policy decision and final execution verdict; report concise reason/source; ensure shell receipt reuse is consistent and not rescanned into contradiction.

Out: replacing eggsact, rewriting the sandbox, allowing arbitrary restricted commands, reintroducing default regex blocklist or removing permission prompts.

## 6. Required production changes

**Contract:** `PreflightDecision` should be represented as a typed advisory/risk classification with findings and a recommended action, not an indistinguishable binary hard Deny. If standalone preflight remains configured to block, label that *as a separate caller policy* and ensure equivalent configured policies behave equivalently. Explicitly document which actor may enforce a scanner-issued block, vs `ApprovalRouter` Deny or sandbox failure. Do not discard the original action when collapsing to `summary` if it is relevant to diagnostics.

**Execution:** consolidate any duplicated evaluation/scan dispatch so an authorized call cannot be denied by a later conflicting scanner observation; avoid removing hard pre-spawn security checks. Preserve the safe default when eggsact unavailable: no silent bypass of explicit hard policy, no escalation on clean advisory.

**Feedback:** return `allowed`, `requires_approval`, `denied` and independent advisory findings, plus bounded, actionable explanation. The model must not interpret `preflight: block` as actual Deny when the authoritative policy granted execution.

**Compatibility:** additive status fields if needed; preserve historical tool schema and audit receipts, no storage migration.

## 7. Ordered work packages

A. Build a test matrix of shell commands categorized by clean, advisory risk, security hard deny, stored allow/deny, approval ask, snapshot change and sandbox unavailability. Use inert examples or mocks for destructive patterns; never actually execute catastrophic shell payloads.

B. Capture harness vs direct tool contradictions under identical *effective* policy. Where intentional differing policies exist, make the reason visible and documented.

C. Reconcile typed preflight recommendation and authoritative `ApprovalRouter` decision; keep fallback and missing-service behavior deterministic.

D. Add compact audit fields and docs; verify no escalation of every clean `bash` call and preserved Deny under Yolo/Automatic/Ask.

## 8. Failure, cancellation, restart, and contention

An approval receipt pins the batch snapshot; a later policy change must not retroactively bless execution. Cancellation before spawn prevents execution; late preflight cannot resurrect denied command. Concurrent approvals and persistent grant versioning preserve existing semantics. If advisory service fails, classify unavailable explicitly while hard denies still apply.

## 9. Compatibility and migration

No DB migration. Existing stored grants and per-session approval modes retain semantics. Standalone-direct and agent-broker difference is either reconciled or explicitly policy-driven with an operator-visible reason. Do not reinterpret existing audit text as a hard denial.

## 10. Required tests

Focused: `tool_batch` permission/preflight tests, `bash` harness-authorization path, `preflight` service. Integration: same command under matching policy from both callers, compare final authorization and advisory outcomes. Restart: saved Deny/AlwaysAllow still authoritative. Contention/cancel: policy snapshot drift and concurrent approval, timeout. Security negatives: explicit Deny, required unavailable sandbox, child ceiling, symlink/sensitive path, scanner failure; inert high-risk examples never spawn.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

First run focused test filters for the named modules (`cargo test -p codegg --lib <filter> --locked`) and existing integration targets; confirm actual crate/target names at execution time. Record exact commands, stdout/result and head in closure. Hosted checks only where the existing closure policy requires them. No live-provider CI or new scheduled test lane.

## 12. Documentation updates

Update only affected `architecture/` and `docs/` surfaces; record any compatibility/config changes in `docs/tools.md` or `architecture/provider.md` where applicable. Keep existing predecessor closure history immutable.

## 13. Acceptance criteria

1. Identical effective security policy yields consistent decision, with any caller-specific difference explicit, typed and documented.
2. Routine in-workspace commands execute without accidental preflight escalation; preflight findings are still reported.
3. No saved hard Deny, sandbox boundary, approval or child authority is bypassed.
4. Failing-before/passing-after tests include the observed "scanner block but command ran" explanation without treating advisory text as authoritative policy.

## 14. Stop conditions

Stop and record a blocker if implementation would need a new authority owner, broad sandbox changes, unsafe command execution in tests, an undocumented provider wire workaround, a nonadditive migration, or an unaccepted architectural decision. A test-only mitigation with a production gap is not closure.

## 15. Handoff

At start reconcile this plan against actual code and exact head. Preserve unrelated changes. Commit scoped production/tests/docs and add `plans/closure/agent-loop-operational-reliability/NNN-status.md` with requirement/evidence table, exact commands and outcomes, restart/contention/security proof, and residuals. Update registry and roadmap to reflect actual closure or blocker only.

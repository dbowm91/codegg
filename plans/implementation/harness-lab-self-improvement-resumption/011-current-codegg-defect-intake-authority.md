# Harness Lab Self-Improvement S011 — Genuine Current-Defect Intake and Eligibility Authority

Status: **closed offline**. Primary class: **infrastructure**. Closure: `plans/closure/harness-lab-self-improvement-resumption/011-status.md`. The trusted current-source reproduction and validation completed in two private native lanes; the current source has zero eligible real defects. Live campaigns remain operationally blocked.

Authority: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/001-terminology-and-domain-model.md`; `plans/003-planning-process.md`; accepted `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`; `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md`. Existing S001–S009 closure records are historical evidence, not re-opened milestones.

Baseline reviewed: `main@2b3785bfd743325f08149e9b6eb41828e823e4df` (2026-10-30), containing PR #219 and S001–S009 closure artifacts. At implementation, re-read `origin/main`, its CI, existing user changes and current `dev/harness_lab` interfaces; use the actual resulting source SHA in every receipt. Do not assume this planning baseline remains HEAD.

Ownership: development-only Python `dev/harness_lab` controls evaluation, qualification, admission and receipts. Native CodeGG daemon/broker remains authority for tools, permissions, jobs and ACP; `opencode_go` is the sole CodeGG provider adapter in scope; OpenCode CLI is an external proposer and never the evaluator.

## 1. Objective and evidence gap

Extend the synthetic-only `build_generic_dossier` path in `dev/harness_lab/defect_intake.py` into a **trusted, reproducible, real current-CodeGG defect intake**. S009 established synthetic positive schema mechanics but intentionally returns `campaign_eligible: false` for all generic inputs. S003/S006 correctly refused H007's seven runs; historical C003 is not a fresh defect. A real new bug must be admitted without allowing self-asserted JSON, an agent-authored verifier, a model/task failure, a provider timeout or a stale historical receipt to authorize patches.

Intake proves **defect eligibility**; it does not confer live provider, source-write, model, cost or merge authorization. Even an eligible dossier is subject to separate operator approval and admission at S013.

## 2. Non-regression and ownership

- `defect_dossier_v1` for the seven immutable H007 receipts and `generic_defect_dossier_v1` for synthetic S009 fixtures retain their exact outputs and meanings. Add a distinct versioned `current_codegg_defect_dossier_v1` or equivalent; no reinterpretation of old evidence.
- Distinguish product-harness fault from tool-use quality, task failure, user-workspace errors, provider route failure, verifier setup failure, timeout and uncertain outcome. Only a **native CodeGG behavior** that violates a frozen contract qualifies.
- Agent/model must not author the trusted verification verdict or elevate its own request. The trusted controller independently runs an operator-approved known verifier/test command against the identified current source/binary. Avoid executing arbitrary shell content from untrusted manifest fields; map allowlisted test identifiers to controller-owned commands.
- Require at least two distinct private-lane failing-first observations from the same exact source/binary and failure signature (plus explicitly scoped controlled variation where relevant). Repeated failures on a shared process/fixture do not count as independent lanes.
- Preserve pinned baseline revision, executable digest, policy/profile, case, allowed tools, route (or `none` for deterministic offline), test and independent verifier digests; candidate-scope prefixes may include only product source and must exclude `dev/harness_lab/`, all verifier/holdout sources, immutable evidence, generated billing/admission receipts and `plans/`.
- Fail closed on mixed revisions, unknown outcomes, missing evidence, symlink traversal, duplicate lane identity, unverifiable signatures, evaluation contamination or unsatisfied protected controls. No automatic source modification on intake.

## 3. Expected changes and schema

Implement the generic real path alongside, not on top of, the frozen H007/synthetic builders. Suggested inputs: `defect_id`, `owner=codegg_harness`, `evidence_kind=current_codegg_defect`, exact source and binary SHA, executable command ID, independently frozen expected result, test/holdout digests, structured red native receipts with run+daemon+lane identity, stable failure signature, verifier terminal and candidate patch prefixes. Bind all hashes into a canonical dossier identity. Store an explicit bounded provenance record showing **who/what performed the trusted reproduction**; no bare boolean `verified: true` from a supplied file is authoritative. Separate `reproduction_validated`, `defect_eligible`, and `live_authorized` (always false at intake).

Prefer a controller-owned attestation built from actual observed subprocess/test results under private roots. If operator-controlled attestation is necessary to establish trusted inputs, design it as a separate admission command with reproducible execution—not an arbitrary manifest string or synthetically blessed source. Keep the allowlisted execution contract testable without credentials or network.

## 4. Ordered work packages

A. **Freeze contract and threat model.** Enumerate agent-controlled vs controller-controlled inputs, trusted source/binary/verifier authority, protected scopes, symlink behavior, stale checkout, path-hardening, and replay identities. Document what is and is not proof of a current CodeGG defect.

B. **Build native reproducible intake.** A trusted local command executes an allowlisted failing-first test/verifier against the exact pinned current CodeGG executable in two separate private lanes. Emit red receipts and refuse if the source is unclean/mismatched, both failures are not identical in mechanism, or the verifier cannot distinguish product defect from setup. The command must not execute a supplied arbitrary shell command.

C. **Bind frozen holdouts and scope.** Create a content-addressed dossier with no candidate-accessible verifier or holdout, explicit protected negative controls, allowed source prefixes and a current-revision freshness policy. A verified current defect may be `defect_eligible: true` **only** with independent trusted observations; it is never automatically `live_authorized`.

D. **Exercise positive and adversarial paths.** Use a private synthetic *production-shaped* native CodeGG fault-injection fixture to exercise the positive contract. Report it as `synthetic_fixture` and ineligible for real-campaign promotion; separately test a bona fide current CodeGG regression when available and do not manufacture one. Malicious input tests include substituted verifier, hash mismatches, symlink/path escapes, result forgery, contaminated candidate source, mixed routes/revisions, skipped controls, single flaky failure and a historical H007 record.

E. **Operator/CLI integration.** Expose read-only `intake validate/report` and trusted reproduction mechanics under the existing `dev.harness_lab` CLI; do not change `optimize` default or allow intake to spend provider requests. Document how a dossier is passed to the S013 campaign preflight after separate approval.

## 5. Verification and stop criteria

Focused tests for positive native *synthetic* path and all refusal paths must execute, not just validate JSON shape. A forged dossier containing all expected static fields must still be refused unless the controller's own independent verification ran and matched. Verify stable identity under repeated exact input and different identity under changed source/verifier. Confirm H007 and S009 byte/semantic invariance.

```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
git diff --check
scripts/verify.sh quick
# Run scripts/verify.sh full and exact-source-head hosted CI when production/integration code changes.
```

## 6. Storage/protocol/migration/security

Add a new local developer-only versioned dossier and, if necessary, controller-owned receipt schema. No production daemon/ACP schema changes, no migration or rewrite of S004/S009 evidence; old readers remain valid. Credentials absent in offline tests; redact sensitive paths, outputs and secrets. Restrict reads/writes to operator-approved repository roots; never trust relative path normalization alone against symlink escape or changed files during verification. For current-defect evidence, pin and recheck hashes at admission and again before campaign execution.

## 7. Closure

Strict closure is the proven mechanism and tests for a **trusted current-defect path**, including verified non-promotion for all synthetic/historical cases, not the existence of a genuine defect today. If real reproducible current defects are absent, record `eligible_real_defects=0`; do not falsify a positive current-case result. `stable_offline` may be claimed only after S010 integration and this intake contract meet the S006 promotion standard. Stop on self-attesting evidence, arbitrary command execution, inaccessible holdouts, weaker production scope, or inherited authorization. Closure: `plans/closure/harness-lab-self-improvement-resumption/011-status.md`, with schema/authority evidence, red reproductions, negatives, tests/CI, and precise remaining eligibility count.

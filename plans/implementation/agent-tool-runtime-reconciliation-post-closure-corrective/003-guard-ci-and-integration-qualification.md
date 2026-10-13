# Agent Tool Runtime Post-Closure Corrective C003 — Guard, CI and Integration Qualification

Status: conditionally closed — accepted at `plans/closure/agent-tool-runtime-reconciliation-post-closure-corrective/003-status.md`; current-main full suite has four reproducible upstream TUI failures, hosted CI failed on upstream Clippy, and the exact-ref Desktop E2E WebDriver trajectory is still running

Repository baseline: `a54d18c7e7b92d1c9ac4971d7cdb8cc916418a5d` implementation audit; current `main` at audit `b9ba494c1b21638c2e2f660301ba207ce8859682`; final integration base `667005a66e491efae6a28fca43931e05e94214f8`.

Source roadmap: `plans/subsystems/agent-tool-runtime-reconciliation-post-closure-corrective-addendum.md#4-milestones-and-dependencies`.

Predecessor plans / closure records:

- `plans/implementation/agent-tool-runtime-reconciliation-post-closure-corrective/001-contract-safe-tool-retry-admission.md` and eventual `plans/closure/agent-tool-runtime-reconciliation-post-closure-corrective/001-status.md`.
- `plans/implementation/agent-tool-runtime-reconciliation-post-closure-corrective/002-descendant-effect-and-nested-task-corrective.md` and eventual `plans/closure/agent-tool-runtime-reconciliation-post-closure-corrective/002-status.md`.
- Historical `plans/closure/agent-tool-runtime-reconciliation/001-status.md` to `004-status.md`.

Long-term requirements:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md#7-corrective-passes`.

Applicable ADRs: existing CI/testing, broker and agent authority decisions; no new ADR for guard/qualification wiring.

Primary class: infrastructure.

## 1. Objective

Turn the locally verified four tool-runtime static guards plus C001/C002 correctness regressions into enforced quick/hosted verification, reconcile the implementation with the current `main` safely, and produce exact-revision closure evidence before recommending integration.

This is a **qualification gate**, not another runtime-feature milestone. It cannot claim readiness before C001 and C002 are accepted.

## 2. Why currently blocked and what is already ready

Hard dependencies are C001 no-replay and C002 descendant effect/nested delegation closure. Interface dependencies already exist for `scripts/verify.sh`, `.github/workflows/ci.yml`, `.github/workflows/desktop-e2e.yml`, Cargo nextest and existing test targets. Operational dependencies: GitHub Actions access and applicable OS runners. A hosted run may still fail or be unavailable, in which case C003 must not be marked strictly closed.

## 3. Evidence of original verification gap

At audit `a54d18c7`:

- Four introduced static guards exist but are **not** invoked by canonical `scripts/verify.sh quick`: `check_mcp_broker_adapter.py`, `check_tool_descriptor_snapshot.py`, `check_descendant_tool_ceiling.py`, and `check_tool_disclosure_wire.py`.
- `.github/workflows/ci.yml` explicitly runs selected Python guards and the broad nextest suite; it does not run these four. Hosted CI for this branch had zero runs at audit.
- `plans/closure/agent-tool-runtime-reconciliation/001-status.md` to `004-status.md` contain `Pending commit` placeholders, despite later implementation commits `832205c7e` (M001/M002), `a483885c6` (M003/M004) and closure-docs `a54d18c7e`. They are historical closure records: do not silently rewrite them.
- Branch comparison at audit: implementation head `a54d18c7e` was ahead by nine commits and behind `main` by eight. Full workspace and `server,plugins,lsp-test-support` nextest/hosted/native-platform checks were not run on the branch's exact implementation SHA.
- C001/C002 findings were missed by green focused tests, so a clean quick/Clippy exit alone cannot substitute for positive/negative executions.

## 4. Invariants and non-goals

Canonical quick, CI and local guard behavior must match; a developer cannot merge a runtime diff that fails any required tool-runtime guard merely because that guard was optional/manual. Guard scripts must check actual source ownership boundaries, not only one expected string location; they must include self-tests or mutation negative evidence. Do not let a static guard silently skip missing files or return zero with no assertions.

Existing unrelated CI, Rust-version, platform and budget policies must remain unchanged. No new release automation, protected-branch setting, customer feature gate, native provider allowlist, broad integration framework or CI provider migration.

Historical closures remain immutable; a new authoritative C003 closure should link those records and implementation SHAs. Stop if reconciliation drags in unrelated security/sandbox changes: register a separate conflict plan or split integration.

## 5. Scope and required production/infrastructure changes

### Core/domain and storage

No new production domain behavior or storage migration. Consume accepted C001/C002 code. Runtime changes in C003 may be only narrowly scoped rebase/conflict fixes with dedicated tests and traceability. No authority change under the guise of a merge conflict resolution.

### Protocol/DTO

None expected; record any schema compatibility changes already made by C001/C002. Preserve `parent_authority` additive JSON and provider `FullCompatibility` gating.

### Runtime and concurrency

Test C001/C002’s shared execution paths on the exact integration SHA, including multiple simultaneous calls, cancellation, nested spawns and non-idempotent timeout. If live MCP/provider tests are unavailable, deterministic fake-server fixtures remain required; do not imply that fake tests establish real-client interop.

### CI/verification ownership

1. Add all four predecessor guards (plus C001/C002 added guard scripts) to `scripts/verify.sh quick` with explicit commands, deterministic order and fail-on-nonzero; they then run automatically in `verify.sh full`.
2. Add equivalent explicit guard steps to `.github/workflows/ci.yml` or have the workflow call the canonical guard-only entry consistently; no duplicate policy definitions that can drift. Preserve docs-only path-ignore and relevant-PR optimization only if they still allow guards to run on changes to guard source/config/call sites.
3. Add or verify `--self-test` / mutation negative fixtures for every guard. When a guard lacks self-test support, add the smallest dependency-free test exercising both one allowed and one forbidden fixture.
4. Update `architecture/testing.md` and owner docs so the changed guard contract is discoverable; operator messages should identify the violated invariant and source file, without logging secrets.

### Main integration and status ownership

Resolve main drift only after C001/C002 accepted evidence. Compare file and behavioral overlap, especially `tool_batch.rs`, `worker.rs`, `request_preparation.rs`, provider wire, sandbox and test infrastructure. Rebase/merge with traceable exact commits; do not force-push someone else's work and do not infer successful integration from a conflict-free textual merge. Update the registry and the corrective addendum only after exact revision verification. Existing M001–M004 closure records are kept unchanged; the new C003 record links them and correct SHA ledger.

## 6. Ordered work packages

### Work package A — Dependency acceptance and change inventory

Required changes: confirm C001/C002 closures; diff against current main and inspect overlapping files, test/guard validity and CI triggers.

Acceptance evidence: explicit dependency SHAs, compare stats and conflict-disposition ledger. Do not perform integration until both prerequisites are closed.

### Work package B — Enforce guarded invariants

Required changes: wire `check_mcp_broker_adapter.py`, `check_tool_descriptor_snapshot.py`, `check_descendant_tool_ceiling.py`, `check_tool_disclosure_wire.py`, plus C001/C002 guards, into quick and hosted CI; add negative self-tests.

Acceptance evidence: executing `bash scripts/verify.sh quick` runs all guarded checks; deliberately failing each guard exits nonzero in isolated fixture; CI logs show each guarded step.

### Work package C — Reconcile with main

Required changes: obtain current main SHA, safely integrate implementation; preserve all unrelated changes. Re-run compiled/runtime and test fixtures after resolving conflicts.

Acceptance evidence: diff against both parents, no lost commits, stable tool identity/wire/authority APIs and clean quick/local full checks.

### Work package D — Hosted and platform qualification

Required changes: open/update integration PR if required by project handoff workflow; obtain GitHub `CI / verify` and Desktop E2E for **exact** reconciled SHA, verify workflow freshness (not an older SHA/attempt). Native macOS/Windows as available; if absent record explicitly and honor existing platform gate policy.

Acceptance evidence: CI URLs, exact HEAD, statuses, step logs, counts and skipped tests; no claiming green for pending or unavailable runs.

### Work package E — Closure and release-handback

Required changes: publish C003 record, update corrective roadmap + registry; accurately classify residual high/medium/low findings and merge recommendation.

Acceptance evidence: independent trace from M001–M004 commits to corrective acceptance, QA, CI, and target branch.

## 7. Failure, cancellation, restart and contention semantics

Qualification must include an outer batch cancelled during MCP transport (single remote side effect at most); simultaneous child/grandchild calls with no privilege widening or nested locks; stale tool descriptor and plugin activation at turn boundary; durable child restart with origin-bound snapshot. CI cancellation or partial failure is not green. An unavailable hosted workflow is an **operational blocker**; the plan cannot convert it to a passing test by reasoning about source. Guard runs must be deterministic offline and must not require credentials.

## 8. Compatibility/migration

No new store or tool-name migration. The merged branch must remain compatible with current main’s user-visible interfaces and security changes. The provider-native tool-search allowlists remain empty unless a separate live qualification change explicitly authorizes enablement. The suite must run on exact revision after conflict resolution, not only on pre-rebase implementation SHAs.

## 9. Required tests and verification commands

At a minimum, on exact reconciled head:

```bash
python3 scripts/check_mcp_broker_adapter.py
python3 scripts/check_tool_descriptor_snapshot.py
python3 scripts/check_descendant_tool_ceiling.py
python3 scripts/check_tool_disclosure_wire.py
# Additional C001/C002 guards and their self-test invocations as installed.
bash scripts/verify.sh quick
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p codegg --test agent_tool_m002_discovery
cargo test -p codegg --test agent_tool_m003_descendant
cargo test -p codegg --test provider_tool_disclosure_wire
cargo test -p codegg --test agent_loop_harness
cargo test -p codegg --test fake_eggsearch_mcp
cargo test -p codegg --test tool_broker_integration
cargo test -p codegg --test subagent
bash scripts/verify.sh full
```

`verify.sh full` includes `cargo nextest run --workspace --locked --profile ci` and `cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci`. Running locally is useful but not a substitute for the project's required hosted CI run. Use actual test target names and report replacements. The Desktop E2E workflow is independent; verify its triggering conditions and results rather than claiming `verify.sh` runs it.

## 10. Documentation updates

- `architecture/testing.md` — quick and CI tool-boundary guard catalog, false positive/negative tests, host evidence rules.
- `architecture/tool_broker.md`, `architecture/agent-tool-surface.md` — link C001/C002 accepted closures and qualification.
- `plans/subsystems/agent-tool-runtime-reconciliation-post-closure-corrective-addendum.md` and `plans/registry.md` — status only when verified.
- New C003 closure record with exact SHAs and CI hyperlinks. Original M001–M004 historical records untouched.

## 11. Acceptance criteria

- C001 and C002 closure accepted and their regression tests are present in the integrated source.
- All four predecessor guards plus any new corrective guards are executed by quick and hosted CI; positive/negative self-tests demonstrate enforcement rather than vacuous success.
- No process-execution, MCP or plugin double-dispatch through caller retries; nested legal delegation works; forged/widened-effect descendant calls cannot execute; documented live-mode gating stays conservative.
- The integrated branch is reconciled to current main without unrelated history loss; code compiles and all required quick/full tests pass at exact SHA.
- Hosted `CI / verify` and Desktop E2E green for exact SHA, or an explicitly blocked disposition with precise evidence. No untested platform claimed green.
- A complete SHA ledger and required closure record exist; no `Pending commit` placeholders in the **new** closure ledger.

## 12. Stop conditions

Stop for missing C001/C002 closures, merge conflicts affecting unrelated architecture, guard self-tests unable to demonstrate detection, test flakes obscuring security proof, inadequate CI permissions, missing hosted runs, or platform capability unavailable for a required hard gate. Report a specific blocker and avoid false strict closure.

## 13. Closure evidence

Write `plans/closure/agent-tool-runtime-reconciliation-post-closure-corrective/003-status.md` with exact implementation/merge SHAs, dependencies, test matrix and counts, guard output including negative fixtures, hosted CI/desktop links on same SHA, feature/OS limitations, before/after retry/nested/effect outcomes, no-lost-work main integration disposition and recommendation (closed/conditional/blocked). Update registry and roadmap only after this record is accepted.

## 14. Handoff notes

Hosted branch CI may not run without a pull request because `.github/workflows/ci.yml` is `pull_request` + `push: main` gated; plan for a PR when ready, but do not merge solely to force CI. CI `paths-ignore` excludes docs-only changes, so implementation/guard edits must be included in qualified PR. Use the exact branch head, not the historical closure SHA.

# Harness Lab — Mainline Integration and Long-Horizon Qualification Addendum

Status: **active planning**; H001 and H002 ready; H003 ready for offline fixture work (soft interface with H002); H004 blocked on H001–H003 closure and fresh live-budget admission.

Reviewed base: `plans/harness-lab-continuation-20261026@cd9fde42491c85941c31efbc2d5d200e8db4d638` (2026-10-27). Re-read latest refs and status before modifying source.

## 1. Purpose and scope

Finish the already working Harness Lab continuation by (1) integrating the proven correction set into actual `main`, (2) reconciling C004's obsolete **stall-terminal benchmark expectation** with the accepted ADR-0040 Option B, and (3) measuring sustained, causally meaningful developer workflows rather than repeated small isolated tasks.

This is **qualification of CodeGG's native harness**, not a new long-horizon scheduler, job system, broker, policy engine, model router or external service. It does not reopen M001–M005, C001–C007, D001–D003, Agent Recovery C002, or the closed Long-Horizon Work Execution M001–M005 merely to repeat already passing tests. Any defect discovered by the new evidence is handed to the true owning subsystem with a focused failing-first corrective, not fixed speculatively here.

### Provider boundary (explicit user constraint)

**Exactly one CodeGG provider integration is in scope: `opencode_go`.** The request route is `opencode_go/<model-id>`. OpenCode Go acts as the proxy/aggregator; the upstream organizations servicing model IDs are not separate CodeGG provider integrations for this work. An `openai`, `anthropic`, `google`, `minimax` or any other native adapter is **not** a hard dependency, target, fallback, parity test or correctness gate here.

Model IDs behind `opencode_go` are *serving routes*, not requirements for multiple providers. No two-model, two-upstream, multi-vendor or multi-adapter comparison is required for closure. The same stable, currently routable `opencode_go/muse-spark-1.3-contributor` route may be used for the entire qualification; any change of serving route is a **separately identified, budget-admitted experiment** rather than implicit provider coverage.

### Historical evidence and limitations

- Branch `plans/harness-lab-continuation-20261026` is 24 commits ahead of `main@957fcbf00e17bf108429f0d75dcbf760736ae511` at review, with **no PR** for that branch. Its implementation SHA `56279dfc7dc8ee82f0239d6412b4fa50a42a74d6` has successful exact-head CI `40383901325`; later two commits are documentation, closure, and live-measurement receipts. A CI success on the branch is not a success on the merge commit.
- D001 mainline-derived headless path binding, D002 typed native→ACP tool terminal truth, D003 sustained event integrity and Recovery C002 are all closed on the continuation branch; E001/E002 strict Clippy and SQLite restart CI corrections closed there.
- D003 separately authorized live sample: 63/63 completed and observed, zero gaps; one-sided 95% upper bound 4.643845% **conditional on independent trials**, for one host, pinned binary, `opencode_go/muse-spark-1.3-contributor` and alternating small `worklog` / `ranges` tasks. This neither establishes errorlessness nor long-session autonomy.
- C004 remains conditional solely because the legacy protected control `repeated-identical-failure-stall` expects a terminal stall. C005's five live samples did not repeat the identical failed read. ADR-0040 Option B expressly forbids treating repeated call identity or unknown progress alone as terminal evidence; Recovery C002 removed unused `Stalled` variants. This is a **contract mismatch**, not permission to reintroduce false stall termination.
- Existing `dev/harness_lab/fixtures/agent_autonomy/cases.jsonl` has genuine supported Rust compile/debug and background-job scenarios already; delegated workstreams and isolated MCP/LSP cases are explicitly **deferred** for missing infrastructure/verifier contracts. Do not promote those to supported because another repository feature appears closed.

## 2. Authority and unchanged invariants

References: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/001-terminology-and-domain-model.md`; `plans/003-planning-process.md`; accepted `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`; `plans/subsystems/harness-lab-roadmap.md`; `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md`; `plans/subsystems/harness-lab-continuation-corrective-addendum.md`; relevant original closure records.

CodeGG daemon + broker remain the execution authority. The Python harness owns private test lanes, immutable fixtures, evaluator and budget admission. Candidate-proposing OpenCode CLI does not approve or merge code; its own CLI provider namespace must not be confused with CodeGG's `opencode_go` provider. Every provider-backed experiment requires a **new explicit operator-approved route and enforceable request/time/spend authority**. No authorization is inferred from earlier successful usage or from a subscription. No secrets in receipts.

No obligatory VM/container, disposable enterprise infrastructure, SBC compilation/startup, uncontrolled production filesystem, automatic push/merge, blanket permission escalation or unrecoverable child replay. Preserve existing immutability of historical ledger rows, fixture manifests and acceptance scores.

## 3. Work classes and sequencing

| Milestone | Primary class | Status | Dependency |
|---|---|---|---|
| H001 — Merge-ready mainline promotion with exact-merge-head evidence | invariant | **ready** | Existing continuation branch and source CI success |
| H002 — Versioned replacement for obsolete stall-stop control | invariant | **ready** | Accepted ADR-0040 and closed Recovery C002 |
| H003 — Realistic long-horizon Rust/job test corpus | capability | **ready for offline work** | Existing native Harness Lab; H002 interface is **soft** for later score aggregation |
| H004 — Single-provider live qualification and final status reconciliation | capability | **blocked** | H001 **hard**; H002 and H003 **hard**; **operational** fresh `opencode_go` budget approval and host feasibility |

H001–H003 may progress in parallel in distinct worktrees; H004 is a separately admitted handoff. CI may verify offline fixtures at low cost without a provider account.

## 4. Milestone contracts

### H001 — Mainline integration and exact-resulting-head qualification

`plans/implementation/harness-lab-mainline-qualification/001-mainline-integration-and-exact-head-verification.md`

Review the 24-commit continuation diff, source and fixture changes, planning-only trailing commits and PR state. Produce the ordinary reviewable branch/PR, reconcile concurrent `main` without weakening unrelated work, and verify the **actual resulting merge commit** under strict formatting/Clippy/guards, 16k+ workspace test sweep and relevant platform workflows. Branch success does not suffice. No automated merge without maintainer review; do not conflate plan-registration with code landing.

### H002 — ADR-consistent protected-control contract

`plans/implementation/harness-lab-mainline-qualification/002-adr-consistent-stall-control-contract.md`

Retain historical v1 manifest/receipts and their negative evidence byte-for-byte. Add a **new, separately versioned** control/score fixture with correct predicates: unconfirmed progress cannot produce stall, confirmed stagnation produces bounded recovery guidance and documented run-budget terminal when applicable, and false stalls are regressions. Guarantee at least one instrumented deterministic repeated-call path enters the intended branch; a model declining to repeat is **not_exercised**, not a pass/failure. Never edit v1 expected terminals to retrofit a win. Provide a disposition of C004's conditional gate by accepted ADR and qualified successor evidence, not by a dishonest historical pass.

### H003 — Cohesive production-path coding scenarios

`plans/implementation/harness-lab-mainline-qualification/003-long-horizon-rust-and-job-fixtures.md`

Add/extend meaningful, isolated Rust edit→compile→test→failure repair cycles; genuinely asynchronous background compilation while agent does other work; resumed job/cancellation/restart with no duplicate dispatch; and multi-stage read/search/context recovery in a **single coherent work session**. Compare against prior supported cases to avoid duplicate synthetic tests. Keep hidden verifiers outside workspace, deterministic local fake provider (and offline recorded ACP scenarios) for CI, per-worktree Cargo target/artifact coordination and truthful time/CPU/RSS evidence. Deferred delegated/MCP/LSP cases are not new hard requirements.

### H004 — Bounded live sustained qualification via OpenCode Go only

`plans/implementation/harness-lab-mainline-qualification/004-single-opencode-go-sustained-qualification.md`

After H001–H003 close, run **one explicitly approved** `opencode_go/<model-id>` serving route over 30–60 minute *coherent tasks*, not 63 tiny independent trials; record meaningful progress, recovery, job wait, tool errors, code/test verifier output, all prompt/tool provider requests, provider-reported tokens/null costs, timeouts and cleanup. Failure injection is separately marked. Compare reproducibility against an immutable task baseline, **not against a different provider**. Require a genuine independent acceptance test and no invisible retries or hard-coded forced stall terminal. When budget approval or route access is missing, stop and record H004 blocked; do not start live work using past approval.

## 5. Cross-cutting security, compatibility and closure

- **Session/worktree correctness:** daemon-owned root, no scope widening through shared Cargo output; isolated jobs/subagents only when their public contracts are closed/verified for the actual selected mode.
- **Cancellation/restart:** state continuation and job waits are distinguishable from provider stream idle; unknown tool terminal and ACP gaps stay unmeasured. No duplicate mutating execution on reconnect.
- **Fixture integrity:** immutable manifests, hidden verifiers and holdout outside the candidate's writable scope; versioned benchmark schemas and recorded digests; scoring must discriminate “never exercised”, “observed correct”, and “observed failure”.
- **Measurement:** include per-case request/tool-count, wallclock, progress transitions, job start/wait completion, exact SHA/binary digest, failure class, policy, evidence completeness and shutdown; no invented dollars or claims beyond one serving route.
- **Documentation:** record any migration of benchmark fixture and legacy evidence; no claim about upstream-proxy diversity, multi-provider parity or model ranking.
- **Closure:** new records in `plans/closure/harness-lab-mainline-qualification/NNN-status.md`, with implementation/PR SHAs, requirement-to-evidence table, failing-first tests, whole suite/CI, security/privacy, operational budget receipts and explicitly unresolved items. Update registry and roadmap only after actual proof. The historical M/C/D closure documents are not rewritten.

## 6. Status and handoff

| Milestone | Status | Plan | Closure |
|---|---|---|---|
| H001 | ready | `plans/implementation/harness-lab-mainline-qualification/001-mainline-integration-and-exact-head-verification.md` | pending |
| H002 | ready | `plans/implementation/harness-lab-mainline-qualification/002-adr-consistent-stall-control-contract.md` | pending |
| H003 | ready for offline work | `plans/implementation/harness-lab-mainline-qualification/003-long-horizon-rust-and-job-fixtures.md` | pending |
| H004 | blocked | `plans/implementation/harness-lab-mainline-qualification/004-single-opencode-go-sustained-qualification.md` | H001–H003 and fresh spending authorization |

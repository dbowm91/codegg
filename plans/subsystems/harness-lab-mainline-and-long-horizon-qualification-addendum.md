# Harness Lab — Mainline Integration and Long-Horizon Qualification Addendum

Status: **active**; H001–H003 qualified, H004 conditionally closed, H005 not sustained-qualified and H006 corrective implementation merged (PR #216) without sustained qualification. H007 fixture/scoring implementation merged via PR #217 (`99cecc050e`); its seven live attempts across three explicit authorizations all failed sustained qualification. Two ended with typed provider timeouts, others with implementation, verifier, repair or job-overlap shortfalls; none reached 30 minutes of active work. All seven recorded complete observed ACP tool-terminal pairing and zero reported gaps. No daemon/scheduler/ACP defect is established by these runs. The independent successor is Harness Lab self-improvement resumption S001–S006 (PR #219); all previous live authorizations are consumed.

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
| H001 — Merge-ready mainline promotion with exact-merge-head evidence | invariant | **closed** | PR #205 merged at `9185b9c34ce30fd52a41aacd0aaae75f39e54895`; exact resulting-main CI attempt 3 and all applicable platform workflows passed. Closure: `plans/closure/harness-lab-mainline-qualification/001-status.md` |
| H002 — Versioned replacement for obsolete stall-stop control | invariant | **closed** | Accepted ADR-0040 and closed Recovery C002; v2 deterministic native controls qualified |
| H003 — Realistic long-horizon Rust/job test corpus | capability | **closed for offline native qualification** | Coherent Rust repair, real background jobs, holdout verifier and two isolated lanes; context epoch instrumentation remains unavailable |
| H004 — Single-provider live qualification and final status reconciliation | capability | **conditionally closed; measurement complete, sustained target not demonstrated** | One authorized Muse Spark 1.3 Contributor run used 18/60 requests, solved the fixture, passed the hidden verifier and observed zero event gaps in 112.7 seconds. A 30–60 minute task and prolonged job wait were not demonstrated; future work requires a new plan and authorization. Closure: `plans/closure/harness-lab-mainline-qualification/004-status.md` |
| H005 — Genuine sustained multi-stage Rust workload | capability | **blocked — sole live attempt consumed; not sustained-qualified** | The multi-crate event-ledger task and verifier are implemented. The operator selected the newer stored account and the one approved Muse Spark attempt ran for 10m14s, failed the hidden verifier, and observed seven submitted jobs with null projected statuses. The lane missed `CODEGG_HARNESS_MAX_PROVIDER_REQUESTS`, leaving actual request count unknown. Initial cap binding and the suite-wide remaining allowance correction passed focused offline tests and exact-head hosted CI run [40636905420](https://github.com/dbowm91/codegg/actions/runs/40636905420). No live authorization remains. Closure: `plans/closure/harness-lab-mainline-qualification/005-status.md`. |
| H006 — Job-observable sustained corrective qualification | capability | **corrective implementation successfully merged in PR #216; sustained qualification unachieved after three consumed live authorizations** | PR #216 merged at `f3c0a9e2`; exact-head CI 40897534593 passed. The latest live task and verifier passed all five checks, with two repairs, exact-ID running/completed evidence, and zero ACP gaps, but active work ended at 10m34s, below 30 minutes. The failed sustained threshold remains explicit. No daemon, scheduler, or ACP event-loss defect was observed. Closure: `plans/closure/harness-lab-mainline-qualification/006-status.md`. |
| H007 — Deep Rust workload and separate horizon measures | capability | **implementation merged by PR #217; seven live attempts, three consumed authorizations, none sustained-qualified** | Attempts 1 and 3 ended with typed `provider_timeout`. Attempts 2 and 7 showed exact-ID running/completed job observation with productive overlap, while implementation, repair and/or independent verifier conditions failed. None completed both repairs and all qualification gates; longest observed active span was 674 s (required 1,800 s). All seven had complete observed ACP tool-event pairs with zero gaps. Billed USD remains unknown. Closure: `plans/closure/harness-lab-mainline-qualification/007-status.md`. The separate self-improvement resumption line owns next diagnostics and controller requalification; no additional live calls are authorized here. |

H001–H004 are recorded in their closure documents. H002 and H003 remain closed at their original scopes. H004's limited measurement does not establish sustained reliability. H005's offline implementation is complete, but its single live attempt was consumed without qualification; the lane omitted the request-cap environment, so actual request usage is unknown and no remainder is claimed. H006's corrective implementation merged successfully in PR #216, but its sustained objective remains unqualified after three consumed authorizations. The latest H006 receipt shows a completed task, two repair cycles, complete ACP event capture, and an exact-ID job observation through both `running` and `completed`; the independent verifier passed. The remaining gap is the 30-minute observation horizon: the model completed correctly in under eleven minutes. H007 owns workload calibration and separates task performance from horizon reliability; no daemon, scheduler, or ACP event-loss defect was found. CI may verify offline fixtures without a provider account.

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

One explicitly approved `opencode_go/muse-spark-1.3-contributor` session ran after H001–H003 closed. It used 18/60 admitted requests, completed in 112.7 seconds, solved the fixture, passed the hidden verifier, and observed zero tool-event gaps. The task completed far below the 30–60 minute sustained-horizon target; the run therefore closes as a measurement with a named qualification limit, not as long-session reliability evidence. The live authorization is consumed. Any longer coherent task requires its own design and fresh approval. See `plans/closure/harness-lab-mainline-qualification/004-status.md` and its frozen JSONL receipt.

### H005 — Genuine sustained multi-stage Rust development and continuation

`plans/implementation/harness-lab-mainline-qualification/005-genuine-sustained-multi-stage-rust-workload.md`

H005 defines a new multi-crate event-ledger project with dependent ingestion, indexing, journal-recovery and CLI stages; a substantial deterministic corpus; two measured failure-recovery cycles; and a CodeGG-owned workspace test job that must remain active while the same agent implements downstream work. Its one authorized attempt used `opencode_go/muse-spark-1.3-contributor` and the selected newer stored account, but ended after 10m14s without passing the verifier. Seven jobs were submitted with null projected statuses. The lane lacked `CODEGG_HARNESS_MAX_PROVIDER_REQUESTS`, so actual request usage is unknown and the authorization is consumed. H005 remains unqualified regardless of its offline CI result.

### H006 — Job-observable sustained corrective qualification

`plans/implementation/harness-lab-mainline-qualification/006-job-observable-sustained-corrective-qualification.md`

H006's corrective implementation is successfully integrated by PR #216. Its sustained live qualification remains unachieved: the third attempt completed the task, passed the independent verifier and repair checks, and observed an exact-ID job running and completed with complete ACP event capture, but ended before 30 minutes. The next work is H007 workload calibration; it must report task outcome separately from the sustained-horizon observation. No daemon, scheduler, or ACP event-loss defect was found. H006's three authorizations are consumed.

### H007 — Deep Rust workload and separate horizon measures

`plans/implementation/harness-lab-mainline-qualification/007-deep-workload-and-separate-horizon-measures.md`

H007's offline calibration and implementation merged via PR #217 at `99cecc050e`; its feature head `ab22d66` passed [hosted Verify](https://github.com/dbowm91/codegg/actions/runs/40974217864). Confirm resulting-mainline verification separately. The seven 2026-10-29 live attempts, under three explicitly granted authorizations, are preserved in `plans/closure/harness-lab-mainline-qualification/007-status.md`; none met full task-plus-30-minute horizon qualification. Two ended with typed `provider_timeout`; others had implementation/verifier/repair/job requirements incomplete. Attempts 2 and 7 demonstrated exact-ID running/completed job overlap. Every attempted run reported paired observed ACP tool terminals and zero gaps. Distinguish task result, route failure and horizon; no systematic scheduler/ACP defect has been shown. The next work is `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md` (S001–S006), independent of 30-minute sustained success. Live provider execution still requires fresh authorization.

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
| H001 | closed | `plans/implementation/harness-lab-mainline-qualification/001-mainline-integration-and-exact-head-verification.md` | PR #205 merged at `9185b9c34ce30fd52a41aacd0aaae75f39e54895`; exact resulting-main CI and applicable platform workflows passed; `001-status.md` |
| H002 | closed | `plans/implementation/harness-lab-mainline-qualification/002-adr-consistent-stall-control-contract.md` | `plans/closure/harness-lab-mainline-qualification/002-status.md` |
| H003 | closed for offline qualification | `plans/implementation/harness-lab-mainline-qualification/003-long-horizon-rust-and-job-fixtures.md` | `plans/closure/harness-lab-mainline-qualification/003-status.md` |
| H004 | conditionally closed — measurement complete; sustained target not demonstrated | `plans/implementation/harness-lab-mainline-qualification/004-single-opencode-go-sustained-qualification.md` | One authorized Muse Spark 1.3 Contributor session used 18/60 requests, completed in 112.7 seconds, passed the independent verifier, and observed zero event gaps. A 30–60 minute coherent task remains future work requiring separate design and authorization; see `plans/closure/harness-lab-mainline-qualification/004-status.md`. |
| H005 | blocked — sole live attempt consumed; sustained objective not qualified | `plans/implementation/harness-lab-mainline-qualification/005-genuine-sustained-multi-stage-rust-workload.md` | The selected newer credential was used. The attempt ended after 10m14s without passing its verifier or duration threshold; seven submitted jobs projected null status. Request-cap environment binding was missing, so actual provider-request use is unknown. Offline cap wiring is corrected and the implementation head passed hosted CI. No live authorization remains. Closure/evidence: `plans/closure/harness-lab-mainline-qualification/005-status.md`. |
| H006 | three live attempts consumed without sustained qualification; latest task and verifier passed but duration threshold did not; workload calibration owns the next corrective | `plans/implementation/harness-lab-mainline-qualification/006-job-observable-sustained-corrective-qualification.md` | The latest live result took 10m43s and used 21/120 requests. The agent completed the task, passed all five hidden verifier checks, completed two repair cycles, observed the exact same job ID as `running` and `completed`, and recorded 31/31 ACP tool terminals with zero gaps. Its 10m34s active-work span is below the required 30 minutes. No daemon, scheduler, or ACP event-loss defect was produced. All live authorization is consumed. Closure: `plans/closure/harness-lab-mainline-qualification/006-status.md`. |
| H007 | fixture/scoring implementation merged by PR #217; seven live attempts/three authorizations consumed without sustained qualification | `plans/implementation/harness-lab-mainline-qualification/007-deep-workload-and-separate-horizon-measures.md` | Two typed provider timeouts, incomplete implementation/repair/overlap on other attempts; all observed event terminals paired, no gaps, active span always below 30 minutes. No full qualification or billed USD claim. See `plans/closure/harness-lab-mainline-qualification/007-status.md`. Successor: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md`. |

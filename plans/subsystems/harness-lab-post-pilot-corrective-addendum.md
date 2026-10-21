# Harness Lab Post-Pilot Corrective Addendum

Status: conditionally closed — C001 and C002 closed; C003 and C004 offline preparation closed, with both live halves awaiting live-provider authorization

Supersedes: **no original closure**. The historical M001–M003 strict closures remain intact. M004 and M005 stay **conditionally closed** until their outstanding evidence is discharged. See `plans/subsystems/harness-lab-roadmap.md`, `plans/closure/harness-lab/004-status.md`, `005-status.md`.

Long-term references:
- `plans/000-long-term-specification.md#23-acp-boundary`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`

References: `architecture/acp.md`, `architecture/run_store.md`, `dev/harness_lab/README.md`, `dev/harness_lab/improve.py`, `dev/harness_lab/ledger.py`, `dev/harness_lab/supervisor.py`, `plans/implementation/harness-lab/004-opencode-proposer-and-candidate-gates.md`, `005-optimization-controller-and-agent-pilot.md`, `plans/closure/harness-lab/evidence/live-pilot-20261021.json`.

Baseline inspected: `main` at `2a9fc376707ef14dab4a2f9a27f6aba8ac00dd59`, 2026-10-21. Agents MUST re-check HEAD, branch conflicts and merged successors before coding.

## 1. Purpose and ownership boundary

Qualify the **developer-only**, Python-driven, private-daemon CodeGG Harness Lab as a trustworthy instrument for improving the agent-facing harness, not CodeGG product features or general model performance. Close observed defects in failure/usage evidence and optimize/evaluator correctness, then prove an actual bounded OpenCode proposal/build/retest attempt, followed by sustained real-agent workload evaluation.

Python remains the stable, independent experiment controller and scoring authority. OpenCode is optional and is only the **source-code proposer**; CodeGG ACP talks to the real native daemon/scheduler/broker. A subject candidate cannot modify its independent verifier/hidden holdout, experiment budget, ledger or controller. Changes to those contracts require a separately reviewed controller version, not candidate promotion.

The approved environment is the user's **trusted separate development workstation**: **no VM/container provision, no local-isolation research milestone, no request for interactive permissions during authorized benchmark turns**. Explicit `yolo + full-host` exercises true tool freedom, or `yolo + workspace-write` is a separately qualified more restricted profile; explicit Deny still wins. The multiple per-root daemon lanes, unique databases, worktrees, owned-process cleanup and production-default-daemon protections implemented by M001/M002 remain mandatory.

## 2. Corrective findings, underlying evidence, and why prior verification missed them

1. **F001 — Native failed-turn diagnostics/usage gap (M005).** Live Muse Spark 1.3's Task 1 emitted 7 completed tool calls, then `HarnessTurnFailure`, no changed task file, no useful provider terminal usage and no detailed error classification. Its 10 verifier tests failed, and Task 2 was stopped under the spending rule. In `src/acp.rs`, successful `TurnCompleted` returns optional `codeggUsage`; `TurnFailed` currently becomes generic ACP JSON-RPC error `-32010` with session/turn IDs only. Existing happy-path tool fixtures did not exercise real failed-provider turns with usage/disclosure.
2. **F002 — Missing enforceable spend semantics (M005).** Token counts/cost may be unavailable, especially on failures and subscription/provider adapters; an absence is never zero. The recorded pilot explicitly stopped because configured usage limits could not be independently enforced. A fixed provider account subscription is not by itself a measurable per-experiment financial hard limit. Preflight tests and happy-path usage did not prove failure-path coverage or remote/third-party quota enforcement.
3. **F003 — Acceptance criterion overly narrow / negative-controls confusion (M004).** `qualify_candidate` checks candidate expected-terminal matches, paired nonregression and an improvement where a baseline terminal differs from an expected terminal; the frozen 12-case suite already matches all expected terminals (including **intentional negatives**), so no real candidate can be accepted on that evidence. `ledger.compare` summarizes terminal labels and omits performance distributions/denominator provenance needed for efficiency-only acceptance. Correctness gates prevented false acceptance but did not prove genuine optimization.
4. **F004 — Interrupted qualification evidence gap (M004/M005).** The old M004 closure recorded fail-closed on partial candidate rows; M005 later added immutable-prefix candidate resume. It has focused tests, but there is no independent **live subprocess interruption during candidate scoring -> restart -> complete without duplicate receipts** qualification. Status must be reconciled with the new code rather than reimplementing a closed unit-test feature.
5. **F005 — OpenCode real proposer/correction/accepted fix unproven (M004).** An OpenCode 1.19.0 adapter, fake responses, scoped worktrees, required changelog, compile retry and candidate gates exist. No authenticated real OpenCode patch, compile correction and independently accepted defect fix was run. Deterministic mock tests are not interchangeable with live provider evidence.
6. **F006 — Sustained/multitask agent experience unproven (M005).** First paired task: LongCat passed 10/10 tests with usage; Muse Spark failed without terminal usage; second frozen task not run. Existing 12-case fixtures are provider-free scripted tool chains; they do not establish half-hour-plus agent continuity, real task correctness across agents/models, recovery from provider/transport faults, background jobs, subagents/worktrees, or MCP/LSP coverage. Expand only after spending and evaluator invariants are proven.

## 3. Work classification

- **Invariants:** truthful failure/usage provenance; explicit deny authority; checkpoint/ledger immutability; complete paired evidence; independent verifier and no fake productivity gain; no silent live spend beyond enforceable admission.
- **Capabilities:** first accepted/rejected real candidate correction, resumable complete runs, useful sustained native ACP benchmark suites, model/provider comparisons.
- **Infrastructure:** failure receipts, budget/usage accounting gates, versioned score contract, interrupted-run reconciliation and corpus provenance.
- **Polish:** explanatory diagnostics, reporting ergonomics and opt-in benchmark suites after reliability closure.

## 4. Dependency graph / release of work

```
C001 failure evidence + enforceable budget admission ─┐
                                                    ├── C003 real proposer/acceptance & restart qualification
C002 scoring/holdout + resume correctness ───────────┘          |
                                                               v
                                                    C004 sustained pilot/coverage & Task 2
```

- **C001 ready:** existing ACP/native telemetry and M005 partial pilot are sufficient interfaces.
- **C002 ready:** existing immutable ledger, evaluator and M004/M005 candidate controller are sufficient interfaces. Work may proceed in parallel with C001, but touching common Python code requires ordinary Git coordination.
- **C003 blocked (hard):** C001 and C002 strict closure.
- **C004 blocked (hard):** C003 strict closure for *autonomous-optimization-readiness claim*; the frozen Task 2 provider pair may be run as a separate operational action after C001 closure, provided no code changes or comparisons are called optimizer acceptance. Corpus research/fixtures are a soft, parallel dependency.
- No ADR required for these developer-only changes **provided** canonical daemon ownership, ACP v1 compatibility and production singleton policy remain unchanged. Stop/raise ADR if code needs to replace these boundaries.

## 5. Milestones

### C001 — Failure-path diagnostics and budget-stop authority
Class: invariant/capability. Plan: `plans/implementation/harness-lab-post-pilot-corrective/001-failed-turn-evidence-and-budget-authority.md`. **Closed** — `plans/closure/harness-lab-post-pilot-corrective/001-status.md`. F001/F002 resolved: every failure terminal carries an allowlisted `TurnFailureCategory` and an explicit usage provenance, and a live trial must pass the fail-closed budget admission preflight before it starts. Failure classified with safe evidence and usage provenance. An actual enforceable live-budget source is required before resuming user-approved pilot; otherwise the correct output is `BLOCKED: usage_unverifiable`. No model call needed for deterministic closure.

### C002 — Independent score contract and exact candidate resume
Class: invariant/infrastructure. Plan: `plans/implementation/harness-lab-post-pilot-corrective/002-scoring-and-resumable-qualification.md`. **Closed** — `plans/closure/harness-lab-post-pilot-corrective/002-status.md`. F003/F004 resolved: acceptance runs through a versioned score contract with protected negative controls and an explicit `insufficient_evidence` outcome, the comparator refuses an incomplete or identity-mismatched pairing, and an interrupted attempt is recorded as `interrupted_unknown` with no duplicate rows on resume. Add versioned criterion types: correctness failure recovery, reliability/efficiency paired objective with adequate evidence, and explicit negative-control preservation. Prove full child + outer checkpoint interrupted evaluation and no double-counted attempts.

### C003 — End-to-end OpenCode correction and independent acceptance
Class: capability. Plan: `plans/implementation/harness-lab-post-pilot-corrective/003-live-proposer-correction-and-acceptance.md`. **C001/C002 dependency discharged.** The remaining blocker is a live-provider authorization decision, not a code dependency: an approved model route with C001-verified spend authority, a re-pinned installed OpenCode CLI (`improve.py` still pins `1.19.0`; the installed CLI reports `1.19.3`), and a genuine externally observed harness defect. Resolve F005 and complete independent restart/accepted-fix receipts. First establish deterministic seeded-regression control to prove acceptance wiring (labeled synthetic); then run optional authenticated OpenCode on a **real externally verified harness defect** within approved bounds. If no genuine defect exists, report blocked rather than invent a production improvement. Code stays in a candidate worktree; no auto-merge.

### C004 — Sustained tool-rich ACP qualification and remaining model pair
Class: capability. Plan: `plans/implementation/harness-lab-post-pilot-corrective/004-sustained-agent-benchmark-qualification.md`. **Blocked** on C003 for final autonomy claim. The C001 precondition for the Task 2 provider pair is now discharged, so its remaining blocker is the approved usage budget. Resolve F006, complete Task 2 only with an enforceable approved usage budget, exercise 30–60 minute tool-rich real model tasks with deterministic verifiers and failure/latency/error/cost provenance, and record holdout/variance over repeated trials. Maintain no-cost CI fixtures.

## 6. Cross-cutting requirements

### Storage and migrations
No production DB schema migration by default. The external ledger/assessment/score/checkpoint schema must be versioned and must migrate or reject pre-corrective records explicitly; the original 2026-10-21 pilot JSON is **immutable**. Redact credentials/provider response bodies, prohibit hidden reasoning in public ACP/ledger.

### Protocol
ACP v1 remains a thin native adapter. Add optional namespaced terminal error `data` / observability only when canonical daemon data already exists or a bounded authorized native DTO is justified. Never forge token usage or standard ACP stop reasons; no global policy bypass.

### Concurrency, recovery and security
Support >1 concurrently bound private daemon. Candidate binary/daemon generation/candidate patch/test suite/score contract must match their frozen receipts. Restart never overwrites a prior terminal row; ambiguous in-flight attempts are explicit and reconciled, not silently replayed. Full-host is trusted-user authority; workspace-write is not equivalent. The Python supervisor owns process groups and rollback, OpenCode cannot self-accept.

### Metrics and budgets
Observed usage, unavailable usage, and upper-bound reserved budget are distinct. Provider-side caps are only claimed if verifiably enforced for the selected route/failure path. No locally estimated tokens or prices may be labeled actual or treated as enforceable spend ceilings. If a hard dollar cap is requested and only post-hoc usage exists, decline launch absent a proper source of authority.

### Portability and external dependencies
Developer Python is optional. OpenCode is pinned/qualified and optional; `ManualProposer` works offline. Ordinary CI MUST be credential-free, deterministic, and bounded; live model runs are manual and separately labeled.

## 7. Verification and completion

Focused `python3 -m unittest discover -s dev/harness_lab/tests -v`, Rust ACP/provider/daemon tests as affected, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `scripts/verify.sh quick`, and real private-lane tool-path tests. A green CI run on another SHA does not qualify new code. Each corrective must write `plans/closure/harness-lab-post-pilot-corrective/NNN-status.md` with requirement/evidence mapping, exact SHA, tests run, negative/fault injections, credential-safety, unresolved findings and unblock audit.

Final completion requires truthful failed-turn evidence or authoritative stop, complete independent candidate scoring and resume, actual OpenCode proposer/correction evidence and a real eligible accepted/rejected trace, plus the sustained pilot with a defensible denominator and approved budget. If unavailable provider evidence prevents one part, record conditional—not false full closure.

## 8. Milestone status

| Milestone | Status | Plan | Closure | Blocker |
|---|---|---|---|---|
| C001 | closed | `implementation/harness-lab-post-pilot-corrective/001-failed-turn-evidence-and-budget-authority.md` | `closure/harness-lab-post-pilot-corrective/001-status.md` | none |
| C002 | closed | `implementation/harness-lab-post-pilot-corrective/002-scoring-and-resumable-qualification.md` | `closure/harness-lab-post-pilot-corrective/002-status.md` | none |
| C003 | conditionally closed (dependency discharged) | `implementation/harness-lab-post-pilot-corrective/003-live-proposer-correction-and-acceptance.md` | `closure/harness-lab-post-pilot-corrective/003-status.md` | live-provider authorization: an approved model route with C001-verified spend authority, and a genuine observed defect. The OpenCode CLI pin now names the actually installed 1.19.3 and the bounded compile-correction mechanic is qualified against a labelled fault injection |
| C004 | conditionally closed (offline preparation closed) | `implementation/harness-lab-post-pilot-corrective/004-sustained-agent-benchmark-qualification.md` | `closure/harness-lab-post-pilot-corrective/004-status.md` | hard: C003 strict closure for the final autonomy claim; the offline taxonomy, loader, isolation guard and one materialized case are closed, and Task 2's remaining blocker is the approved usage budget |

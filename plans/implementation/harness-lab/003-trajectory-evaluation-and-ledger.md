# Developer Harness Lab M003 — ACP Trajectory Evaluation and Experiment Ledger

Status: blocked — hard dependencies on Harness Lab M001 and M002 closure; planning handoff prepared

Repository baseline: `f37840ef5fca36d44b35daec5da6b26cd049a3ee` (`main`, reviewed 2026-10-18). Reinspect against actual M001/M002 closure.

Source roadmap: `plans/subsystems/harness-lab-roadmap.md#7-milestones`

Long-term requirements: `plans/000-long-term-specification.md#23-acp-boundary`, `#24-protocol-and-storage-requirements`, `#26-reliability-and-recovery`, `#28-observability`, `#29-system-invariants`; `plans/001-terminology-and-domain-model.md`.

Primary class: infrastructure/capability

## 1. Objective

Extend the independently supervised, explicitly authorized real ACP subjects into an auditable **baseline agent-experience evaluation system** with deterministic task verifiers, complete run identity, categorized failure traces, controller-owned resource budgets and paired-revision comparisons. Prove assessment/replay/report; do not yet allow autonomous source editing.

## 2. Dependency readiness

Hard prerequisites: M001's concurrent daemon/ACP subject identity and data isolation; M002's requested/effective policy and truthful terminal classification. This plan is **not executable until both have closure evidence**. Fixture definitions and design may be drafted in parallel only as interface work.

## 3. Current evidence

`src/acp.rs` emits public message chunks, tool starts/completions, plus terminal prompt response. `architecture/run_store.md` documents immutable artifact/read and run records; `architecture/projection.md` supports replay/cursors, and `src/agent/progress_recovery.rs` includes repeat/stall classifications. `architecture/context-ledger.md` exposes actual model-visible tool result compression and `context_read` handles. No unified experiment-level ledger, task verifier schema, stable cost instrumentation, or paired baseline comparison has been established. The Rust `tool-advisor bench` is **tool discovery evaluation**, not a full agent trajectory benchmark, and should not be relabeled as one.

## 4. Invariants

- **Verifier decides correctness**, not agent prose, a model judge alone, or an accidental successful exit. A skipped/unknown verifier is never a pass.
- Trial, candidate, exact binary hash, provider+model, agent, resolved tool surface, configuration/asset fingerprint, ACP version, daemon generation, policy, session, turn, task fixture revision and verifier revision are joined without inferential PID/name matching.
- Distinguish raw tool output from **what model actually saw**, including truncation/projection/context handle recovery. Do not store hidden reasoning or plaintext secrets.
- All metrics must state available/unavailable and provenance. Unknown tokens/costs are `null`, not zero. A transport retry does not erase a failed attempt and a rerun does not silently replace an unsuccessful trial.
- The controller, not the subject, owns global limits and timeouts. A subject timeout, death, auth failure, denial or output truncation cannot become success by a successful controller retry.
- Cases explicitly identify supported capabilities, authorized tool scope and destructive-write expectations; no live network third-party targets required.

## 5. Scope

In: `dev/harness_lab` task/fixture/verifier schema, semantic trace normalizer (ACP plus approved native read-only receipts), budget monitor, report and comparison CLI, deterministic/fault scenarios, opt-in live-model suite.

Out: self-patching OpenCode flow (M004), VM/container setup, production telemetry exfiltration, expanding native transport beyond minimum structured read/receipts, speculative subjective global "agent quality" scalar, SBC benchmarks, release/CI always-on expensive model runs.

## 6. Required changes

### Task cases and verifiers

- Versioned compact JSON/YAML/JSONL manifest: `task_id`, `group_id` to avoid leakage across train/validation/holdout, checkout fixture/baseline, initial files and environment, prompt, target model/agent, allowed capabilities, requested policy, independent expected postconditions, verifier command/API, walltime/turn/tool/token/cost caps and cleanup.
- Include case groups: read-heavy exploration; tool discovery/no-tool; read/search/edit/test; shell compile/error correction; projected output plus `context_read` recovery; repeated-but-distinct tool calls (non-stall); identical failing repeats (bounded stall); job completion without polling; ACP cancel and resume; subagent/worktree contention (optional when native feature ready).
- For Rust build-fix cases use a **small fixture Rust workspace** separate from codegg itself; warm dependency cache where reproducible and record cold/warm state. One large main-repository build task may be an explicit opt-in, not mandatory in quick mode.
- Verifiers use filesystem content/hash, expected command exit, test assertions, expected artifact, bounded interaction semantics; model evaluator can annotate failures only.

### Recorder and ledger

- Append-only/redacted JSONL event log per trial; atomic immutable run manifest and SQLite index for query/compare. Each event includes a stable correlation key and monotonic sequence or explicitly recorded absence.
- Record tool call count/status/latency, permission allow/deny/ask, retries and recovery escalations when obtainable, external process/job waits, context artifact projection/recovery, token in/out and cost when accurately reported, walltime/CPU/RSS where measured, test correctness and terminal class.
- Implement bounded capture and retention; complete tool stdout/stderr only via authenticated artifact handles or bounded redacted snapshots; do not dump credential-bearing environment. Never infer missing usage from tool call count.
- Add a structured **observability gap report** listing fields ACP cannot presently expose. Fill through an authorized native read-only side channel or backward-compatible projection metadata only when required; do not copy whole daemon internals into ACP.

### Controller budgets and evaluation

- Externally enforce per-case timeout, per-experiment wallclock, iteration/trial count and concurrency. Track token/cost ceilings only if provider usage is trustworthy; if not observable, either stop with `budget_unverifiable` or use an explicitly marked conservative secondary estimator, never falsely assert a hard cap.
- Implement deterministic fail/stop: `passed`, `assertion_failed`, `execution_failed`, `cancelled`, `denied`, `timeout`, `budget_exhausted`, `infrastructure_error`, `indeterminate`, distinct from controller subprocess exit status.
- Compare fixed versions on paired case IDs and repeated trial seeds/conditions where supported; report successes/denominators, tool-retry friction broken out by kind, total tokens/cost, latency, crash/stall rate. Separate primary correctness floors from secondary cost/friction tradeoffs.
- Split development/validation/hidden holdout corpus by `group_id` and immutable digest; proposer never sees hidden verifier answers in the future M004 loop.
- Support `replay/report/compare` without provider calls, prove provenance and run integrity. Keep baseline frozen within an experiment.

## 7. Ordered work packages

**A — Case + verifier contract.** Produce 8–12 small developer-local deterministic cases (at least read-heavy, edit+verify, nonstall repeated exploration, correct stall, cancellation, failed command recovery, context projected output and shell process) and a versioned schema with case grouping and failure classes.

**B — Trace/ledger.** Connect M001/M002 subject observations to immutable trial/event/manifest records, normalize terminal and tool states, correlate with native run IDs and record missing telemetry rather than hallucinating. Secret and reasoning redaction tests required.

**C — Budget/cancel/retention.** Put walltime, trial/tool caps, cancellation and process cleanup under Python authority; implement resumability and checksum audit for interrupted trials.

**D — Baseline and comparison.** Run a deterministic provider-free assessment fixture suite through the actual ACP tool path. Add optional manual live-model fixture profiles and provide report/compare functionality, with task correctness independent of agent self-assessment.

## 8. Failure/restart/contention

A controller restart may resume only when ledger/candidate/daemon identity and active lease can be reconciled. Otherwise mark interrupted and start a **new** trial with a new identity. A transport timeout is not an agent timeout; record both when applicable. Explicitly bound subprocesses and background jobs so a finished foreground prompt cannot leave a trial's writer process untracked. Parallel comparisons must avoid conflicting worktrees and shared Cargo target dirs; cache only read-only dependency data or proven-safe compile cache.

## 9. Compatibility/migration

No production schema migration expected. The external ledger schema must be versioned from the first run; write migration tests if future fields are added. Native projection/RunStore interfaces remain authoritative and security-redacted. ACP v1 compatibility retained. No opt-out of secrets policy or production permissions to improve telemetry completeness.

## 10. Tests

- Unit property tests for classification, trial pairing, missing metrics, leakage-resistant splits, immutable case/version hashes, verifier skip/failure, cost budget exceeded and report reproducibility.
- Mock ACP frames covering out-of-order/stale tools, omitted terminal, duplicated response, stream crash, hidden reasoning, truncated artifacts, mismatched session/generation and unresolved permissions.
- Real deterministic CodeGG ACP suite with multi-tool tasks under both policy profiles where actually supported; at least one intentional failed verifier and one intentional cancellation/timeout, with correct nonzero controller result.
- Independent replay/compare that functions offline using frozen fixture records; retain exact failure taxonomy.

## 11. Verification commands

```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 -m dev.harness_lab smoke --subjects 2
# implement actual CLI equivalents; no paid model required:
python3 -m dev.harness_lab evaluate --suite dev/harness_lab/fixtures/smoke.jsonl
python3 -m dev.harness_lab report --latest
cargo test -p codegg --test acp_stdio
cargo fmt --all -- --check
./verify.sh quick
```

A provider-backed benchmark run is manual/opt-in and must record provider/model/cost/version; no green CI claim based on an unavailable paid provider.

## 12. Acceptance and stop conditions

One provider-free, **real tool-path** suite must yield a reproducible ledger and truthful negative results; two different subject candidates can run paired cases without shared mutable state; offline replay/compare reproduces verification and tool/recovery metrics; cancellation and budget stops are terminal and distinguishable. If a necessary metric is unavailable, mark it unavailable and do **not** invent it or block all capability, except when a configured **hard** spending cap cannot be proven.

## 13. Closure artifact

`plans/closure/harness-lab/003-status.md` with commit SHA, task fixture revision/digests, requirements-to-evidence matrix, case-by-case real ACP results, telemetry gaps, explicit optional paid-run status, replay/compare receipts, bounds/retention/redaction tests and remaining qualification needs.
